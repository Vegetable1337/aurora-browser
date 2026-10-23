//! Real WebView2 permission enforcement test. Only creates a fresh temp profile.
use aurora_browser::site_permissions;

#[cfg(windows)]
fn main() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, RwLock},
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
        Legacy(Result<Vec<site_permissions::LegacyPermission>, String>),
        Ready(Result<(), String>),
        Title(String),
        Timeout,
    }
    struct App {
        proxy: EventLoopProxy<Event>,
        root: std::path::PathBuf,
        url: String,
        view: Option<WebView>,
        window: Option<Window>,
        context: Option<WebContext>,
        passed: bool,
        audit: Option<Arc<std::sync::atomic::AtomicU64>>,
    }
    impl ApplicationHandler<Event> for App {
        fn resumed(&mut self, el: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            let window = el
                .create_window(
                    Window::default_attributes()
                        .with_title("Aurora permission API test")
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
            let mut policy = site_permissions::Policy::default();
            policy
                .set(&self.url, "geolocation", site_permissions::Decision::Deny)
                .unwrap();
            self.audit =
                Some(site_permissions::attach(&view, Arc::new(RwLock::new(policy))).unwrap());
            let proxy = self.proxy.clone();
            site_permissions::read_legacy(&view, move |r| {
                let _ = proxy.send_event(Event::Legacy(r));
            })
            .unwrap();
            self.window = Some(window);
            self.view = Some(view);
            self.context = Some(context);
        }
        fn user_event(&mut self, el: &ActiveEventLoop, event: Event) {
            match event {
                Event::Legacy(Ok(settings)) => {
                    let proxy = self.proxy.clone();
                    site_permissions::reset_legacy(
                        self.view.as_ref().unwrap(),
                        settings,
                        move |r| {
                            let _ = proxy.send_event(Event::Ready(r));
                        },
                    )
                    .unwrap();
                }
                Event::Legacy(Err(e)) => {
                    eprintln!("FAIL reading legacy permissions: {e}");
                    el.exit();
                }
                Event::Ready(Ok(())) => {
                    self.view.as_ref().unwrap().load_url(&self.url).unwrap();
                }
                Event::Ready(Err(e)) => {
                    eprintln!("FAIL legacy permission reset: {e}");
                    el.exit();
                }
                Event::Title(t) if t == "PERMISSION_DENIED" => {
                    assert!(
                        self.audit
                            .as_ref()
                            .unwrap()
                            .load(std::sync::atomic::Ordering::Relaxed)
                            > 0,
                        "No actual Aurora permission callback observed"
                    );
                    println!("PASS actual geolocation request denied; Aurora callback observed");
                    self.passed = true;
                    el.exit();
                }
                Event::Title(t) if t == "UNEXPECTED_ALLOW" => {
                    eprintln!("FAIL unexpected permission grant");
                    el.exit();
                }
                Event::Timeout => {
                    eprintln!("FAIL 15s deadline, no success inferred");
                    el.exit();
                }
                _ => {}
            }
        }
        fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, e: WindowEvent) {
            if matches!(e, WindowEvent::CloseRequested) {
                el.exit();
            }
        }
    }
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}/", server.local_addr().unwrap().port());
    std::thread::spawn(move || {
        for mut stream in server.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = [0; 4096];
            let _ = stream.read(&mut buf);
            let body = r#"<!doctype html><title>WAITING</title><script>navigator.geolocation.getCurrentPosition(()=>document.title='UNEXPECTED_ALLOW',e=>document.title=e.code===1?'PERMISSION_DENIED':'OTHER_ERROR');</script>"#;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
        }
    });
    let root =
        std::env::temp_dir().join(format!("aurora-permissions-probe-{}", uuid::Uuid::new_v4()));
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
        url,
        view: None,
        window: None,
        context: None,
        passed: false,
        audit: None,
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
