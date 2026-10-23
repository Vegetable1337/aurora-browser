//! Exercises WebView2 cookie and origin-storage APIs in a disposable profile.
use aurora_browser::site_data;

#[cfg(windows)]
fn main() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        time::Duration,
    };
    use winit::{
        application::ApplicationHandler,
        event::WindowEvent,
        event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
        window::{Window, WindowId},
    };
    use wry::{WebContext, WebView, WebViewBuilder};

    enum Event {
        Title(String),
        Cookies(Result<Vec<site_data::CookieInfo>, String>),
        CookieDeleted(Result<(), String>),
        Cleared(Result<(), String>),
        Cache(Result<(), String>),
        Timeout,
    }
    struct App {
        proxy: EventLoopProxy<Event>,
        root: std::path::PathBuf,
        origin: String,
        view: Option<WebView>,
        window: Option<Window>,
        context: Option<WebContext>,
        passed: bool,
        started: bool,
    }
    impl ApplicationHandler<Event> for App {
        fn resumed(&mut self, el: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            let window = el
                .create_window(
                    Window::default_attributes()
                        .with_title("Aurora WebView2 site data API test")
                        .with_visible(false),
                )
                .unwrap();
            let mut context = WebContext::new(Some(self.root.clone()));
            let proxy = self.proxy.clone();
            let view = WebViewBuilder::new_with_web_context(&mut context)
                .with_url("about:blank")
                .with_document_title_changed_handler(move |title| {
                    let _ = proxy.send_event(Event::Title(title));
                })
                .build(&window)
                .unwrap();
            view.load_url(&self.origin).unwrap();
            self.window = Some(window);
            self.view = Some(view);
            self.context = Some(context);
        }

        fn user_event(&mut self, el: &ActiveEventLoop, event: Event) {
            match event {
                Event::Title(title) if title == "DATA_READY" && !self.started => {
                    self.started = true;
                    let proxy = self.proxy.clone();
                    if let Err(error) = site_data::list_cookies(
                        self.view.as_ref().unwrap(),
                        &self.origin,
                        move |result| {
                            let _ = proxy.send_event(Event::Cookies(result));
                        },
                    ) {
                        eprintln!("FAIL get cookies: {error}");
                        el.exit();
                    }
                }
                Event::Cookies(Ok(cookies)) => {
                    let Some(cookie) = cookies.iter().find(|c| c.name == "aurora_site_data_probe")
                    else {
                        eprintln!("FAIL test cookie missing from WebView2 CookieManager");
                        el.exit();
                        return;
                    };
                    let view = self.view.as_ref().unwrap();
                    let proxy = self.proxy.clone();
                    if let Err(error) = site_data::delete_cookie(
                        view,
                        &self.origin,
                        &cookie.name,
                        &cookie.domain,
                        &cookie.path,
                        move |result| {
                            let _ = proxy.send_event(Event::CookieDeleted(result));
                        },
                    ) {
                        eprintln!("FAIL delete cookie: {error}");
                        el.exit();
                    }
                }
                Event::CookieDeleted(Ok(())) => {
                    if let Err(error) = self.view.as_ref().unwrap().evaluate_script(
                        "setTimeout(()=>document.title=document.cookie.includes('aurora_site_data_probe=')?'COOKIE_REMAINS':'COOKIE_DELETED',100)",
                    ) {
                        eprintln!("FAIL verify cookie deletion: {error}");
                        el.exit();
                    }
                }
                Event::CookieDeleted(Err(error)) => {
                    eprintln!("FAIL WebView2 cookie deletion: {error}");
                    el.exit();
                }
                Event::Title(title) if title == "COOKIE_DELETED" => {
                    if let Err(error) = self.view.as_ref().unwrap().evaluate_script(
                        "document.cookie='aurora_site_data_probe=secret; Path=/';localStorage.setItem('aurora-probe','secret')",
                    ) {
                        eprintln!("FAIL prepare origin-clear verification: {error}");
                        el.exit();
                        return;
                    }
                    let proxy = self.proxy.clone();
                    if let Err(error) = site_data::clear_origin_storage(
                        self.view.as_ref().unwrap(),
                        &self.origin,
                        move |result| {
                            let _ = proxy.send_event(Event::Cleared(result));
                        },
                    ) {
                        eprintln!("FAIL clear origin storage: {error}");
                        el.exit();
                    }
                }
                Event::Cookies(Err(error)) => {
                    eprintln!("FAIL get cookies: {error}");
                    el.exit();
                }
                Event::Title(title) if title == "COOKIE_REMAINS" => {
                    eprintln!("FAIL selected cookie remained visible to the page after WebView2 deletion");
                    el.exit();
                }
                Event::Cleared(Ok(())) => {
                    let proxy = self.proxy.clone();
                    if let Err(error) =
                        site_data::clear_profile_cache(self.view.as_ref().unwrap(), move |result| {
                            let _ = proxy.send_event(Event::Cache(result));
                        })
                    {
                        eprintln!("FAIL clear profile cache: {error}");
                        el.exit();
                    }
                }
                Event::Cleared(Err(error)) => {
                    eprintln!("FAIL WebView2 Storage.clearDataForOrigin: {error}");
                    el.exit();
                }
                Event::Cache(Ok(())) => {
                    let script = "setTimeout(()=>document.title=(localStorage.getItem('aurora-probe')===null&&!document.cookie.includes('aurora_site_data_probe=')?'ORIGIN_CLEARED':'DATA_REMAINS'),40)";
                    if let Err(error) = self.view.as_ref().unwrap().evaluate_script(script) {
                        eprintln!("FAIL verify cleared origin: {error}");
                        el.exit();
                    }
                }
                Event::Cache(Err(error)) => {
                    eprintln!("FAIL WebView2 profile cache clear: {error}");
                    el.exit();
                }
                Event::Title(title) if title == "ORIGIN_CLEARED" => {
                    println!(
                        "PASS WebView2 cookie listing/deletion, origin storage and profile cache clearing verified"
                    );
                    self.passed = true;
                    el.exit();
                }
                Event::Title(title) if title == "DATA_REMAINS" => {
                    eprintln!("FAIL cookie or localStorage remained after cleanup");
                    el.exit();
                }
                Event::Timeout => {
                    eprintln!("FAIL 15s deadline, no success inferred");
                    el.exit();
                }
                _ => {}
            }
        }

        fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
            if matches!(event, WindowEvent::CloseRequested) {
                el.exit();
            }
        }
    }

    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://127.0.0.1:{}", server.local_addr().unwrap().port());
    std::thread::spawn(move || {
        for mut stream in server.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut request = [0; 4096];
            let _ = stream.read(&mut request);
            let body = r#"<!doctype html><title>WAITING</title><script>document.cookie='aurora_site_data_probe=secret; Path=/';localStorage.setItem('aurora-probe','secret');document.title='DATA_READY';</script>"#;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
        }
    });

    let root =
        std::env::temp_dir().join(format!("aurora-site-data-probe-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    println!("Disposable profile: {}", root.display());
    let event_loop = EventLoop::<Event>::with_user_event().build().unwrap();
    let proxy = event_loop.create_proxy();
    let timeout = proxy.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(15));
        let _ = timeout.send_event(Event::Timeout);
    });
    let mut app = App {
        proxy,
        root,
        origin: origin.clone(),
        view: None,
        window: None,
        context: None,
        passed: false,
        started: false,
    };
    event_loop.run_app(&mut app).unwrap();
    let passed = app.passed;
    drop(app);
    if !passed {
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Requires Windows WebView2");
    std::process::exit(2);
}
