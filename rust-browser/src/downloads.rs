//! Native WebView2 download progress and cancellation support.

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DownloadUpdate {
    pub id: u64,
    pub url: String,
    pub path: String,
    pub received: u64,
    pub total: Option<u64>,
    pub state: String,
    pub error: Option<String>,
}

pub fn safe_suggested_filename(input: &str) -> String {
    let leaf = std::path::Path::new(input)
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or(input);
    let mut name: String = leaf
        .chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
            {
                '_'
            } else {
                ch
            }
        })
        .take(128)
        .collect();
    while matches!(name.chars().last(), Some(' ' | '.')) {
        name.pop();
    }
    if name.is_empty() || name == "." || name == ".." {
        return "download".into();
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit())
    {
        name.insert(0, '_');
    }
    name
}

#[cfg(test)]
mod tests {
    use super::safe_suggested_filename;

    #[test]
    fn suggested_download_name_cannot_escape_or_target_devices() {
        assert_eq!(safe_suggested_filename("../../report.pdf"), "report.pdf");
        assert_eq!(safe_suggested_filename("CON.txt"), "_CON.txt");
        assert_eq!(safe_suggested_filename("bad:name?.txt"), "bad_name_.txt");
        assert_eq!(safe_suggested_filename("..."), "download");
        assert!(safe_suggested_filename(&"x".repeat(300)).len() <= 128);
    }
}

#[cfg(windows)]
#[derive(Clone, Default)]
pub struct DownloadControl {
    operations: std::rc::Rc<
        std::cell::RefCell<
            std::collections::HashMap<
                u64,
                webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2DownloadOperation,
            >,
        >,
    >,
    next_id: std::rc::Rc<std::cell::Cell<u64>>,
    last_progress:
        std::rc::Rc<std::cell::RefCell<std::collections::HashMap<u64, std::time::Instant>>>,
}

#[cfg(windows)]
impl DownloadControl {
    pub fn attach(
        &self,
        view: &wry::WebView,
        changed: impl Fn(DownloadUpdate) + 'static,
    ) -> Result<(), String> {
        use webview2_com::{
            BytesReceivedChangedEventHandler, DownloadStartingEventHandler,
            Microsoft::Web::WebView2::Win32::*, StateChangedEventHandler, take_pwstr,
        };
        use windows::core::{Interface, PWSTR};
        use wry::WebViewExtWindows;

        let core =
            unsafe { view.controller().CoreWebView2() }.map_err(|error| error.to_string())?;
        let webview4 = core
            .cast::<ICoreWebView2_4>()
            .map_err(|error| error.to_string())?;
        let control = self.clone();
        let changed = std::rc::Rc::new(changed);
        let handler = DownloadStartingEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else {
                return Ok(());
            };
            let operation = unsafe { args.DownloadOperation() }?;
            let mut uri = PWSTR::null();
            let mut path = PWSTR::null();
            unsafe {
                operation.Uri(&mut uri)?;
                operation.ResultFilePath(&mut path)?;
            }
            let uri = take_pwstr(uri);
            let path = take_pwstr(path);
            let counter = control.next_id.get().wrapping_add(1) & 0xffff;
            control.next_id.set(counter);
            let epoch_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_millis() as u64);
            let id = epoch_ms
                .saturating_mul(65_536)
                .saturating_add(counter)
                .max(1);
            control
                .operations
                .borrow_mut()
                .insert(id, operation.clone());
            let total = download_total(&operation);
            let received = download_received(&operation);
            (changed)(DownloadUpdate {
                id,
                url: uri.clone(),
                path: path.clone(),
                received,
                total,
                state: "in_progress".into(),
                error: None,
            });

            let progress_operation = operation.clone();
            let progress_changed = changed.clone();
            let progress_times = control.last_progress.clone();
            let progress_url = uri.clone();
            let progress_path = path.clone();
            let progress_handler =
                BytesReceivedChangedEventHandler::create(Box::new(move |_, _| {
                    let now = std::time::Instant::now();
                    {
                        let mut times = progress_times.borrow_mut();
                        if times
                            .get(&id)
                            .is_some_and(|last| now.duration_since(*last).as_millis() < 120)
                        {
                            return Ok(());
                        }
                        times.insert(id, now);
                    }
                    (progress_changed)(DownloadUpdate {
                        id,
                        url: progress_url.clone(),
                        path: progress_path.clone(),
                        received: download_received(&progress_operation),
                        total: download_total(&progress_operation),
                        state: "in_progress".into(),
                        error: None,
                    });
                    Ok(())
                }));
            unsafe { operation.add_BytesReceivedChanged(&progress_handler, &mut 0) }?;

            let state_control = control.clone();
            let state_changed = changed.clone();
            let state_url = uri;
            let state_handler = StateChangedEventHandler::create(Box::new(move |download, _| {
                let Some(download) = download else {
                    return Ok(());
                };
                let mut native_state = COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS;
                unsafe {
                    download.State(&mut native_state)?;
                }
                let state = if native_state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED {
                    "completed"
                } else if native_state == COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED {
                    "interrupted"
                } else {
                    return Ok(());
                };
                let mut native_path = PWSTR::null();
                let mut reason = COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NONE;
                unsafe {
                    download.ResultFilePath(&mut native_path)?;
                    download.InterruptReason(&mut reason)?;
                }
                let error = (state == "interrupted").then(|| interrupt_reason(reason.0));
                (state_changed)(DownloadUpdate {
                    id,
                    url: state_url.clone(),
                    path: take_pwstr(native_path),
                    received: download_received(&download),
                    total: download_total(&download),
                    state: state.into(),
                    error,
                });
                state_control.last_progress.borrow_mut().remove(&id);
                state_control.operations.borrow_mut().remove(&id);
                Ok(())
            }));
            unsafe { operation.add_StateChanged(&state_handler, &mut 0) }?;
            Ok(())
        }));
        unsafe { webview4.add_DownloadStarting(&handler, &mut 0) }
            .map_err(|error| error.to_string())
    }

    pub fn cancel(&self, id: u64) -> Result<(), String> {
        let operation = self
            .operations
            .borrow()
            .get(&id)
            .cloned()
            .ok_or_else(|| "Загрузка уже завершена".to_owned())?;
        unsafe { operation.Cancel() }.map_err(|error| error.to_string())
    }
}

#[cfg(windows)]
fn download_total(
    operation: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2DownloadOperation,
) -> Option<u64> {
    let mut total = -1;
    unsafe { operation.TotalBytesToReceive(&mut total) }.ok()?;
    (total >= 0).then_some(total as u64)
}

#[cfg(windows)]
fn download_received(
    operation: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2DownloadOperation,
) -> u64 {
    let mut received = 0;
    if unsafe { operation.BytesReceived(&mut received) }.is_ok() {
        received.max(0) as u64
    } else {
        0
    }
}

#[cfg(windows)]
fn interrupt_reason(reason: i32) -> String {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    match COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON(reason) {
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_CANCELED => {
            "Загрузка отменена пользователем".into()
        }
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_DISCONNECTED
        | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_FAILED
        | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_SERVER_DOWN
        | COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_NETWORK_TIMEOUT => {
            "Ошибка сетевого соединения".into()
        }
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_ACCESS_DENIED => "Нет доступа к файлу".into(),
        COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_FILE_NO_SPACE => {
            "Недостаточно места на диске".into()
        }
        _ => format!("Загрузка прервана (код {reason})"),
    }
}
