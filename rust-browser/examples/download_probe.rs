//! Verifies WebView2 download progress and cancellation in a disposable profile.
use aurora_browser::downloads::{DownloadControl, DownloadUpdate};

#[cfg(windows)]
fn main() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        path::PathBuf,
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
        Download(DownloadUpdate),
        Timeout,
    }
    struct App {
        proxy: EventLoopProxy<Event>,
        root: PathBuf,
        output: PathBuf,
        url: String,
        view: Option<WebView>,
        window: Option<Window>,
        context: Option<WebContext>,
        control: DownloadControl,
        saw_progress: bool,
        passed: bool,
    }
    impl ApplicationHandler<Event> for App {
        fn resumed(&mut self, el: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            let window = el
                .create_window(
                    Window::default_attributes()
                        .with_title("Aurora download API test")
                        .with_visible(false),
                )
                .unwrap();
            let mut context = WebContext::new(Some(self.root.clone()));
            let output = self.output.clone();
            let view = WebViewBuilder::new_with_web_context(&mut context)
                .with_url("about:blank")
                .with_download_started_handler(move |_, path| {
                    *path = output.clone();
                    true
                })
                .build(&window)
                .unwrap();
            let proxy = self.proxy.clone();
            self.control
                .attach(&view, move |update| {
                    let _ = proxy.send_event(Event::Download(update));
                })
                .unwrap();
            view.load_url(&self.url).unwrap();
            self.window = Some(window);
            self.view = Some(view);
            self.context = Some(context);
        }

        fn user_event(&mut self, el: &ActiveEventLoop, event: Event) {
            match event {
                Event::Download(update)
                    if update.state == "in_progress"
                        && update.received > 0
                        && !self.saw_progress =>
                {
                    if std::path::Path::new(&update.path) != self.output.as_path() {
                        eprintln!(
                            "FAIL download manager reported unexpected path: {}",
                            update.path
                        );
                        el.exit();
                        return;
                    }
                    self.saw_progress = true;
                    if let Err(error) = self.control.cancel(update.id) {
                        eprintln!("FAIL cancel WebView2 download: {error}");
                        el.exit();
                    }
                }
                Event::Download(update) if update.state == "interrupted" => {
                    if self.saw_progress
                        && update.error.as_deref() == Some("Загрузка отменена пользователем")
                    {
                        println!(
                            "PASS actual WebView2 download reported progress and cancellation"
                        );
                        self.passed = true;
                    } else {
                        eprintln!("FAIL unexpected interruption state: {:?}", update.error);
                    }
                    el.exit();
                }
                Event::Download(update) if update.state == "completed" => {
                    eprintln!("FAIL fixture completed before the cancellation check");
                    el.exit();
                }
                Event::Timeout => {
                    eprintln!(
                        "FAIL 15s deadline; progress={}, output_exists={}",
                        self.saw_progress,
                        self.output.exists()
                    );
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
    let url = format!(
        "http://127.0.0.1:{}/fixture.bin",
        server.local_addr().unwrap().port()
    );
    std::thread::spawn(move || {
        for mut stream in server.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut request = [0; 4096];
            let _ = stream.read(&mut request);
            let total = 32 * 1024 * 1024usize;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=probe.bin\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n"
            );
            let bytes = [b'R'; 64 * 1024];
            for _ in 0..(total / bytes.len()) {
                if stream.write_all(&bytes).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    });
    let root = std::env::temp_dir().join(format!("aurora-download-probe-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let output = root.join("probe.bin");
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
        output,
        url,
        view: None,
        window: None,
        context: None,
        control: DownloadControl::default(),
        saw_progress: false,
        passed: false,
    };
    event_loop.run_app(&mut app).unwrap();
    if !app.passed {
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Requires Windows WebView2");
    std::process::exit(2);
}
