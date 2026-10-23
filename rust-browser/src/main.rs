#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use aurora_browser::{
    downloads, extensions, https_first, privacy, site_data, site_permissions, vault,
};

use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex, RwLock},
};
use std::{fs, path::PathBuf};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    window::{Icon, Window, WindowId},
};
use wry::{PageLoadEvent, Rect, WebView, WebViewBuilder, dpi::LogicalPosition};
#[cfg(windows)]
use wry::{WebViewBuilderExtWindows, WebViewExtWindows};

const CHROME_HEIGHT: u32 = 104;
const HOME_PAGE: &str = "about:aurora";

#[derive(Debug, Deserialize)]
struct BrowserCommand {
    action: String,
    value: Option<String>,
    tab_id: Option<u64>,
}

enum UserEvent {
    Command(BrowserCommand),
    StartupTimeout,
    ShowWindow,
    PermissionsReady(u64, Result<(), String>),
    LegacyPermissions(u64, Result<Vec<site_permissions::LegacyPermission>, String>),
    SiteCookies(u64, String, Result<Vec<site_data::CookieInfo>, String>),
    SiteDataDone(u64, String, String, Result<(), String>),
    ProfileCacheDone(u64, Result<(), String>),
    HttpNavigationBlocked(u64, String),
    HttpsNavigationFailed(u64, String),
    DownloadChanged(u64, String, bool, downloads::DownloadUpdate),
    PageTitle(u64, String),
    PageLoading(u64, bool),
    PageLoaded(u64, String),
    MenuOpen(bool),
    OverlayHidden,
    OverlayReady,
    ExtensionsListed(Result<Vec<extensions::ExtensionInfo>, String>),
    ExtensionChanged(Result<(), String>),
    ExtensionInstalled(Result<extensions::ExtensionInfo, String>),
    ExtensionPrepared(Result<(PathBuf, extensions::ExtensionManifest, PathBuf), String>),
    FilterListsUpdated(Result<(Vec<String>, Arc<adblock::Engine>), String>),
}

struct BrowserTab {
    id: u64,
    title: String,
    url: String,
    is_loading: bool,
    first_load_complete: bool,
    awaiting_first_navigation: bool,
    privacy_attached: bool,
    permissions: site_permissions::SharedPolicy,
    permission_gate: Arc<std::sync::atomic::AtomicBool>,
    http_allowlist: Arc<Mutex<HashSet<String>>>,
    https_fallback_candidate: Arc<Mutex<Option<String>>>,
    https_first_enabled: Arc<std::sync::atomic::AtomicBool>,
    privacy: Arc<privacy::PrivacyState>,
    is_private: bool,
    view: WebView,
}

#[derive(Clone, Serialize, Deserialize)]
struct DownloadRecord {
    id: u64,
    tab_id: u64,
    profile_scope: String,
    private: bool,
    url: String,
    path: String,
    received: u64,
    total: Option<u64>,
    state: String,
    error: Option<String>,
    started_ms: u128,
    #[serde(default)]
    speed: Option<u64>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Bookmark {
    title: String,
    url: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct HistoryEntry {
    title: String,
    url: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct BrowserSettings {
    search_engine: String,
    animations: bool,
    theme: String,
    homepage: String,
    #[serde(default = "default_adblock_enabled")]
    adblock_enabled: bool,
    #[serde(default = "default_https_first")]
    https_first: bool,
}

fn default_adblock_enabled() -> bool {
    true
}

fn default_https_first() -> bool {
    true
}

impl Default for BrowserSettings {
    fn default() -> Self {
        Self {
            search_engine: "duckduckgo".into(),
            animations: true,
            theme: "dark".into(),
            homepage: "aurora".into(),
            adblock_enabled: true,
            https_first: true,
        }
    }
}

impl AuroraApp {
    #[cfg(not(windows))]
    fn show_extension_error(&self, error: &str) {
        if let (Some(overlay), Ok(message)) = (self.overlay.as_ref(), serde_json::to_string(error))
        {
            let _ = overlay.evaluate_script(&format!("window.showExtensionError({message})"));
        }
    }

    #[cfg(windows)]
    fn confirm_extension_permissions(
        &self,
        directory: &std::path::Path,
        manifest: &extensions::ExtensionManifest,
    ) -> bool {
        let permissions = manifest
            .permissions
            .iter()
            .chain(manifest.optional_permissions.iter())
            .cloned()
            .collect::<Vec<_>>();
        let hosts = manifest
            .host_permissions
            .iter()
            .chain(manifest.optional_host_permissions.iter())
            .cloned()
            .collect::<Vec<_>>();
        let mut review = format!(
            "Название: {}\nВерсия: {}\n\nРазрешения API:\n{}\n\nДоступ к сайтам:\n{}\n\nУстанавливайте только расширения, которым доверяете.",
            manifest.name,
            manifest.version,
            if permissions.is_empty() {
                "нет".into()
            } else {
                permissions.join(", ")
            },
            if hosts.is_empty() {
                "нет".into()
            } else {
                hosts.join("\n")
            }
        );
        if !manifest.description.is_empty() {
            review = format!("{}\n\n{}", manifest.description, review);
        }
        let Some(window) = self.window.as_ref() else {
            let _ = fs::remove_dir_all(directory);
            return false;
        };
        let answer = rfd::MessageDialog::new()
            .set_title("Проверка разрешений расширения")
            .set_description(review)
            .set_level(rfd::MessageLevel::Warning)
            .set_buttons(rfd::MessageButtons::OkCancel)
            .set_parent(window)
            .show();
        let approved = matches!(
            answer,
            rfd::MessageDialogResult::Ok | rfd::MessageDialogResult::Yes
        );
        if !approved {
            let _ = fs::remove_dir_all(directory);
        }
        approved
    }

    #[cfg(windows)]
    fn extension_paths(&self) -> Option<(PathBuf, PathBuf)> {
        let profile_dir = self.context.as_ref()?.data_directory()?.to_path_buf();
        Some((
            profile_dir.join("Extensions"),
            profile_dir.join("aurora-extensions.json"),
        ))
    }

    #[cfg(windows)]
    fn request_extensions(&mut self) {
        let Some(id) = self.active_tab else {
            return;
        };
        if self
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .is_some_and(|tab| tab.is_private)
        {
            if let Some(overlay) = self.overlay.as_ref() {
                let _ = overlay.evaluate_script("window.showExtensionError('Менеджер расширений недоступен в приватной вкладке')");
            }
            return;
        }
        let Some((_, registry)) = self.extension_paths() else {
            return;
        };
        let Some(view) = self
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .map(|tab| &tab.view)
        else {
            return;
        };
        let proxy = self.proxy.as_ref().expect("event loop proxy").clone();
        if let Err(error) = extensions::list_extensions(view, registry, move |result| {
            let _ = proxy.send_event(UserEvent::ExtensionsListed(result));
        }) && let Some(overlay) = self.overlay.as_ref()
        {
            let message = serde_json::to_string(&error)
                .unwrap_or_else(|_| "\"Не удалось получить список расширений\"".into());
            let _ = overlay.evaluate_script(&format!("window.showExtensionError({message})"));
        }
    }

    #[cfg(windows)]
    fn set_extension_enabled(&mut self, id: &str, enabled: bool) {
        let Some(tab_id) = self.active_tab else {
            return;
        };
        if self
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .is_some_and(|tab| tab.is_private)
        {
            return;
        }
        let Some((_, registry)) = self.extension_paths() else {
            return;
        };
        let Some(view) = self
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .map(|tab| &tab.view)
        else {
            return;
        };
        let proxy = self.proxy.as_ref().expect("event loop proxy").clone();
        if let Err(error) = extensions::set_extension_enabled(
            view,
            registry,
            id.to_owned(),
            enabled,
            move |result| {
                let _ = proxy.send_event(UserEvent::ExtensionChanged(result));
            },
        ) {
            let message = serde_json::to_string(&error)
                .unwrap_or_else(|_| "\"Не удалось изменить расширение\"".into());
            if let Some(overlay) = self.overlay.as_ref() {
                let _ = overlay.evaluate_script(&format!("window.showExtensionError({message})"));
            }
        }
    }

    #[cfg(windows)]
    fn remove_extension(&mut self, id: &str) {
        let Some(tab_id) = self.active_tab else {
            return;
        };
        if self
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .is_some_and(|tab| tab.is_private)
        {
            return;
        }
        let Some((_, registry)) = self.extension_paths() else {
            return;
        };
        let Some(view) = self
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .map(|tab| &tab.view)
        else {
            return;
        };
        let proxy = self.proxy.as_ref().expect("event loop proxy").clone();
        if let Err(error) =
            extensions::remove_extension(view, registry, id.to_owned(), move |result| {
                let _ = proxy.send_event(UserEvent::ExtensionChanged(result));
            })
        {
            let message = serde_json::to_string(&error)
                .unwrap_or_else(|_| "\"Не удалось удалить расширение\"".into());
            if let Some(overlay) = self.overlay.as_ref() {
                let _ = overlay.evaluate_script(&format!("window.showExtensionError({message})"));
            }
        }
    }

    #[cfg(windows)]
    fn install_extension_from_folder(&mut self) {
        let active_tab = self
            .active_tab
            .and_then(|id| self.tabs.iter().find(|tab| tab.id == id));
        if !active_tab.is_some_and(|tab| !tab.is_private) {
            if let Some(overlay) = self.overlay.as_ref() {
                let _ = overlay.evaluate_script(
                    "window.showExtensionError('Откройте обычную вкладку для управления расширениями')",
                );
            }
            return;
        }
        let Some((managed_root, _)) = self.extension_paths() else {
            return;
        };
        let Some(profile_dir) = self
            .context
            .as_ref()
            .and_then(|context| context.data_directory())
            .map(PathBuf::from)
        else {
            return;
        };
        let Some(source) = rfd::FileDialog::new()
            .set_title("Выберите распакованное расширение MV3")
            .pick_folder()
        else {
            return;
        };
        let proxy = self.proxy.as_ref().expect("event loop proxy").clone();
        std::thread::spawn(move || {
            let result = extensions::stage_package(&source, &managed_root)
                .and_then(|directory| {
                    extensions::read_manifest(&directory)
                        .map(|manifest| (directory, manifest, profile_dir))
                })
                .map_err(|error| error.to_string());
            let _ = proxy.send_event(UserEvent::ExtensionPrepared(result));
        });
    }

    #[cfg(windows)]
    fn install_prepared_extension(
        &mut self,
        directory: PathBuf,
        manifest: extensions::ExtensionManifest,
    ) {
        let Some(tab_id) = self.active_tab else {
            let _ = fs::remove_dir_all(directory);
            return;
        };
        if self
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .is_some_and(|tab| tab.is_private)
        {
            let _ = fs::remove_dir_all(directory);
            return;
        }
        let Some((_, registry)) = self.extension_paths() else {
            let _ = fs::remove_dir_all(directory);
            return;
        };
        let Some(view) = self
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .map(|tab| &tab.view)
        else {
            let _ = fs::remove_dir_all(directory);
            return;
        };
        let proxy = self.proxy.as_ref().expect("event loop proxy").clone();
        if let Err(error) = extensions::install_extension(
            view,
            directory.clone(),
            registry,
            manifest,
            move |result| {
                let _ = proxy.send_event(UserEvent::ExtensionInstalled(result));
            },
        ) {
            let _ = fs::remove_dir_all(directory);
            let message = serde_json::to_string(&error)
                .unwrap_or_else(|_| "\"Не удалось установить расширение\"".into());
            if let Some(overlay) = self.overlay.as_ref() {
                let _ = overlay.evaluate_script(&format!("window.showExtensionError({message})"));
            }
        }
    }

    fn find_in_active_tab(&mut self, backwards: bool) {
        let Ok(query) = serde_json::to_string(&self.find_query) else {
            return;
        };
        let Some(tab) = self.active_tab_mut() else {
            return;
        };
        let script = format!("window.find({query}, false, {backwards}, true);");
        let _ = tab.view.evaluate_script(&script);
    }

    fn active_tab_mut(&mut self) -> Option<&mut BrowserTab> {
        let id = self.active_tab?;
        self.tabs.iter_mut().find(|tab| tab.id == id)
    }

    fn create_tab(&mut self, url: &str) {
        self.create_tab_with_mode(url, false);
    }

    fn create_tab_with_mode(&mut self, url: &str, is_private: bool) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        if !is_private && self.context.is_none() {
            return;
        }
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let size = window.inner_size().to_logical::<u32>(window.scale_factor());
        let title_proxy = self.proxy.as_ref().expect("event loop proxy").clone();
        let load_proxy = self.proxy.as_ref().expect("event loop proxy").clone();
        let downloads_dir = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("Downloads");
        let _ = fs::create_dir_all(&downloads_dir);
        let download_dir_for_handler = downloads_dir.clone();
        let download_scope = self.download_scope.clone();
        let download_control = self.download_control.clone();
        let download_proxy = self.proxy.as_ref().expect("event loop proxy").clone();
        let mut builder = if is_private {
            WebViewBuilder::new()
        } else {
            WebViewBuilder::new_with_web_context(self.context.as_mut().expect("checked context"))
        };
        let permission_ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let navigation_ready = permission_ready.clone();
        let http_allowlist = Arc::new(Mutex::new(HashSet::new()));
        let allowed_http = http_allowlist.clone();
        let https_first_enabled = Arc::new(std::sync::atomic::AtomicBool::new(
            self.settings.https_first,
        ));
        let https_first_for_handler = https_first_enabled.clone();
        let navigation_proxy = self.proxy.as_ref().expect("event loop proxy").clone();
        builder = builder.with_navigation_handler(move |url| {
            if site_permissions::origin(&url).is_none() {
                return true;
            }
            if !navigation_ready.load(std::sync::atomic::Ordering::Acquire) {
                return false;
            }
            let Some(parsed) = url::Url::parse(&url).ok() else {
                return false;
            };
            if https_first_for_handler.load(std::sync::atomic::Ordering::Acquire)
                && parsed.scheme() == "http"
                && !https_first::is_local_http_url(&url)
            {
                let origin = parsed.origin().ascii_serialization();
                if allowed_http.lock().is_ok_and(|set| set.contains(&origin)) {
                    return true;
                }
                let _ = navigation_proxy.send_event(UserEvent::HttpNavigationBlocked(id, url));
                return false;
            }
            true
        });
        let page_background = if self.settings.theme == "light" {
            (234, 240, 249, 255)
        } else {
            (16, 19, 27, 255)
        };
        let target_url = if url == HOME_PAGE && self.settings.homepage == "duckduckgo" {
            "https://duckduckgo.com/"
        } else {
            url
        };
        let embedded_home = url == HOME_PAGE && self.settings.homepage == "aurora";
        builder = if embedded_home {
            builder.with_html(new_tab_html(&self.settings))
        } else {
            // Register the request filter before asking WebView2 to navigate.
            builder.with_url("about:blank")
        };
        let builder = builder
            .with_incognito(is_private)
            .with_focused(false)
            .with_visible(false)
            .with_background_color(page_background)
            .with_download_started_handler(move |url, path| {
                let name = url::Url::parse(&url)
                    .ok()
                    .and_then(|parsed| parsed.path_segments()?.next_back().map(str::to_owned))
                    .map(|name| downloads::safe_suggested_filename(&name))
                    .unwrap_or_else(|| "download".into());
                match rfd::FileDialog::new()
                    .set_directory(&download_dir_for_handler)
                    .set_file_name(&name)
                    .save_file()
                {
                    Some(destination) => {
                        *path = destination;
                        true
                    }
                    None => false,
                }
            })
            .with_bounds(Rect {
                position: LogicalPosition::new(0, CHROME_HEIGHT).into(),
                size: LogicalSize::new(size.width, size.height.saturating_sub(CHROME_HEIGHT))
                    .into(),
            })
            .with_document_title_changed_handler(move |title| {
                let _ = title_proxy.send_event(UserEvent::PageTitle(id, title));
            })
            .with_on_page_load_handler(move |event, url| match event {
                PageLoadEvent::Started => {
                    let _ = load_proxy.send_event(UserEvent::PageLoading(id, true));
                }
                PageLoadEvent::Finished => {
                    let _ = load_proxy.send_event(UserEvent::PageLoading(id, false));
                    let _ = load_proxy.send_event(UserEvent::PageLoaded(id, url));
                }
            });
        #[cfg(windows)]
        let builder = builder.with_browser_extensions_enabled(!is_private);
        let view = match builder.build_as_child(window) {
            Ok(view) => view,
            Err(error) => {
                eprintln!("Aurora could not create WebView2 tab {id}: {error}");
                self.show_chrome_toast("Не удалось создать вкладку. Попробуйте ещё раз.");
                return;
            }
        };
        let event_profile = download_scope.clone();
        if let Err(error) = download_control.attach(&view, move |update| {
            let _ = download_proxy.send_event(UserEvent::DownloadChanged(
                id,
                event_profile.clone(),
                is_private,
                update,
            ));
        }) {
            eprintln!("Aurora could not attach the WebView2 download manager: {error}");
        }
        let https_fallback_candidate = Arc::new(Mutex::new(None::<String>));
        let candidate_for_handler = https_fallback_candidate.clone();
        let https_setting_for_handler = https_first_enabled.clone();
        let failure_proxy = self.proxy.as_ref().expect("event loop proxy").clone();
        #[cfg(windows)]
        if let Err(error) =
            https_first::observe_failed_navigation(&view, move |source, succeeded, status| {
                let Ok(mut candidate) = candidate_for_handler.lock() else {
                    return;
                };
                let Some(expected) = candidate.as_ref() else {
                    return;
                };
                let same_origin = url::Url::parse(&source)
                    .ok()
                    .zip(url::Url::parse(expected).ok())
                    .is_some_and(|(actual, expected)| actual.origin() == expected.origin());
                if same_origin && succeeded {
                    *candidate = None;
                } else if same_origin && https_first::should_offer_fallback(status) {
                    if !https_setting_for_handler.load(std::sync::atomic::Ordering::Acquire) {
                        *candidate = None;
                        return;
                    }
                    if let Some(https_url) = candidate.take()
                        && let Some(http_url) = https_first::fallback_url(&https_url)
                    {
                        let _ = failure_proxy
                            .send_event(UserEvent::HttpsNavigationFailed(id, http_url));
                    }
                }
            })
        {
            eprintln!(
                "Aurora could not subscribe to navigation completion for HTTPS-first: {error}"
            );
        }
        let permissions = if is_private {
            Arc::new(RwLock::new(site_permissions::Policy::default()))
        } else {
            self.permissions.clone()
        };
        #[cfg(windows)]
        if let Err(error) = site_permissions::attach(&view, permissions.clone()) {
            eprintln!("Aurora could not attach site permission policy to tab {id}: {error}");
            self.show_chrome_toast("Не удалось включить защиту разрешений для вкладки.");
            return;
        }
        let mut privacy_attached = false;
        let tab_privacy = if is_private {
            Arc::new(privacy::PrivacyState::new(
                self.privacy_engine.clone().expect("privacy engine"),
                self.settings.adblock_enabled,
                Default::default(),
            ))
        } else {
            self.privacy_state.clone().expect("profile privacy")
        };
        {
            let state = &tab_privacy;
            #[cfg(windows)]
            match privacy::attach_to_view(&view, state.clone()) {
                Ok(()) => privacy_attached = true,
                Err(error) => eprintln!("Aurora privacy filter could not attach: {error}"),
            }
        }
        let permission_proxy = self.proxy.as_ref().expect("event loop proxy").clone();
        #[cfg(windows)]
        if let Err(error) = site_permissions::read_legacy(&view, move |result| {
            let _ = permission_proxy.send_event(UserEvent::LegacyPermissions(id, result));
        }) {
            eprintln!("Aurora permission initialization failed; navigation blocked: {error}");
        }
        self.tabs.push(BrowserTab {
            id,
            title: "Новая вкладка".into(),
            url: target_url.into(),
            is_loading: true,
            first_load_complete: false,
            awaiting_first_navigation: !embedded_home,
            privacy_attached,
            permissions,
            permission_gate: permission_ready,
            http_allowlist,
            https_fallback_candidate,
            https_first_enabled,
            privacy: tab_privacy,
            is_private,
            view,
        });
        self.select_tab(id);
    }

    fn select_tab(&mut self, id: u64) {
        if !self.tabs.iter().any(|tab| tab.id == id) {
            return;
        }
        self.active_tab = Some(id);
        for tab in &self.tabs {
            let _ = tab
                .view
                .set_visible(self.layout_ready && tab.id == id && tab.first_load_complete);
        }
        self.refresh_chrome();
        self.refresh_privacy_panel();
    }

    fn reorder_tabs(&mut self, encoded_ids: &str) {
        let Ok(ids) = serde_json::from_str::<Vec<u64>>(encoded_ids) else {
            return;
        };
        if ids.len() != self.tabs.len() {
            return;
        }
        let mut reordered = Vec::with_capacity(self.tabs.len());
        for id in ids {
            let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
                return;
            };
            if reordered.iter().any(|tab: &BrowserTab| tab.id == id) {
                return;
            }
            reordered.push(self.tabs.remove(index));
        }
        self.tabs = reordered;
        self.refresh_chrome();
    }

    fn close_tab(&mut self, event_loop: &ActiveEventLoop, id: u64) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        if self.tabs[index].is_private {
            self.cancel_private_downloads_for_tab(id);
            self.downloads
                .retain(|download| !(download.private && download.tab_id == id));
            self.refresh_downloads_overlay();
        }
        if self.tabs.len() == 1 {
            self.cancel_active_downloads();
            self.persist_downloads();
            event_loop.exit();
            return;
        }
        self.tabs.remove(index);
        if self.active_tab == Some(id) {
            let next_index = index.min(self.tabs.len() - 1);
            let next_id = self.tabs[next_index].id;
            self.select_tab(next_id);
        } else {
            self.refresh_chrome();
        }
    }

    fn save_bookmark(&mut self) {
        let Some(tab) = self
            .active_tab
            .and_then(|id| self.tabs.iter().find(|tab| tab.id == id))
        else {
            return;
        };
        if !(tab.url.starts_with("https://") || tab.url.starts_with("http://")) {
            if let Some(chrome) = self.chrome.as_ref() {
                let _ = chrome
                    .evaluate_script("window.showToast('Эту страницу нельзя добавить в закладки')");
            }
            return;
        }
        let bookmark = Bookmark {
            title: tab.title.clone(),
            url: tab.url.clone(),
        };
        let Some(bookmarks) = self.bookmarks.as_mut() else {
            return;
        };
        if !bookmarks.iter().any(|saved| saved.url == bookmark.url) {
            bookmarks.push(bookmark);
            if let Some(vault) = self.active_vault.as_ref() {
                persist_vault_records(
                    vault,
                    "bookmarks",
                    self.bookmarks.as_deref().unwrap_or(&[]),
                    |entry| &entry.url,
                );
            } else if let Some(path) = &self.bookmarks_path {
                let _ = write_json_atomically(path, bookmarks);
            }
        }
        self.refresh_chrome();
        if let Some(chrome) = self.chrome.as_ref() {
            let _ = chrome.evaluate_script("window.showToast('Закладка сохранена')");
        }
    }

    fn record_history(&mut self, id: u64, url: String) {
        if url.starts_with("about:") || url.starts_with("data:") {
            return;
        }
        if self.tabs.iter().any(|tab| tab.id == id && tab.is_private) {
            return;
        }
        let title = self
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .map(|tab| tab.title.clone())
            .unwrap_or_else(|| url.clone());
        let Some(history) = self.history.as_mut() else {
            return;
        };
        history.retain(|entry| entry.url != url);
        history.insert(0, HistoryEntry { title, url });
        history.truncate(1000);
        if let Some(vault) = self.active_vault.as_ref() {
            persist_vault_records(
                vault,
                "history",
                self.history.as_deref().unwrap_or(&[]),
                |entry| &entry.url,
            );
        } else if let Some(path) = &self.history_path {
            let _ = write_json_atomically(path, history);
        }
        self.refresh_chrome();
    }

    fn refresh_chrome(&self) {
        let Some(chrome) = self.chrome.as_ref() else {
            return;
        };
        let tabs: Vec<_> = self
            .tabs
            .iter()
            .map(|tab| serde_json::json!({"id": tab.id, "title": tab.title, "loading": tab.is_loading, "private": tab.is_private}))
            .collect();
        let tabs_json = serde_json::to_string(&tabs).unwrap_or_else(|_| "[]".into());
        let active_json = serde_json::to_string(&self.active_tab).unwrap_or_else(|_| "null".into());
        let address_json = serde_json::to_string(&self.active_tab.and_then(|id| {
            self.tabs
                .iter()
                .find(|tab| tab.id == id)
                .filter(|tab| tab.url != HOME_PAGE)
                .map(|tab| tab.url.as_str())
        }))
        .unwrap_or_else(|_| "null".into());
        let script = format!(
            "window.renderTabs({tabs_json}, {active_json}); window.setAddress({address_json}); document.documentElement.classList.toggle('no-motion', {}); document.documentElement.classList.toggle('light', {});",
            !self.settings.animations,
            self.settings.theme == "light"
        );
        let _ = chrome.evaluate_script(&script);
    }

    fn refresh_privacy_panel(&self) {
        let Some(overlay) = self.overlay.as_ref() else {
            return;
        };
        let Some(state) = self.privacy_state.as_ref() else {
            return;
        };
        let active = self
            .active_tab
            .and_then(|id| self.tabs.iter().find(|tab| tab.id == id));
        let state = active.map(|tab| &tab.privacy).unwrap_or(state);
        let attached = active.is_some_and(|tab| tab.privacy_attached);
        let host = active
            .and_then(|tab| url::Url::parse(&tab.url).ok())
            .and_then(|url| url.host_str().map(str::to_owned));
        let host_json = serde_json::to_string(&host).unwrap_or_else(|_| "null".into());
        let allowed = host.as_deref().is_some_and(|host| state.site_allowed(host));
        let script = format!(
            "window.setPrivacy({}, {}, {}, {}, {}, {});",
            state.is_enabled() && attached,
            state.blocked_count(),
            host_json,
            allowed,
            self.filter_list_count,
            attached
        );
        let _ = overlay.evaluate_script(&script);
        let origin = active.and_then(|tab| site_permissions::origin(&tab.url));
        let choices: Vec<_> = site_permissions::KINDS
            .iter()
            .map(|kind| {
                let decision = active
                    .and_then(|tab| {
                        tab.permissions
                            .read()
                            .ok()
                            .map(|p| p.decision(origin.as_deref().unwrap_or(""), kind))
                    })
                    .unwrap_or_default();
                serde_json::json!({"kind":kind,"decision":decision})
            })
            .collect();
        let connection = match origin.as_deref() {
            Some(o) if o.starts_with("https:") => {
                "Адрес HTTPS. TLS проверяет WebView2; сведения о сертификате и смешанном содержимом здесь не получены."
            }
            Some(_) => "Адрес HTTP: соединение не защищено TLS.",
            None => "Внутренняя страница. Разрешения сайта недоступны.",
        };
        let snapshot = serde_json::json!({"origin":origin.clone(),"tabId":active.map(|t|t.id),"permissionReady":active.is_some_and(|t|t.permission_gate.load(std::sync::atomic::Ordering::Acquire)),"connection":connection,"permissions":choices,"private":active.is_some_and(|t|t.is_private),"blockedSite":origin.as_deref().map(|o|state.blocked_for_origin(o)).unwrap_or(0)});
        let _ = overlay.evaluate_script(&format!("window.setSitePrivacy({snapshot})"));
    }

    fn show_site_data_error(&self, error: String) {
        if let (Some(overlay), Ok(message)) = (self.overlay.as_ref(), serde_json::to_string(&error))
        {
            let _ = overlay.evaluate_script(&format!("window.siteDataError({message})"));
        }
    }

    fn confirm_http_navigation(&mut self, id: u64, url: String, https_failed: bool) {
        if !self.tabs.iter().any(|tab| tab.id == id) {
            return;
        }
        let Some(origin) = url::Url::parse(&url)
            .ok()
            .map(|url| url.origin().ascii_serialization())
        else {
            return;
        };
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let description = if https_failed {
            format!(
                "Не удалось подключиться к сайту по HTTPS. Перейти по незашифрованному HTTP?\n\n{url}\n\nПароли и другие данные на этой странице могут передаваться без шифрования."
            )
        } else {
            format!(
                "Сайт запрашивает незашифрованное соединение HTTP. Продолжить?\n\n{url}\n\nПароли и другие данные могут передаваться без шифрования."
            )
        };
        let accepted = matches!(
            rfd::MessageDialog::new()
                .set_title("Небезопасное HTTP-соединение")
                .set_description(description)
                .set_level(rfd::MessageLevel::Warning)
                .set_buttons(rfd::MessageButtons::YesNo)
                .set_parent(window)
                .show(),
            rfd::MessageDialogResult::Yes | rfd::MessageDialogResult::Ok
        );
        if accepted && let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            if let Ok(mut allowed) = tab.http_allowlist.lock() {
                allowed.insert(origin);
            }
            tab.url = url.clone();
            let _ = tab.view.load_url(&url);
            self.refresh_chrome();
        } else if self.active_tab == Some(id)
            && let Some(chrome) = self.chrome.as_ref()
        {
            let _ = chrome.evaluate_script("window.showToast('Переход по HTTP отменён')");
        }
    }

    fn open_overlay(&mut self) {
        if self.overlay.is_none() {
            let (Some(window), Some(context)) = (self.window.as_ref(), self.ui_context.as_mut())
            else {
                return;
            };
            let proxy = self.proxy.as_ref().expect("event loop proxy").clone();
            let size = window.inner_size().to_logical::<u32>(window.scale_factor());
            let ready_proxy = proxy.clone();
            let overlay = WebViewBuilder::new_with_web_context(context)
                .with_html(OVERLAY_HTML)
                .with_navigation_handler(|url| is_trusted_ui_navigation(&url, OVERLAY_HTML))
                .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
                .with_transparent(true)
                .with_visible(false)
                .with_bounds(Rect {
                    position: LogicalPosition::new(0, 0).into(),
                    size: LogicalSize::new(size.width, size.height).into(),
                })
                .with_ipc_handler(move |request| {
                    match serde_json::from_str::<BrowserCommand>(request.body()) {
                        Ok(command) if command.action == "overlay_hidden" => {
                            let _ = proxy.send_event(UserEvent::OverlayHidden);
                        }
                        Ok(command) => {
                            let _ = proxy.send_event(UserEvent::Command(command));
                        }
                        Err(error) => eprintln!("Aurora overlay message rejected: {error}"),
                    }
                })
                .with_on_page_load_handler(move |event, _| {
                    if let PageLoadEvent::Finished = event {
                        let _ = ready_proxy.send_event(UserEvent::OverlayReady);
                    }
                })
                .build_as_child(window)
                .expect("create Aurora overlay");
            self.overlay = Some(overlay);
        }
        if let Some(overlay) = self.overlay.as_ref() {
            if let Some(window) = self.window.as_ref() {
                let size = window.inner_size().to_logical::<u32>(window.scale_factor());
                let _ = overlay.set_bounds(Rect {
                    position: LogicalPosition::new(0, 0).into(),
                    size: LogicalSize::new(size.width, size.height).into(),
                });
            }
            let _ = overlay.set_visible(true);
            #[cfg(windows)]
            raise_webview(overlay);
            self.update_overlay(true);
        }
    }

    fn update_overlay(&self, open: bool) {
        let Some(overlay) = self.overlay.as_ref() else {
            return;
        };
        let bookmarks = serde_json::to_string(self.bookmarks.as_deref().unwrap_or(&[]))
            .unwrap_or_else(|_| "[]".into());
        let history = serde_json::to_string(self.history.as_deref().unwrap_or(&[]))
            .unwrap_or_else(|_| "[]".into());
        let settings = serde_json::to_string(&self.settings).unwrap_or_else(|_| "{}".into());
        let profiles: Vec<_> = self
            .profiles_root
            .as_ref()
            .map(|root| vault::ProfileVault::discover(root))
            .unwrap_or_default()
            .into_iter()
            .map(|(id, name, _)| serde_json::json!({"id":id,"name":name}))
            .collect();
        let profiles = serde_json::to_string(&profiles).unwrap_or_else(|_| "[]".into());
        let active_profile = self
            .active_vault
            .as_ref()
            .map(vault::ProfileVault::display_name);
        let active_profile =
            serde_json::to_string(&active_profile).unwrap_or_else(|_| "null".into());
        let script = if open {
            format!(
                "window.setData({bookmarks}, {history}, {settings}, {profiles}, {active_profile}); window.setOpen(true);"
            )
        } else {
            format!(
                "window.setData({bookmarks}, {history}, {settings}, {profiles}, {active_profile});"
            )
        };
        let _ = overlay.evaluate_script(&script);
        self.refresh_privacy_panel();
    }

    fn activate_vault(&mut self, new_vault: vault::ProfileVault) -> Result<(), String> {
        // Read every saved record before tearing down the current profile. A
        // decryption or decode failure must leave the active profile untouched.
        let bookmarks = load_vault_records(&new_vault, "bookmarks")?;
        let history = load_vault_records(&new_vault, "history")?;
        // Load every encrypted security/privacy record before switching context.
        // A corrupt record must not silently reset a user's decisions.
        let permissions = load_vault_record::<site_permissions::Policy>(
            &new_vault,
            "site-permissions",
            "policy",
        )?
        .unwrap_or_default();
        let privacy_exceptions =
            load_vault_record::<HashSet<String>>(&new_vault, "privacy-sites", "exceptions")?
                .unwrap_or_default();
        let downloads =
            load_vault_record::<Vec<DownloadRecord>>(&new_vault, "downloads", "history")?
                .unwrap_or_default();
        let data_dir = new_vault.directory().join("WebView2");
        fs::create_dir_all(&data_dir)
            .map_err(|error| format!("Не удалось открыть данные WebView2 профиля: {error}"))?;
        self.cancel_active_downloads();
        self.persist_downloads();
        // Destroy views before their WebView2 context so profile data is released
        // and the old cookie store is no longer attached to any tab.
        self.overlay = None;
        self.tabs.clear();
        self.active_tab = None;
        self.context = None;
        self.context = Some(wry::WebContext::new(Some(data_dir)));
        let profile_id = new_vault.profile_id().to_owned();
        self.active_vault = Some(new_vault);
        self.download_scope = profile_id;
        self.downloads = downloads;
        self.download_speed_samples.clear();
        self.permissions = Arc::new(RwLock::new(permissions));
        let engine = self
            .privacy_engine
            .clone()
            .unwrap_or_else(|| Arc::new(RwLock::new(Arc::new(privacy::build_engine(&[])))));
        self.privacy_state = Some(Arc::new(privacy::PrivacyState::new(
            engine,
            self.settings.adblock_enabled,
            privacy_exceptions,
        )));
        self.privacy_path = None;
        self.bookmarks = Some(bookmarks);
        self.history = Some(history);
        self.bookmarks_path = None;
        self.history_path = None;
        self.create_tab(HOME_PAGE);
        Ok(())
    }

    fn show_chrome_toast(&self, message: &str) {
        if let (Some(chrome), Ok(message)) = (self.chrome.as_ref(), serde_json::to_string(message))
        {
            let _ = chrome.evaluate_script(&format!("window.showToast({message})"));
        }
    }

    fn activate_default_profile(&mut self) {
        self.cancel_active_downloads();
        self.persist_downloads();
        self.overlay = None;
        self.tabs.clear();
        self.active_tab = None;
        self.context = None;
        let Some(data_dir) = self.default_data_dir.clone() else {
            return;
        };
        self.context = Some(wry::WebContext::new(Some(data_dir.clone())));
        self.active_vault = None;
        self.configure_downloads();
        self.configure_privacy_state(data_dir.join("privacy-sites.json"));
        self.bookmarks_path = Some(data_dir.join("bookmarks.json"));
        self.history_path = Some(data_dir.join("history.json"));
        self.bookmarks = Some(read_json(&self.bookmarks_path));
        self.history = Some(read_json(&self.history_path));
        self.create_tab(HOME_PAGE);
    }

    fn configure_privacy_state(&mut self, path: PathBuf) {
        let policy = if let Some(vault) = self.active_vault.as_ref() {
            vault
                .list("site-permissions")
                .ok()
                .into_iter()
                .flatten()
                .find(|(id, _)| id == "policy")
                .and_then(|(_, bytes)| serde_json::from_slice(&bytes).ok())
                .unwrap_or_default()
        } else {
            fs::read(path.with_file_name("site-permissions.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_default()
        };
        self.permissions = Arc::new(RwLock::new(policy));
        let allowed: std::collections::HashSet<String> =
            if let Some(vault) = self.active_vault.as_ref() {
                vault
                    .list("privacy-sites")
                    .ok()
                    .into_iter()
                    .flatten()
                    .find(|(id, _)| id == "exceptions")
                    .and_then(|(_, bytes)| serde_json::from_slice(&bytes).ok())
                    .unwrap_or_default()
            } else {
                fs::read_to_string(&path)
                    .ok()
                    .and_then(|text| serde_json::from_str(&text).ok())
                    .unwrap_or_default()
            };
        let engine = self
            .privacy_engine
            .clone()
            .unwrap_or_else(|| Arc::new(RwLock::new(Arc::new(privacy::build_engine(&[])))));
        self.privacy_state = Some(Arc::new(privacy::PrivacyState::new(
            engine,
            self.settings.adblock_enabled,
            allowed,
        )));
        self.privacy_path = self.active_vault.is_none().then_some(path);
    }

    fn configure_downloads(&mut self) {
        if let Some(vault) = self.active_vault.as_ref() {
            self.download_scope = vault.profile_id().to_owned();
            self.downloads = vault
                .list("downloads")
                .ok()
                .into_iter()
                .flatten()
                .find(|(id, _)| id == "history")
                .and_then(|(_, bytes)| serde_json::from_slice::<Vec<DownloadRecord>>(&bytes).ok())
                .unwrap_or_default();
        } else {
            self.download_scope = "default".into();
            self.downloads = self
                .default_data_dir
                .as_ref()
                .map(|dir| read_json(&Some(dir.join("downloads.json"))))
                .unwrap_or_default();
        }
        self.download_speed_samples.clear();
    }

    fn persist_downloads(&self) {
        let records: Vec<_> = self
            .downloads
            .iter()
            .filter(|download| !download.private)
            .cloned()
            .collect();
        if let Some(vault) = self.active_vault.as_ref() {
            if let Ok(bytes) = serde_json::to_vec(&records)
                && let Err(error) = vault.put("downloads", "history", &bytes)
            {
                eprintln!("Aurora encrypted download history save failed: {error}");
            }
        } else if let Some(dir) = self.default_data_dir.as_ref()
            && let Err(error) = write_json_atomically(&dir.join("downloads.json"), &records)
        {
            eprintln!("Aurora download history save failed: {error}");
        }
    }

    fn cancel_active_downloads(&mut self) {
        let active: Vec<_> = self
            .downloads
            .iter()
            .filter(|item| item.state == "in_progress")
            .map(|item| item.id)
            .collect();
        for id in &active {
            let _ = self.download_control.cancel(*id);
        }
        for download in &mut self.downloads {
            if active.contains(&download.id) {
                download.state = "interrupted".into();
                download.error =
                    Some("Загрузка остановлена при закрытии или переключении профиля".into());
                download.speed = None;
            }
        }
        self.download_speed_samples.clear();
    }

    fn cancel_private_downloads_for_tab(&self, tab_id: u64) {
        for download in self
            .downloads
            .iter()
            .filter(|item| item.private && item.tab_id == tab_id && item.state == "in_progress")
        {
            let _ = self.download_control.cancel(download.id);
        }
    }

    fn download_is_visible(&self, item: &DownloadRecord) -> bool {
        let active_id = self.active_tab;
        let active_private = self
            .active_tab
            .and_then(|id| self.tabs.iter().find(|tab| tab.id == id))
            .is_some_and(|tab| tab.is_private);
        if active_private {
            item.private && Some(item.tab_id) == active_id
        } else {
            !item.private
        }
    }

    fn refresh_downloads_overlay(&self) {
        let Some(overlay) = self.overlay.as_ref() else {
            return;
        };
        let entries: Vec<_> = self
            .downloads
            .iter()
            .filter(|item| self.download_is_visible(item))
            .map(|item| {
                let file_name = std::path::Path::new(&item.path)
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .unwrap_or("Файл");
                serde_json::json!({
                    "id": item.id,
                    "name": file_name,
                    "url": item.url,
                    "path": item.path,
                    "received": item.received,
                    "total": item.total,
                    "state": item.state,
                    "error": item.error,
                    "speed": item.speed,
                    "private": item.private,
                })
            })
            .collect();
        let json = serde_json::to_string(&entries).unwrap_or_else(|_| "[]".into());
        let _ = overlay.evaluate_script(&format!("window.setDownloads({json})"));
    }

    fn apply_download_update(
        &mut self,
        tab_id: u64,
        profile_scope: String,
        private: bool,
        update: downloads::DownloadUpdate,
    ) {
        if private
            && !self
                .tabs
                .iter()
                .any(|tab| tab.id == tab_id && tab.is_private)
        {
            return;
        }
        if profile_scope != self.download_scope {
            return;
        }
        let now = std::time::Instant::now();
        let is_new = !self
            .downloads
            .iter()
            .any(|download| download.id == update.id);
        let speed = self
            .download_speed_samples
            .get(&update.id)
            .and_then(|(previous, at)| {
                let elapsed = now.duration_since(*at).as_secs_f64();
                (elapsed > 0.0 && update.received >= *previous)
                    .then_some(((update.received - *previous) as f64 / elapsed) as u64)
            });
        self.download_speed_samples
            .insert(update.id, (update.received, now));
        if let Some(record) = self
            .downloads
            .iter_mut()
            .find(|download| download.id == update.id)
        {
            record.path = update.path;
            record.received = update.received;
            record.total = update.total;
            record.state = update.state.clone();
            record.error = update.error;
            if speed.is_some() {
                record.speed = speed;
            }
        } else {
            self.downloads.push(DownloadRecord {
                id: update.id,
                tab_id,
                profile_scope,
                private,
                url: update.url,
                path: update.path,
                received: update.received,
                total: update.total,
                state: update.state.clone(),
                error: update.error,
                started_ms: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |duration| duration.as_millis()),
                speed,
            });
        }
        if update.state != "in_progress" {
            self.download_speed_samples.remove(&update.id);
        }
        if is_new || update.state != "in_progress" {
            self.persist_downloads();
        }
        self.refresh_downloads_overlay();
    }

    fn open_downloaded_file(&self, id: u64) {
        let Some(download) = self.downloads.iter().find(|item| {
            item.id == id && item.state == "completed" && self.download_is_visible(item)
        }) else {
            return;
        };
        let path = std::path::Path::new(&download.path);
        if path.is_file() {
            let _ = std::process::Command::new("explorer.exe")
                .arg(format!("/select,{}", path.display()))
                .spawn();
        }
    }
}

#[derive(Default)]
struct AuroraApp {
    window: Option<Window>,
    window_shown: bool,
    layout_ready: bool,
    chrome: Option<WebView>,
    tabs: Vec<BrowserTab>,
    active_tab: Option<u64>,
    next_tab_id: u64,
    context: Option<wry::WebContext>,
    ui_context: Option<wry::WebContext>,
    proxy: Option<EventLoopProxy<UserEvent>>,
    bookmarks: Option<Vec<Bookmark>>,
    bookmarks_path: Option<PathBuf>,
    history: Option<Vec<HistoryEntry>>,
    history_path: Option<PathBuf>,
    overlay: Option<WebView>,
    settings: BrowserSettings,
    settings_path: Option<PathBuf>,
    find_query: String,
    profiles_root: Option<PathBuf>,
    default_data_dir: Option<PathBuf>,
    active_vault: Option<vault::ProfileVault>,
    privacy_engine: Option<Arc<RwLock<Arc<adblock::Engine>>>>,
    privacy_state: Option<Arc<privacy::PrivacyState>>,
    privacy_path: Option<PathBuf>,
    filter_list_count: usize,
    permissions: site_permissions::SharedPolicy,
    download_control: downloads::DownloadControl,
    downloads: Vec<DownloadRecord>,
    download_scope: String,
    download_speed_samples: std::collections::HashMap<u64, (u64, std::time::Instant)>,
}

impl ApplicationHandler<UserEvent> for AuroraApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attributes = Window::default_attributes()
            .with_title("Aurora")
            .with_visible(false)
            .with_window_icon(Some(browser_icon()))
            .with_decorations(false)
            .with_inner_size(LogicalSize::new(1440.0, 900.0))
            .with_maximized(true)
            .with_min_inner_size(LogicalSize::new(720.0, 480.0));
        let window = event_loop
            .create_window(attributes)
            .expect("create browser window");
        let size = window.inner_size().to_logical::<u32>(window.scale_factor());
        let proxy = self
            .proxy
            .as_ref()
            .expect("event loop proxy is initialized")
            .clone();

        let ui_dir = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("Aurora")
            .join("Rust")
            .join("PrivilegedUI");
        fs::create_dir_all(&ui_dir).expect("create privileged UI profile directory");
        let mut ui_context = wry::WebContext::new(Some(ui_dir));
        let chrome = WebViewBuilder::new_with_web_context(&mut ui_context)
            .with_browser_extensions_enabled(false)
            .with_html(CHROME_HTML)
            .with_navigation_handler(|url| is_trusted_ui_navigation(&url, CHROME_HTML))
            .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
            .with_focused(true)
            .with_bounds(Rect {
                position: LogicalPosition::new(0, 0).into(),
                size: LogicalSize::new(size.width, size.height).into(),
            })
            .with_ipc_handler(move |request| {
                match serde_json::from_str::<BrowserCommand>(request.body()) {
                    Ok(command) if command.action == "menu_open" => {
                        let _ = proxy.send_event(UserEvent::MenuOpen(
                            command.value.as_deref() == Some("true"),
                        ));
                    }
                    Ok(command) => {
                        let _ = proxy.send_event(UserEvent::Command(command));
                    }
                    Err(error) => eprintln!("Aurora toolbar message rejected: {error}"),
                }
            })
            .build_as_child(&window)
            .expect("create Aurora toolbar");

        let data_dir = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("Aurora")
            .join("Rust")
            .join("Default");
        fs::create_dir_all(&data_dir).expect("create browser profile directory");
        let bookmarks_path = data_dir.join("bookmarks.json");
        let bookmarks = fs::read_to_string(&bookmarks_path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        let history_path = data_dir.join("history.json");
        let history = fs::read_to_string(&history_path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        let settings_path = data_dir.join("settings.json");
        let settings = fs::read_to_string(&settings_path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        let context = wry::WebContext::new(Some(data_dir.clone()));
        self.bookmarks = Some(bookmarks);
        self.bookmarks_path = Some(bookmarks_path);
        self.history = Some(history);
        self.history_path = Some(history_path);
        self.settings = settings;
        self.settings_path = Some(settings_path);
        self.filter_list_count = 0;
        self.privacy_engine = Some(Arc::new(RwLock::new(Arc::new(privacy::build_engine(&[])))));
        self.configure_privacy_state(data_dir.join("privacy-sites.json"));
        self.profiles_root = Some(data_dir.parent().unwrap_or(&data_dir).join("Profiles"));
        self.default_data_dir = Some(data_dir.clone());
        self.configure_downloads();
        self.chrome = Some(chrome);
        self.window = Some(window);
        self.ui_context = Some(ui_context);
        self.context = Some(context);
        self.create_tab(HOME_PAGE);
        let update_proxy = self.proxy.as_ref().expect("event loop proxy").clone();
        let filter_cache = data_dir.join("filter-lists.json");
        std::thread::spawn(move || {
            let send_engine = |lists: Vec<String>| {
                let engine = Arc::new(privacy::build_engine(&lists));
                let _ = update_proxy.send_event(UserEvent::FilterListsUpdated(Ok((lists, engine))));
            };
            let cached = privacy::load_cached_filter_lists(&filter_cache);
            if !cached.is_empty() {
                send_engine(cached);
            }
            match privacy::download_filter_lists() {
                Ok(lists) => {
                    if let Err(error) = write_json_atomically(&filter_cache, &lists) {
                        eprintln!("Aurora filter list cache write failed: {error}");
                    }
                    send_engine(lists);
                }
                Err(error) => {
                    let _ = update_proxy.send_event(UserEvent::FilterListsUpdated(Err(error)));
                }
            }
        });
        let timeout_proxy = self.proxy.as_ref().expect("event loop proxy").clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(12));
            let _ = timeout_proxy.send_event(UserEvent::StartupTimeout);
        });
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::LegacyPermissions(id, result) => {
                let Some(tab) = self.tabs.iter().find(|t| t.id == id) else {
                    return;
                };
                let migrated = (|| -> Result<Vec<site_permissions::LegacyPermission>, String> {
                    let settings = result?;
                    if !settings.is_empty() && !tab.is_private {
                        let mut policy = tab
                            .permissions
                            .read()
                            .map_err(|_| "Permission policy locked")?
                            .clone();
                        policy.import_legacy(&settings);
                        if let Some(vault) = self.active_vault.as_ref() {
                            vault
                                .put(
                                    "site-permissions",
                                    &format!("legacy-{}", uuid::Uuid::new_v4()),
                                    &serde_json::to_vec(&settings).map_err(|e| e.to_string())?,
                                )
                                .map_err(|e| e.to_string())?;
                            vault
                                .put(
                                    "site-permissions",
                                    "policy",
                                    &serde_json::to_vec(&policy).map_err(|e| e.to_string())?,
                                )
                                .map_err(|e| e.to_string())?;
                        } else if let Some(root) = self.default_data_dir.as_ref() {
                            write_json_atomically(
                                &root.join(format!(
                                    "legacy-permissions-{}.json",
                                    uuid::Uuid::new_v4()
                                )),
                                &settings,
                            )
                            .map_err(|e| e.to_string())?;
                            write_json_atomically(&root.join("site-permissions.json"), &policy)
                                .map_err(|e| e.to_string())?;
                        }
                        *tab.permissions
                            .write()
                            .map_err(|_| "Permission policy locked")? = policy;
                    }
                    Ok(settings)
                })();
                match migrated {
                    Ok(settings) => {
                        let proxy = self.proxy.as_ref().unwrap().clone();
                        if let Err(error) =
                            site_permissions::reset_legacy(&tab.view, settings, move |result| {
                                let _ = proxy.send_event(UserEvent::PermissionsReady(id, result));
                            })
                        {
                            eprintln!(
                                "Aurora native permission reset failed; navigation blocked: {error}"
                            );
                        }
                    }
                    Err(error) => {
                        eprintln!("Aurora permission migration failed; navigation blocked: {error}")
                    }
                }
            }
            UserEvent::PermissionsReady(id, result) => match result {
                Ok(()) => {
                    if let Some(tab) = self.tabs.iter().find(|t| t.id == id) {
                        tab.permission_gate
                            .store(true, std::sync::atomic::Ordering::Release);
                        if tab.url != HOME_PAGE {
                            let _ = tab.view.load_url(&tab.url);
                        }
                    }
                }
                Err(error) => eprintln!(
                    "Aurora permission initialization failed; navigation blocked: {error}"
                ),
            },
            UserEvent::SiteCookies(id, origin, result) => {
                if self.active_tab != Some(id)
                    || self
                        .tabs
                        .iter()
                        .find(|tab| tab.id == id)
                        .and_then(|tab| site_permissions::origin(&tab.url))
                        .as_deref()
                        != Some(&origin)
                {
                    return;
                }
                let Some(overlay) = self.overlay.as_ref() else {
                    return;
                };
                match result {
                    Ok(cookies) => {
                        let json = serde_json::to_string(&cookies).unwrap_or_else(|_| "[]".into());
                        let _ = overlay.evaluate_script(&format!("window.setSiteCookies({json})"));
                    }
                    Err(error) => {
                        let message = serde_json::to_string(&error)
                            .unwrap_or_else(|_| "\"Не удалось прочитать cookies\"".into());
                        let _ =
                            overlay.evaluate_script(&format!("window.siteDataError({message})"));
                    }
                }
            }
            UserEvent::SiteDataDone(id, origin, operation, result) => {
                if self.active_tab != Some(id)
                    || self
                        .tabs
                        .iter()
                        .find(|tab| tab.id == id)
                        .and_then(|tab| site_permissions::origin(&tab.url))
                        .as_deref()
                        != Some(&origin)
                {
                    return;
                }
                if let Some(overlay) = self.overlay.as_ref() {
                    match result {
                        Ok(()) => {
                            let op = serde_json::to_string(&operation).unwrap_or_default();
                            let _ = overlay.evaluate_script(&format!("window.siteDataDone({op})"));
                        }
                        Err(error) => {
                            let message = serde_json::to_string(&error)
                                .unwrap_or_else(|_| "\"Не удалось очистить данные\"".into());
                            let _ = overlay
                                .evaluate_script(&format!("window.siteDataError({message})"));
                        }
                    }
                }
            }
            UserEvent::ProfileCacheDone(id, result) => {
                if self.active_tab != Some(id) {
                    return;
                }
                if let Some(overlay) = self.overlay.as_ref() {
                    match result {
                        Ok(()) => {
                            let _ = overlay.evaluate_script("window.siteDataDone('cache')");
                        }
                        Err(error) => {
                            let message = serde_json::to_string(&error)
                                .unwrap_or_else(|_| "\"Не удалось очистить кэш\"".into());
                            let _ = overlay
                                .evaluate_script(&format!("window.siteDataError({message})"));
                        }
                    }
                }
            }
            UserEvent::HttpNavigationBlocked(id, url) => {
                if self.active_tab == Some(id) {
                    self.confirm_http_navigation(id, url, false);
                }
            }
            UserEvent::HttpsNavigationFailed(id, url) => {
                if self.active_tab == Some(id) {
                    self.confirm_http_navigation(id, url, true);
                }
            }
            UserEvent::DownloadChanged(tab_id, profile_scope, private, update) => {
                self.apply_download_update(tab_id, profile_scope, private, update);
            }
            UserEvent::StartupTimeout => {
                if !self.window_shown
                    && let Some(window) = self.window.as_ref()
                {
                    window.set_visible(true);
                    self.window_shown = true;
                }
            }
            UserEvent::ShowWindow => {
                if !self.window_shown
                    && self
                        .active_tab
                        .and_then(|active_id| self.tabs.iter().find(|tab| tab.id == active_id))
                        .is_some_and(|tab| tab.first_load_complete)
                    && let Some(window) = self.window.as_ref()
                {
                    window.set_visible(true);
                    self.window_shown = true;
                }
            }
            UserEvent::FilterListsUpdated(result) => match result {
                Ok((lists, replacement)) => {
                    let mut installed = false;
                    if let Some(engine) = self.privacy_engine.as_ref()
                        && let Ok(mut current) = engine.write()
                    {
                        *current = replacement;
                        installed = true;
                    }
                    if installed {
                        self.filter_list_count = lists.len();
                        self.refresh_privacy_panel();
                    }
                }
                Err(error) => eprintln!("Aurora filter list update failed: {error}"),
            },
            UserEvent::MenuOpen(open) => {
                if open {
                    self.open_overlay();
                } else if let Some(overlay) = self.overlay.as_ref() {
                    let _ = overlay.evaluate_script("window.setOpen(false)");
                }
            }
            UserEvent::OverlayHidden => {
                if let Some(overlay) = self.overlay.as_ref() {
                    let _ = overlay.set_visible(false);
                }
                #[cfg(windows)]
                if let Some(tab) = self.active_tab_mut() {
                    raise_webview(&tab.view);
                }
                if let Some(chrome) = self.chrome.as_ref() {
                    let _ = chrome.evaluate_script("window.syncMenu(false)");
                }
            }
            UserEvent::OverlayReady => self.update_overlay(true),
            UserEvent::ExtensionsListed(result) => {
                if let Some(overlay) = self.overlay.as_ref() {
                    match result {
                        Ok(extensions) => {
                            let json =
                                serde_json::to_string(&extensions).unwrap_or_else(|_| "[]".into());
                            let _ =
                                overlay.evaluate_script(&format!("window.setExtensions({json})"));
                        }
                        Err(error) => {
                            let message = serde_json::to_string(&error).unwrap_or_else(|_| {
                                "\"Не удалось получить список расширений\"".into()
                            });
                            let _ = overlay
                                .evaluate_script(&format!("window.showExtensionError({message})"));
                        }
                    }
                }
            }
            UserEvent::ExtensionChanged(result) => {
                if let Some(overlay) = self.overlay.as_ref() {
                    match result {
                        Ok(()) => {
                            let _ = overlay.evaluate_script("window.showExtensionToast('Изменения сохранены'); window.requestExtensions()");
                        }
                        Err(error) => {
                            let message = serde_json::to_string(&error)
                                .unwrap_or_else(|_| "\"Не удалось изменить расширение\"".into());
                            let _ = overlay
                                .evaluate_script(&format!("window.showExtensionError({message})"));
                        }
                    }
                }
            }
            UserEvent::ExtensionInstalled(result) => {
                if let Some(overlay) = self.overlay.as_ref() {
                    match result {
                        Ok(_) => {
                            let _ = overlay.evaluate_script("window.showExtensionToast('Расширение установлено'); window.requestExtensions()");
                        }
                        Err(error) => {
                            let message = serde_json::to_string(&error)
                                .unwrap_or_else(|_| "\"Не удалось установить расширение\"".into());
                            let _ = overlay
                                .evaluate_script(&format!("window.showExtensionError({message})"));
                        }
                    }
                }
            }
            UserEvent::ExtensionPrepared(result) => match result {
                Ok((directory, manifest, prepared_profile)) => {
                    let active_profile = self
                        .context
                        .as_ref()
                        .and_then(|context| context.data_directory());
                    if active_profile == Some(prepared_profile.as_path())
                        && !self
                            .active_tab
                            .and_then(|id| self.tabs.iter().find(|tab| tab.id == id))
                            .is_some_and(|tab| tab.is_private)
                        && self.confirm_extension_permissions(&directory, &manifest)
                    {
                        self.install_prepared_extension(directory, manifest);
                    } else {
                        let _ = fs::remove_dir_all(directory);
                    }
                }
                Err(error) => {
                    if let Some(overlay) = self.overlay.as_ref() {
                        let message = serde_json::to_string(&error).unwrap_or_else(|_| {
                            "\"Не удалось подготовить пакет расширения\"".into()
                        });
                        let _ = overlay
                            .evaluate_script(&format!("window.showExtensionError({message})"));
                    }
                }
            },
            UserEvent::PageLoading(id, loading) => {
                if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
                    tab.is_loading = loading;
                    self.refresh_chrome();
                }
            }
            UserEvent::Command(command) => match command.action.as_str() {
                "navigate" => {
                    if let Some(input) = command
                        .value
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                    {
                        let url = resolve_input(
                            input,
                            &self.settings.search_engine,
                            self.settings.https_first,
                        );
                        let id = self.active_tab;
                        if self.settings.https_first
                            && url.starts_with("http://")
                            && !https_first::is_local_http_url(&url)
                        {
                            if let Some(id) = id {
                                self.confirm_http_navigation(id, url, false);
                            }
                        } else {
                            let allow_fallback = self.settings.https_first;
                            if let Some(tab) = self.active_tab_mut() {
                                if let Ok(mut candidate) = tab.https_fallback_candidate.lock() {
                                    *candidate = allow_fallback
                                        .then(|| https_first::fallback_url(&url))
                                        .flatten();
                                }
                                tab.url = url.clone();
                                let _ = tab.view.load_url(&url);
                            }
                            self.refresh_chrome();
                        }
                    }
                }
                "open_url" => {
                    if let Some(url) = command.value.as_deref() {
                        let id = self.active_tab;
                        if self.settings.https_first
                            && url.starts_with("http://")
                            && !https_first::is_local_http_url(url)
                        {
                            if let Some(id) = id {
                                self.confirm_http_navigation(id, url.into(), false);
                            }
                        } else if let Some(tab) = self.active_tab_mut() {
                            tab.url = url.into();
                            let _ = tab.view.load_url(url);
                        }
                        self.refresh_chrome();
                    }
                }
                "back" => {
                    if let Some(tab) = self.active_tab_mut() {
                        let _ = tab.view.evaluate_script("history.back()");
                    }
                }
                "forward" => {
                    if let Some(tab) = self.active_tab_mut() {
                        let _ = tab.view.evaluate_script("history.forward()");
                    }
                }
                "reload" => {
                    if let Some(tab) = self.active_tab_mut() {
                        let _ = tab.view.evaluate_script("location.reload()");
                    }
                }
                "find_text" => {
                    self.find_query = command.value.unwrap_or_default();
                    self.find_in_active_tab(false);
                }
                "find_next" => self.find_in_active_tab(false),
                "find_previous" => self.find_in_active_tab(true),
                "find_close" => {
                    if let Some(tab) = self.active_tab_mut() {
                        let _ = tab
                            .view
                            .evaluate_script("window.getSelection()?.removeAllRanges()");
                    }
                }
                "new_tab" => self.create_tab(HOME_PAGE),
                "new_private_tab" => self.create_tab_with_mode(HOME_PAGE, true),
                "create_profile" => {
                    let request = command
                        .value
                        .as_deref()
                        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok());
                    if let (Some(root), Some(request)) = (self.profiles_root.as_ref(), request) {
                        let name = request.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let password = request
                            .get("password")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        match vault::ProfileVault::create(root, name, password) {
                            Ok(created) => {
                                let recovery = serde_json::to_string(&created.recovery_key)
                                    .unwrap_or_else(|_| "\"\"".into());
                                if let Some(chrome) = self.chrome.as_ref() {
                                    let _ = chrome.evaluate_script(&format!("window.alert('Сохраните этот ключ восстановления отдельно. Он будет показан только сейчас:\\n\\n'+{recovery})"));
                                }
                                if let Err(error) = self.activate_vault(created.vault) {
                                    eprintln!("Aurora could not activate the new profile: {error}");
                                    self.show_chrome_toast(&error);
                                }
                            }
                            Err(error) => eprintln!("Aurora profile creation failed: {error}"),
                        }
                    }
                }
                "unlock_profile" | "recover_profile" => {
                    let request = command
                        .value
                        .as_deref()
                        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok());
                    if let (Some(root), Some(request)) = (self.profiles_root.as_ref(), request) {
                        let id = request.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let credential = request
                            .get("credential")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let directory = vault::ProfileVault::discover(root)
                            .into_iter()
                            .find(|(profile_id, _, _)| profile_id == id)
                            .map(|(_, _, path)| path);
                        if let Some(directory) = directory {
                            let result = if command.action == "recover_profile" {
                                vault::ProfileVault::unlock_recovery(&directory, credential)
                            } else {
                                vault::ProfileVault::unlock_password(&directory, credential)
                            };
                            match result {
                                Ok(profile) => {
                                    if let Err(error) = self.activate_vault(profile) {
                                        eprintln!("Aurora could not activate the profile: {error}");
                                        self.show_chrome_toast(&error);
                                    }
                                }
                                Err(error) => {
                                    if let Some(chrome) = self.chrome.as_ref() {
                                        let message = serde_json::to_string(&error.to_string())
                                            .unwrap_or_else(|_| {
                                                "\"Не удалось открыть профиль\"".into()
                                            });
                                        let _ = chrome.evaluate_script(&format!(
                                            "window.showToast({message})"
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
                "default_profile" | "lock_profile" => self.activate_default_profile(),
                "change_profile_password" => {
                    if let (Some(profile), Some(password)) =
                        (self.active_vault.as_mut(), command.value.as_deref())
                    {
                        let result = profile.change_password(password);
                        if let Some(chrome) = self.chrome.as_ref() {
                            let message = if result.is_ok() {
                                "Пароль профиля изменён"
                            } else {
                                "Не удалось изменить пароль профиля"
                            };
                            let _ =
                                chrome.evaluate_script(&format!("window.showToast('{}')", message));
                        }
                    }
                }
                "export_profile_backup" => {
                    if let (Some(profile), Some(password)) =
                        (self.active_vault.as_ref(), command.value.as_deref())
                    {
                        let destination = rfd::FileDialog::new()
                            .add_filter("Резервная копия Aurora", &["aurorabk"])
                            .set_file_name("aurora-profile.aurorabk")
                            .save_file();
                        if let Some(destination) = destination {
                            let result = profile.export_backup(&destination, password);
                            if let Some(chrome) = self.chrome.as_ref() {
                                let message = if result.is_ok() {
                                    "Резервная копия сохранена"
                                } else {
                                    "Не удалось создать резервную копию"
                                };
                                let _ = chrome
                                    .evaluate_script(&format!("window.showToast('{}')", message));
                            }
                        }
                    }
                }
                "restore_profile_backup" => {
                    let request = command
                        .value
                        .as_deref()
                        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok());
                    if let (Some(root), Some(request)) = (self.profiles_root.clone(), request) {
                        let backup_password = request
                            .get("backup_password")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let profile_password = request
                            .get("profile_password")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if let Some(source) = rfd::FileDialog::new()
                            .add_filter("Резервная копия Aurora", &["aurorabk"])
                            .pick_file()
                        {
                            match vault::ProfileVault::restore_backup(
                                &source,
                                &root,
                                backup_password,
                                profile_password,
                            ) {
                                Ok(profile) => {
                                    if let Err(error) = self.activate_vault(profile) {
                                        eprintln!(
                                            "Aurora could not activate the restored profile: {error}"
                                        );
                                        self.show_chrome_toast(&error);
                                    }
                                }
                                Err(error) => {
                                    if let Some(chrome) = self.chrome.as_ref() {
                                        let message = serde_json::to_string(&error.to_string())
                                            .unwrap_or_else(|_| {
                                                "\"Не удалось восстановить профиль\"".into()
                                            });
                                        let _ = chrome.evaluate_script(&format!(
                                            "window.showToast({message})"
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
                "delete_profile" => {
                    if self.active_vault.is_none() {
                        let id = command.value.as_deref().unwrap_or("");
                        if let Some(root) = self.profiles_root.as_ref() {
                            let directory = vault::ProfileVault::discover(root)
                                .into_iter()
                                .find(|(profile_id, _, _)| profile_id == id)
                                .map(|(_, _, path)| path);
                            if let Some(directory) = directory {
                                let _ = vault::ProfileVault::delete_locked(&directory);
                            }
                        }
                        self.update_overlay(false);
                    }
                }
                "extensions_open" | "extensions_list" => {
                    #[cfg(windows)]
                    self.request_extensions();
                    #[cfg(not(windows))]
                    self.show_extension_error(
                        "Управление расширениями пока доступно только в Windows/WebView2",
                    );
                }
                "extensions_install" => {
                    #[cfg(windows)]
                    self.install_extension_from_folder();
                    #[cfg(not(windows))]
                    self.show_extension_error(
                        "Установка расширений пока доступна только в Windows/WebView2",
                    );
                }
                "extensions_enable" | "extensions_disable" => {
                    #[cfg(windows)]
                    self.set_extension_enabled(
                        command.value.as_deref().unwrap_or(""),
                        command.action == "extensions_enable",
                    );
                }
                "extensions_remove" => {
                    #[cfg(windows)]
                    self.remove_extension(command.value.as_deref().unwrap_or(""));
                }
                "bookmark" => self.save_bookmark(),
                "privacy_toggle_enabled" => {
                    if let Some(state) = self.privacy_state.as_ref() {
                        let enabled = !state.is_enabled();
                        state.set_enabled(enabled);
                        for tab in &self.tabs {
                            tab.privacy.set_enabled(enabled);
                        }
                        self.settings.adblock_enabled = enabled;
                        if let Some(path) = &self.settings_path {
                            let _ = write_json_atomically(path, &self.settings);
                        }
                        self.refresh_privacy_panel();
                    }
                }
                "site_permission_set" | "site_permission_reset" => {
                    let Some(tab) = self
                        .active_tab
                        .and_then(|id| self.tabs.iter().find(|t| t.id == id))
                    else {
                        return;
                    };
                    let Some(origin) = site_permissions::origin(&tab.url) else {
                        return;
                    };
                    if !tab
                        .permission_gate
                        .load(std::sync::atomic::Ordering::Acquire)
                    {
                        self.refresh_privacy_panel();
                        return;
                    }
                    let policy = tab.permissions.clone();
                    let result = (|| -> Result<(), String> {
                        let mut next = policy.read().map_err(|_| "Политика недоступна")?.clone();
                        let value: serde_json::Value =
                            serde_json::from_str(command.value.as_deref().unwrap_or(""))
                                .map_err(|_| "Некорректная команда")?;
                        if value["tabId"].as_u64() != Some(tab.id)
                            || value["origin"].as_str() != Some(origin.as_str())
                        {
                            return Err("Вкладка изменилась; обновите панель".into());
                        }
                        if command.action == "site_permission_reset" {
                            next.reset(&origin);
                        } else {
                            // Reject a delayed UI command after switching to another origin.
                            if value["origin"].as_str() != Some(origin.as_str()) {
                                return Err("Сайт изменился; обновите панель".into());
                            }
                            let decision = serde_json::from_value(value["decision"].clone())
                                .map_err(|_| "Некорректное решение")?;
                            next.set(&origin, value["kind"].as_str().unwrap_or(""), decision)?;
                        }
                        if !tab.is_private {
                            if let Some(vault) = self.active_vault.as_ref() {
                                let bytes = serde_json::to_vec(&next).map_err(|e| e.to_string())?;
                                vault
                                    .put("site-permissions", "policy", &bytes)
                                    .map_err(|e| e.to_string())?;
                            } else if let Some(path) = self.default_data_dir.as_ref() {
                                write_json_atomically(&path.join("site-permissions.json"), &next)
                                    .map_err(|e| e.to_string())?;
                            }
                        }
                        *policy.write().map_err(|_| "Политика недоступна")? = next;
                        Ok(())
                    })();
                    if let Err(error) = result {
                        if let Some(overlay) = self.overlay.as_ref() {
                            let message = serde_json::to_string(&error).unwrap();
                            let _ = overlay
                                .evaluate_script(&format!("window.sitePrivacyError({message})"));
                        }
                    } else {
                        // Reload closes existing media streams; the next request uses the new policy.
                        for tab in &self.tabs {
                            if Arc::ptr_eq(&tab.permissions, &policy)
                                && site_permissions::origin(&tab.url).as_deref() == Some(&origin)
                            {
                                let _ = tab.view.evaluate_script("location.reload()");
                            }
                        }
                    }
                    self.refresh_privacy_panel();
                }
                "site_data_refresh"
                | "site_data_clear_origin"
                | "site_cookie_delete"
                | "site_data_clear_cache" => {
                    #[cfg(windows)]
                    {
                        let Some(tab_id) = self.active_tab else {
                            return;
                        };
                        let Some(tab) = self.tabs.iter().find(|tab| tab.id == tab_id) else {
                            return;
                        };
                        let Some(origin) = site_permissions::origin(&tab.url) else {
                            return;
                        };
                        if !tab
                            .permission_gate
                            .load(std::sync::atomic::Ordering::Acquire)
                        {
                            return;
                        }
                        let value: serde_json::Value =
                            serde_json::from_str(command.value.as_deref().unwrap_or("{}"))
                                .unwrap_or_default();
                        if value.get("tabId").and_then(serde_json::Value::as_u64) != Some(tab_id)
                            || value.get("origin").and_then(serde_json::Value::as_str)
                                != Some(origin.as_str())
                        {
                            return;
                        }
                        let Some(proxy) = self.proxy.as_ref().cloned() else {
                            return;
                        };
                        match command.action.as_str() {
                            "site_data_refresh" => {
                                if let Err(error) =
                                    site_data::list_cookies(&tab.view, &tab.url, move |result| {
                                        let _ = proxy.send_event(UserEvent::SiteCookies(
                                            tab_id, origin, result,
                                        ));
                                    })
                                {
                                    self.show_site_data_error(error);
                                }
                            }
                            "site_cookie_delete" => {
                                let name = value
                                    .get("name")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or("");
                                let domain = value
                                    .get("domain")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or("");
                                let path = value
                                    .get("path")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or("");
                                let host = url::Url::parse(&origin)
                                    .ok()
                                    .and_then(|url| url.host_str().map(str::to_owned))
                                    .unwrap_or_default();
                                if !site_data::cookie_domain_matches_host(domain, &host) {
                                    self.show_site_data_error(
                                        "Cookie не относится к текущему сайту".into(),
                                    );
                                    return;
                                }
                                let event_origin = origin.clone();
                                let proxy = proxy.clone();
                                if let Err(error) = site_data::delete_cookie(
                                    &tab.view,
                                    &tab.url,
                                    name,
                                    domain,
                                    path,
                                    move |result| {
                                        let _ = proxy.send_event(UserEvent::SiteDataDone(
                                            tab_id,
                                            event_origin,
                                            "cookie".into(),
                                            result,
                                        ));
                                    },
                                ) {
                                    self.show_site_data_error(error);
                                }
                            }
                            "site_data_clear_origin" => {
                                let event_origin = origin.clone();
                                if let Err(error) = site_data::clear_origin_storage(
                                    &tab.view,
                                    &origin,
                                    move |result| {
                                        let _ = proxy.send_event(UserEvent::SiteDataDone(
                                            tab_id,
                                            event_origin,
                                            "origin".into(),
                                            result,
                                        ));
                                    },
                                ) {
                                    self.show_site_data_error(error);
                                }
                            }
                            "site_data_clear_cache" => {
                                if let Err(error) =
                                    site_data::clear_profile_cache(&tab.view, move |result| {
                                        let _ = proxy.send_event(UserEvent::ProfileCacheDone(
                                            tab_id, result,
                                        ));
                                    })
                                {
                                    self.show_site_data_error(error);
                                }
                            }
                            _ => {}
                        }
                    }
                    #[cfg(not(windows))]
                    self.show_site_data_error(
                        "Управление данными сайтов пока доступно только в WebView2".into(),
                    );
                }
                "privacy_refresh_stats" => self.refresh_privacy_panel(),
                "privacy_toggle_site" => {
                    let host = self
                        .active_tab
                        .and_then(|id| self.tabs.iter().find(|tab| tab.id == id))
                        .and_then(|tab| url::Url::parse(&tab.url).ok())
                        .and_then(|url| url.host_str().map(str::to_owned));
                    let tab = self
                        .active_tab
                        .and_then(|id| self.tabs.iter().find(|t| t.id == id));
                    let private = tab.is_some_and(|t| t.is_private);
                    let state = tab.map(|t| &t.privacy);
                    if let (Some(state), Some(host)) = (state, host)
                        && state.toggle_site(&host)
                    {
                        let mut sites: Vec<_> = state.allowed_sites().into_iter().collect();
                        sites.sort_unstable();
                        if private {
                            // Private exceptions remain in this tab only.
                        } else if let Some(vault) = self.active_vault.as_ref() {
                            if let Ok(bytes) = serde_json::to_vec(&sites)
                                && let Err(error) = vault.put("privacy-sites", "exceptions", &bytes)
                            {
                                eprintln!("Aurora encrypted site exception save failed: {error}");
                            }
                        } else if let Some(path) = &self.privacy_path
                            && let Err(error) = write_json_atomically(path, &sites)
                        {
                            eprintln!("Aurora site exception save failed: {error}");
                        }
                        if let Some(tab) = self.active_tab_mut() {
                            let _ = tab.view.evaluate_script("location.reload()");
                        }
                        self.refresh_privacy_panel();
                    }
                }
                "clear_history" => {
                    if let Some(history) = self.history.as_mut() {
                        history.clear();
                        if let Some(vault) = self.active_vault.as_ref() {
                            persist_vault_records(
                                vault,
                                "history",
                                &[] as &[HistoryEntry],
                                |entry| &entry.url,
                            );
                        } else if let Some(path) = &self.history_path {
                            let _ = write_json_atomically(path, history);
                        }
                    }
                    self.refresh_chrome();
                    if let Some(chrome) = self.chrome.as_ref() {
                        let _ = chrome.evaluate_script("window.showToast('История очищена')");
                    }
                }
                "open_downloads" => {
                    let downloads = std::env::var_os("USERPROFILE")
                        .map(PathBuf::from)
                        .unwrap_or_else(std::env::temp_dir)
                        .join("Downloads");
                    let _ = std::process::Command::new("explorer.exe")
                        .arg(downloads)
                        .spawn();
                }
                "downloads_refresh" => self.refresh_downloads_overlay(),
                "download_cancel" => {
                    if let Some(id) = command
                        .value
                        .as_deref()
                        .and_then(|id| id.parse::<u64>().ok())
                        && self.downloads.iter().any(|download| {
                            download.id == id
                                && download.state == "in_progress"
                                && self.download_is_visible(download)
                        })
                        && let Err(error) = self.download_control.cancel(id)
                        && let (Some(overlay), Ok(message)) =
                            (self.overlay.as_ref(), serde_json::to_string(&error))
                    {
                        let _ =
                            overlay.evaluate_script(&format!("window.downloadError({message})"));
                    }
                }
                "download_show_file" => {
                    if let Some(id) = command
                        .value
                        .as_deref()
                        .and_then(|id| id.parse::<u64>().ok())
                    {
                        self.open_downloaded_file(id);
                    }
                }
                "settings_update" => {
                    if let Some(value) = command.value.as_deref() {
                        if let Some(value) = value.strip_prefix("search_engine=") {
                            if matches!(value, "duckduckgo" | "google") {
                                self.settings.search_engine = value.to_owned();
                            }
                        } else if let Some(value) = value.strip_prefix("animations=") {
                            self.settings.animations = value == "true";
                        } else if let Some(value) = value.strip_prefix("theme=") {
                            if matches!(value, "dark" | "light") {
                                self.settings.theme = value.to_owned();
                            }
                        } else if let Some(value) = value.strip_prefix("homepage=") {
                            if matches!(value, "aurora" | "duckduckgo") {
                                self.settings.homepage = value.to_owned();
                            }
                        } else if let Some(value) = value.strip_prefix("adblock_enabled=") {
                            self.settings.adblock_enabled = value == "true";
                            if let Some(state) = self.privacy_state.as_ref() {
                                state.set_enabled(self.settings.adblock_enabled);
                                for tab in &self.tabs {
                                    tab.privacy.set_enabled(self.settings.adblock_enabled);
                                }
                            }
                        } else if let Some(value) = value.strip_prefix("https_first=") {
                            self.settings.https_first = value == "true";
                            for tab in &self.tabs {
                                tab.https_first_enabled.store(
                                    self.settings.https_first,
                                    std::sync::atomic::Ordering::Release,
                                );
                            }
                        }
                        if let Some(path) = &self.settings_path {
                            let _ = write_json_atomically(path, &self.settings);
                        }
                        if let Some(chrome) = self.chrome.as_ref() {
                            let _ = chrome.evaluate_script(&format!(
                                "document.documentElement.classList.toggle('no-motion', {}); document.documentElement.classList.toggle('light', {});",
                                !self.settings.animations, self.settings.theme == "light"
                            ));
                        }
                        if let Some(overlay) = self.overlay.as_ref() {
                            let _ = overlay.evaluate_script(&format!(
                                "document.documentElement.classList.toggle('no-motion', {}); document.documentElement.classList.toggle('light', {});",
                                !self.settings.animations, self.settings.theme == "light"
                            ));
                        }
                        let page_background = if self.settings.theme == "light" {
                            (234, 240, 249, 255)
                        } else {
                            (16, 19, 27, 255)
                        };
                        for tab in &self.tabs {
                            let _ = tab.view.set_background_color(page_background);
                        }
                        self.update_overlay(false);
                        self.refresh_privacy_panel();
                    }
                }
                "select_tab" => {
                    if let Some(id) = command.tab_id {
                        self.select_tab(id);
                    }
                }
                "reorder_tabs" => {
                    if let Some(ids) = command.value.as_deref() {
                        self.reorder_tabs(ids);
                    }
                }
                "close_tab" => {
                    if let Some(id) = command.tab_id.or(self.active_tab) {
                        self.close_tab(event_loop, id);
                    }
                }
                "drag" => {
                    if let Some(window) = self.window.as_ref() {
                        let _ = window.drag_window();
                    }
                }
                "minimize" => {
                    if let Some(window) = self.window.as_ref() {
                        window.set_minimized(true);
                    }
                }
                "maximize" => {
                    if let Some(window) = self.window.as_ref() {
                        window.set_maximized(!window.is_maximized());
                    }
                }
                "close" => event_loop.exit(),
                _ => {}
            },
            UserEvent::PageTitle(id, title) => {
                if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
                    tab.title = if title.trim().is_empty() {
                        "Новая вкладка".into()
                    } else {
                        title
                    };
                    let current_title = tab.title.clone();
                    let current_url = tab.url.clone();
                    if let Some(history) = self.history.as_mut()
                        && let Some(entry) =
                            history.iter_mut().find(|entry| entry.url == current_url)
                    {
                        entry.title = current_title;
                    }
                    if let Some(vault) = self.active_vault.as_ref() {
                        persist_vault_records(
                            vault,
                            "history",
                            self.history.as_deref().unwrap_or(&[]),
                            |entry| &entry.url,
                        );
                    } else if let (Some(path), Some(history)) =
                        (&self.history_path, self.history.as_ref())
                    {
                        let _ = write_json_atomically(path, history);
                    }
                    self.refresh_chrome();
                }
            }
            UserEvent::PageLoaded(id, url) => {
                if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
                    if url == "about:blank" && tab.awaiting_first_navigation {
                        return;
                    }
                    tab.is_loading = false;
                    tab.first_load_complete = true;
                    tab.awaiting_first_navigation = false;
                    tab.url = if url == "about:blank" {
                        HOME_PAGE.into()
                    } else {
                        url.clone()
                    };
                    if self.active_tab == Some(id)
                        && let Some(window) = self.window.as_ref()
                    {
                        let size = window.inner_size().to_logical::<u32>(window.scale_factor());
                        let _ = tab.view.set_bounds(Rect {
                            position: LogicalPosition::new(0, CHROME_HEIGHT).into(),
                            size: LogicalSize::new(
                                size.width,
                                size.height.saturating_sub(CHROME_HEIGHT),
                            )
                            .into(),
                        });
                        let _ = tab.view.set_visible(true);
                        if !self.window_shown {
                            let proxy = self.proxy.as_ref().expect("event loop proxy").clone();
                            std::thread::spawn(move || {
                                // WebView2's load-finished event precedes its first composed
                                // frame. Let it paint while the top-level window stays hidden.
                                std::thread::sleep(std::time::Duration::from_secs(1));
                                let _ = proxy.send_event(UserEvent::ShowWindow);
                            });
                        }
                    }
                    self.record_history(id, url);
                    self.refresh_privacy_panel();
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.window.as_ref().map(Window::id) != Some(window_id) {
            return;
        }
        match event {
            WindowEvent::Resized(physical_size) => {
                if let Some(window) = self.window.as_ref() {
                    let size = physical_size.to_logical::<u32>(window.scale_factor());
                    if let Some(chrome) = self.chrome.as_ref() {
                        let _ = chrome.set_bounds(Rect {
                            position: LogicalPosition::new(0, 0).into(),
                            size: LogicalSize::new(size.width, size.height).into(),
                        });
                    }
                    if let Some(overlay) = self.overlay.as_ref() {
                        let _ = overlay.set_bounds(Rect {
                            position: LogicalPosition::new(0, 0).into(),
                            size: LogicalSize::new(size.width, size.height).into(),
                        });
                    }
                    for tab in &self.tabs {
                        let _ = tab.view.set_bounds(Rect {
                            position: LogicalPosition::new(0, CHROME_HEIGHT).into(),
                            size: LogicalSize::new(
                                size.width,
                                size.height.saturating_sub(CHROME_HEIGHT),
                            )
                            .into(),
                        });
                    }
                    // A maximized window may report its initial 1440x900
                    // client size before the OS delivers the real size. Keep
                    // page views hidden until this first resize has applied
                    // the final child bounds, avoiding a white stale-size flash.
                    if !self.layout_ready {
                        self.layout_ready = true;
                        if let Some(active_id) = self.active_tab
                            && let Some(tab) = self
                                .tabs
                                .iter()
                                .find(|tab| tab.id == active_id && tab.first_load_complete)
                        {
                            let _ = tab.view.set_visible(true);
                        }
                    }
                }
            }
            WindowEvent::CloseRequested => {
                self.cancel_active_downloads();
                self.persist_downloads();
                event_loop.exit();
            }
            _ => {}
        }
    }
}

fn resolve_input(input: &str, search_engine: &str, https_first: bool) -> String {
    let has_scheme = input.split_once("://").is_some();
    let looks_like_host = input.contains('.') && !input.chars().any(char::is_whitespace);
    if has_scheme {
        input.to_owned()
    } else if input.starts_with("localhost")
        || input.starts_with("127.0.0.1")
        || input.starts_with("[::1]")
    {
        format!("http://{input}")
    } else if looks_like_host {
        let local_http = format!("http://{input}");
        if https_first::is_local_http_url(&local_http) {
            local_http
        } else {
            format!("{}://{input}", if https_first { "https" } else { "http" })
        }
    } else {
        let query: String = url::form_urlencoded::byte_serialize(input.as_bytes()).collect();
        if search_engine == "google" {
            format!("https://www.google.com/search?q={query}")
        } else {
            format!("https://duckduckgo.com/?q={query}")
        }
    }
}

#[cfg(windows)]
fn raise_webview(view: &WebView) {
    use windows::Win32::UI::WindowsAndMessaging::{
        HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
    };

    // Child WebViews retain their native z-order when hidden. Explicitly raise
    // the overlay whenever it opens so the page WebView cannot paint over its
    // card; restore the active page after the overlay closes.
    unsafe {
        let _ = SetWindowPos(
            view.hwnd(),
            Some(HWND_TOP),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// Persist browser state through a same-directory temporary file so a crash
/// cannot leave the previous JSON file half-written.
fn write_json_atomically<T: Serialize>(path: &std::path::Path, value: &T) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(value)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    use std::io::Write;
    let mut file = fs::File::create(&temporary)?;
    file.write_all(&json)?;
    file.sync_all()?;
    drop(file);
    #[cfg(windows)]
    {
        use windows::{
            Win32::Storage::FileSystem::{
                MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
            },
            core::HSTRING,
        };
        let source = HSTRING::from(temporary.as_path());
        let destination = HSTRING::from(path);
        if let Err(error) = unsafe {
            MoveFileExW(
                &source,
                &destination,
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } {
            let _ = fs::remove_file(&temporary);
            return Err(std::io::Error::other(error));
        }
    }
    #[cfg(not(windows))]
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(path: &Option<PathBuf>) -> T {
    path.as_ref()
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn load_vault_records<T: for<'de> Deserialize<'de>>(
    vault: &vault::ProfileVault,
    domain: &str,
) -> Result<Vec<T>, String> {
    let records = vault
        .list(domain)
        .map_err(|error| format!("Не удалось расшифровать хранилище профиля: {error}"))?;
    records
        .into_iter()
        .map(|(_, bytes)| {
            serde_json::from_slice(&bytes)
                .map_err(|_| format!("В зашифрованной области {domain} повреждена запись"))
        })
        .collect()
}

fn load_vault_record<T: for<'de> Deserialize<'de>>(
    vault: &vault::ProfileVault,
    domain: &str,
    id: &str,
) -> Result<Option<T>, String> {
    let records = vault
        .list(domain)
        .map_err(|error| format!("Не удалось расшифровать хранилище профиля: {error}"))?;
    let Some((_, bytes)) = records.into_iter().find(|(record_id, _)| record_id == id) else {
        return Ok(None);
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| format!("В зашифрованной области {domain} повреждена запись"))
}

fn persist_vault_records<T: Serialize>(
    vault: &vault::ProfileVault,
    domain: &str,
    entries: &[T],
    identity: impl Fn(&T) -> &str,
) {
    let mut encrypted_records = Vec::with_capacity(entries.len());
    for entry in entries {
        let Ok(id) = vault.opaque_record_id(domain, identity(entry)) else {
            continue;
        };
        let Ok(bytes) = serde_json::to_vec(entry) else {
            continue;
        };
        encrypted_records.push((id, bytes));
    }
    let _ = vault.replace_records(domain, &encrypted_records);
}

fn main() -> wry::Result<()> {
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .expect("create event loop");
    let mut app = AuroraApp {
        proxy: Some(event_loop.create_proxy()),
        ..AuroraApp::default()
    };
    event_loop.run_app(&mut app).expect("run Aurora");
    Ok(())
}

const CHROME_HTML: &str = include_str!("../ui/chrome.html");
const NEW_TAB_HTML: &str = include_str!("../ui/new_tab.html");
const AURORA_LOGO_SVG: &str = include_str!("../assets/aurora.svg");
const OVERLAY_HTML: &str = include_str!("../ui/overlay.html");

// NavigateToString can surface as a data URL in NavigationStarting. Only the
// exact compiled-in document may occupy a view with Aurora IPC privileges.
fn is_trusted_ui_navigation(url: &str, html: &str) -> bool {
    use base64::Engine;
    // Bound work before decoding an attacker-controlled URL.
    if url.len() > html.len().saturating_mul(4).saturating_add(256) {
        return false;
    }
    let Some((header, payload)) = url.split_once(',') else {
        return false;
    };
    let mut parts = header.split(';');
    if parts.next() != Some("data:text/html") {
        return false;
    }
    let mut encoded = false;
    for part in parts {
        match part {
            "base64" if !encoded => encoded = true,
            "charset=utf-8" | "charset=UTF-8" => {}
            _ => return false,
        }
    }
    let bytes = percent_encoding::percent_decode_str(payload).collect::<Vec<_>>();
    if encoded {
        base64::engine::general_purpose::STANDARD
            .decode(bytes)
            .is_ok_and(|decoded| decoded == html.as_bytes())
    } else {
        bytes == html.as_bytes()
    }
}

fn new_tab_html(settings: &BrowserSettings) -> String {
    let action = if settings.search_engine == "google" {
        "https://www.google.com/search"
    } else {
        "https://duckduckgo.com/"
    };
    let html = NEW_TAB_HTML
        .replace("{{AURORA_LOGO}}", AURORA_LOGO_SVG)
        .replace("https://search.aurora.invalid/", action);
    if settings.theme == "light" {
        html.replace("<html lang=\"ru\">", "<html lang=\"ru\" class=\"light\">")
    } else {
        html
    }
}

fn browser_icon() -> Icon {
    let image = image::load_from_memory(include_bytes!("../assets/aurora.png"))
        .expect("load Aurora window icon")
        .to_rgba8();
    let (width, height) = image.dimensions();
    Icon::from_rgba(image.into_raw(), width, height).expect("create Aurora window icon")
}

#[cfg(test)]
mod ui_navigation_tests {
    use super::*;
    use base64::Engine;
    #[test]
    fn embedded_documents_load_and_foreign_documents_are_rejected() {
        for html in [CHROME_HTML, OVERLAY_HTML] {
            let encoded =
                percent_encoding::utf8_percent_encode(html, percent_encoding::NON_ALPHANUMERIC);
            assert!(is_trusted_ui_navigation(
                &format!("data:text/html;charset=utf-8,{encoded}"),
                html
            ));
            let encoded = base64::engine::general_purpose::STANDARD.encode(html);
            assert!(is_trusted_ui_navigation(
                &format!("data:text/html;base64,{encoded}"),
                html
            ));
            for foreign in [
                "https://example.com",
                "about:blank",
                "javascript:alert(1)",
                "data:text/html,<script>alert(1)</script>",
                "data:text/html;base64,!!!!",
            ] {
                assert!(!is_trusted_ui_navigation(foreign, html));
            }
        }
    }

    #[test]
    fn address_resolution_prefers_https_but_keeps_local_http() {
        assert_eq!(
            resolve_input("example.com/path", "duckduckgo", true),
            "https://example.com/path"
        );
        assert_eq!(
            resolve_input("example.com/path", "duckduckgo", false),
            "http://example.com/path"
        );
        assert_eq!(
            resolve_input("localhost:8080", "duckduckgo", true),
            "http://localhost:8080"
        );
        assert_eq!(
            resolve_input("192.168.1.20/printer", "duckduckgo", true),
            "http://192.168.1.20/printer"
        );
        assert_eq!(
            resolve_input("nas.local", "duckduckgo", true),
            "http://nas.local"
        );
    }

    #[test]
    fn corrupt_encrypted_bookmark_record_fails_profile_loading() {
        let root =
            std::env::temp_dir().join(format!("aurora-record-load-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let result = {
            let created = vault::ProfileVault::create(&root, "Test", "test password").unwrap();
            created
                .vault
                .put("bookmarks", "corrupt", b"not json")
                .unwrap();
            load_vault_records::<Bookmark>(&created.vault, "bookmarks")
        };
        fs::remove_dir_all(&root).unwrap();
        assert!(
            result.is_err(),
            "corrupt encrypted data must not look empty"
        );
    }

    #[test]
    fn corrupt_encrypted_permission_policy_fails_profile_loading() {
        let root =
            std::env::temp_dir().join(format!("aurora-policy-load-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let result = {
            let created = vault::ProfileVault::create(&root, "Test", "test password").unwrap();
            created
                .vault
                .put("site-permissions", "policy", b"not json")
                .unwrap();
            load_vault_record::<site_permissions::Policy>(
                &created.vault,
                "site-permissions",
                "policy",
            )
        };
        fs::remove_dir_all(&root).unwrap();
        assert!(
            result.is_err(),
            "corrupt permission decisions must not be replaced with defaults"
        );
    }
}
