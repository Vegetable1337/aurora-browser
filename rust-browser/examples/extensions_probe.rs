//! Isolated Windows-only extension experiment. Never opens Aurora user data.
#[cfg(not(windows))]
fn main() {
    eprintln!("This experiment requires Windows WebView2");
    std::process::exit(2);
}

#[cfg(windows)]
mod probe {
    use std::result::Result;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        path::PathBuf,
        time::Duration,
    };
    use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
    use windows::core::{HSTRING, Interface};
    use winit::{
        application::ApplicationHandler,
        event::WindowEvent,
        event_loop::{ActiveEventLoop, EventLoop},
        window::{Window, WindowId},
    };
    use wry::{WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows, WebViewExtWindows};

    struct App {
        operation: String,
        directory: PathBuf,
        extension: PathBuf,
        proxy: winit::event_loop::EventLoopProxy<Result<String, String>>,
        window: Option<Window>,
        view: Option<WebView>,
        context: Option<WebContext>,
        code: i32,
    }
    impl ApplicationHandler<Result<String, String>> for App {
        fn resumed(&mut self, el: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            if let Err(error) = self.start(el) {
                let _ = self.proxy.send_event(Err(error.to_string()));
            }
        }
        fn user_event(&mut self, el: &ActiveEventLoop, result: Result<String, String>) {
            match result {
                Ok(value) => {
                    println!("{value}");
                    self.code = 0;
                }
                Err(error) => {
                    eprintln!("Probe failed: {error}");
                    self.code = 1;
                }
            }
            el.exit();
        }
        fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
            if matches!(event, WindowEvent::CloseRequested) {
                el.exit();
            }
        }
    }
    impl App {
        fn start(&mut self, el: &ActiveEventLoop) -> Result<(), Box<dyn std::error::Error>> {
            let window = el.create_window(
                Window::default_attributes()
                    .with_title("Aurora extension probe")
                    .with_visible(false),
            )?;
            let mut context = WebContext::new(Some(self.directory.clone()));
            let proxy = self.proxy.clone();
            let view = WebViewBuilder::new_with_web_context(&mut context)
                .with_browser_extensions_enabled(true)
                .with_url("about:blank")
                .with_document_title_changed_handler(move |title| {
                    eprintln!("Probe document title changed: {title:?}");
                    if let Some(proof) = title.strip_prefix("AURORA_MV3_OK:") {
                        let _ = proxy.send_event(Ok(proof.into()));
                    }
                    if title == "AURORA_PROBE_ERROR" {
                        let _ = proxy.send_event(Err("extension handshake failed".into()));
                    }
                })
                .with_on_page_load_handler(|event, url| match event {
                    wry::PageLoadEvent::Started => eprintln!("Probe navigation started: {url}"),
                    wry::PageLoadEvent::Finished => eprintln!("Probe navigation finished: {url}"),
                })
                .build(&window)?;
            let profile = unsafe {
                view.controller()
                    .CoreWebView2()?
                    .cast::<ICoreWebView2_13>()?
                    .Profile()?
                    .cast::<ICoreWebView2Profile7>()?
            };
            let mut version = windows::core::PWSTR::null();
            unsafe {
                view.environment().BrowserVersionString(&mut version)?;
            }
            eprintln!("WebView2 runtime: {}", take_pwstr(version));
            let proxy = self.proxy.clone();
            match self.operation.as_str() {
                "install" => {
                    let path = HSTRING::from(self.extension.canonicalize()?.as_path());
                    let cb = ProfileAddBrowserExtensionCompletedHandler::create(Box::new(
                        move |hr, extension| {
                            let result = hr.and_then(|_| {
                                extension.ok_or_else(|| {
                                    windows::core::Error::from_hresult(windows::core::HRESULT(
                                        0x80004005u32 as i32,
                                    ))
                                })
                            });
                            let _ = proxy.send_event(
                                result
                                    .map(|_| "installed".into())
                                    .map_err(|e| e.to_string()),
                            );
                            Ok(())
                        },
                    ));
                    unsafe {
                        profile.AddBrowserExtension(&path, &cb)?;
                    }
                }
                "probe" => {
                    let server = TcpListener::bind("127.0.0.1:0")?;
                    let url = format!("http://127.0.0.1:{}/probe", server.local_addr()?.port());
                    std::thread::spawn(move || {
                        for mut stream in server.incoming().flatten() {
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                            let mut request = [0u8; 4096];
                            let _ = stream.read(&mut request);
                            let body = "<!doctype html><title>Aurora probe waiting</title><h1>Extension test page</h1>";
                            let _ = write!(
                                stream,
                                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                body.len(),
                                body
                            );
                        }
                    });
                    // Do not navigate until WebView2 has asynchronously enumerated
                    // this profile's persisted extension. Navigating immediately
                    // after environment creation can race content-script startup.
                    let core = unsafe { view.controller().CoreWebView2()? };
                    let target = HSTRING::from(url);
                    let proxy = self.proxy.clone();
                    let callback = ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(
                        move |hr, list| {
                            let result = (|| -> windows::core::Result<Option<bool>> {
                                hr?;
                                let list = list.ok_or_else(windows::core::Error::from_thread)?;
                                let mut count = 0;
                                unsafe {
                                    list.Count(&mut count)?;
                                }
                                for index in 0..count {
                                    let extension = unsafe { list.GetValueAtIndex(index)? };
                                    let mut name = windows::core::PWSTR::null();
                                    unsafe {
                                        extension.Name(&mut name)?;
                                    }
                                    if take_pwstr(name) == "Aurora MV3 local probe" {
                                        let mut enabled = windows::core::BOOL(0);
                                        unsafe {
                                            extension.IsEnabled(&mut enabled)?;
                                        }
                                        return Ok(Some(enabled.as_bool()));
                                    }
                                }
                                Ok(None)
                            })();
                            match result {
                                Ok(Some(enabled)) => {
                                    eprintln!("MV3 fixture enabled before navigation: {enabled}");
                                    if let Err(error) = unsafe { core.Navigate(&target) } {
                                        let _ = proxy.send_event(Err(error.to_string()));
                                    }
                                }
                                Ok(None) => {
                                    let _ = proxy.send_event(Err(
                                        "MV3 fixture was not present before navigation".into(),
                                    ));
                                }
                                Err(error) => {
                                    let _ = proxy.send_event(Err(error.to_string()));
                                }
                            }
                            Ok(())
                        },
                    ));
                    unsafe {
                        profile.GetBrowserExtensions(&callback)?;
                    }
                }
                operation => {
                    let operation = operation.to_owned();
                    let cb = ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(
                        move |hr, list| {
                            let result = (|| -> windows::core::Result<()> {
                                hr?;
                                let list = list.ok_or_else(windows::core::Error::from_thread)?;
                                let mut count = 0;
                                unsafe {
                                    list.Count(&mut count)?;
                                }
                                let mut fixtures = Vec::new();
                                for index in 0..count {
                                    let extension = unsafe { list.GetValueAtIndex(index)? };
                                    let mut name = windows::core::PWSTR::null();
                                    unsafe {
                                        extension.Name(&mut name)?;
                                    }
                                    let name = take_pwstr(name);
                                    eprintln!("Extension: {name}");
                                    if name == "Aurora MV3 local probe" {
                                        fixtures.push(extension);
                                    }
                                }
                                if operation == "list" {
                                    let mut enabled = windows::core::BOOL(0);
                                    if let Some(extension) = fixtures.first() {
                                        unsafe {
                                            extension.IsEnabled(&mut enabled)?;
                                        }
                                    }
                                    let _ = proxy.send_event(Ok(format!(
                                        "fixture_count={} enabled={}",
                                        fixtures.len(),
                                        enabled.as_bool()
                                    )));
                                    return Ok(());
                                }
                                if fixtures.len() != 1 {
                                    let _ = proxy.send_event(Err(format!(
                                        "Expected one fixture, got {}",
                                        fixtures.len()
                                    )));
                                    return Ok(());
                                }
                                let extension = fixtures.remove(0);
                                if operation == "remove" {
                                    let cb = BrowserExtensionRemoveCompletedHandler::create(
                                        Box::new(move |hr| {
                                            let _ = proxy.send_event(
                                                hr.map(|_| "removed".into())
                                                    .map_err(|e| e.to_string()),
                                            );
                                            Ok(())
                                        }),
                                    );
                                    unsafe {
                                        extension.Remove(&cb)?;
                                    }
                                } else {
                                    let enabled = operation == "enable";
                                    let cb = BrowserExtensionEnableCompletedHandler::create(
                                        Box::new(move |hr| {
                                            let _ = proxy.send_event(
                                                hr.map(|_| format!("enabled={enabled}"))
                                                    .map_err(|e| e.to_string()),
                                            );
                                            Ok(())
                                        }),
                                    );
                                    unsafe {
                                        extension.Enable(enabled, &cb)?;
                                    }
                                }
                                Ok(())
                            })();
                            // Synchronous errors are returned to the COM caller and cause the watchdog to fail.
                            result
                        },
                    ));
                    unsafe {
                        profile.GetBrowserExtensions(&cb)?;
                    }
                }
            }
            self.view = Some(view);
            self.context = Some(context);
            self.window = Some(window);
            Ok(())
        }
    }
    pub fn run() {
        let args: Vec<String> = std::env::args().collect();
        if args.len() != 4
            || !["install", "probe", "list", "enable", "disable", "remove"]
                .contains(&args[1].as_str())
        {
            eprintln!(
                "Usage: extensions_probe <install|probe|list|enable|disable|remove> <isolated-data-dir> <extension-dir>"
            );
            std::process::exit(2);
        }
        let directory = PathBuf::from(&args[2]);
        if !directory
            .parent()
            .is_some_and(|parent| parent.join(".aurora-extension-probe").is_file())
            || !matches!(
                directory.file_name().and_then(|name| name.to_str()),
                Some("A" | "B")
            )
        {
            eprintln!(
                "Refusing to open unmarked data directory. Use packaging/test-extensions-windows.ps1."
            );
            std::process::exit(2);
        }
        let el = EventLoop::<Result<String, String>>::with_user_event()
            .build()
            .expect("event loop");
        let proxy = el.create_proxy();
        let timeout = proxy.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(20));
            let _ = timeout.send_event(Err("20s timeout; no success claimed".into()));
        });
        let mut app = App {
            operation: args[1].clone(),
            directory,
            extension: PathBuf::from(&args[3]),
            proxy,
            window: None,
            view: None,
            context: None,
            code: 1,
        };
        el.run_app(&mut app).expect("run experiment");
        let code = app.code;
        drop(app);
        std::process::exit(code);
    }
}
#[cfg(windows)]
fn main() {
    probe::run();
}
