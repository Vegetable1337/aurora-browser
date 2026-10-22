pub mod vault;

use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    window::{Icon, Window, WindowId},
};
use wry::{PageLoadEvent, PermissionResponse, Rect, WebView, WebViewBuilder, dpi::LogicalPosition};

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
    PageTitle(u64, String),
    PageLoading(u64, bool),
    PageLoaded(u64, String),
    MenuOpen(bool),
    OverlayHidden,
    OverlayReady,
}

struct BrowserTab {
    id: u64,
    title: String,
    url: String,
    is_loading: bool,
    is_private: bool,
    view: WebView,
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
}

impl Default for BrowserSettings {
    fn default() -> Self {
        Self {
            search_engine: "duckduckgo".into(),
            animations: true,
            theme: "dark".into(),
            homepage: "aurora".into(),
        }
    }
}

impl AuroraApp {
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
        let mut builder = if is_private {
            WebViewBuilder::new()
        } else {
            WebViewBuilder::new_with_web_context(self.context.as_mut().expect("checked context"))
        };
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
        builder = if url == HOME_PAGE && self.settings.homepage == "aurora" {
            builder.with_html(new_tab_html(&self.settings))
        } else {
            builder.with_url(url)
        };
        let builder = builder
            .with_incognito(is_private)
            .with_permission_handler(|_| PermissionResponse::Default)
            .with_focused(false)
            .with_visible(false)
            .with_background_color(page_background)
            .with_download_started_handler(move |url, path| {
                let name = url::Url::parse(&url)
                    .ok()
                    .and_then(|parsed| parsed.path_segments()?.next_back().map(str::to_owned))
                    .filter(|name| !name.is_empty())
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
        let view = builder
            .build_as_child(window)
            .expect("create Chromium page view");
        self.tabs.push(BrowserTab {
            id,
            title: "Новая вкладка".into(),
            url: target_url.into(),
            is_loading: true,
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
            let _ = tab.view.set_visible(tab.id == id);
        }
        self.refresh_chrome();
    }

    fn close_tab(&mut self, id: u64) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        if self.tabs.len() == 1 {
            let page = new_tab_html(&self.settings);
            if let Some(tab) = self.tabs.first_mut() {
                tab.title = "Новая вкладка".into();
                tab.url = HOME_PAGE.into();
                tab.is_loading = true;
                let _ = tab.view.load_html(&page);
            }
            self.refresh_chrome();
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

    fn open_overlay(&mut self) {
        if self.overlay.is_none() {
            let (Some(window), Some(context)) = (self.window.as_ref(), self.context.as_mut())
            else {
                return;
            };
            let proxy = self.proxy.as_ref().expect("event loop proxy").clone();
            let size = window.inner_size().to_logical::<u32>(window.scale_factor());
            let ready_proxy = proxy.clone();
            let overlay = WebViewBuilder::new_with_web_context(context)
                .with_html(OVERLAY_HTML)
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
    }

    fn activate_vault(&mut self, new_vault: vault::ProfileVault) {
        // Destroy views before their WebView2 context so profile data is released
        // and the old cookie store is no longer attached to any tab.
        self.overlay = None;
        self.tabs.clear();
        self.active_tab = None;
        self.context = None;
        let data_dir = new_vault.directory().join("WebView2");
        let _ = fs::create_dir_all(&data_dir);
        self.context = Some(wry::WebContext::new(Some(data_dir)));
        self.bookmarks = Some(load_vault_records(&new_vault, "bookmarks"));
        self.history = Some(load_vault_records(&new_vault, "history"));
        self.bookmarks_path = None;
        self.history_path = None;
        self.active_vault = Some(new_vault);
        self.create_tab(HOME_PAGE);
    }

    fn activate_default_profile(&mut self) {
        self.overlay = None;
        self.tabs.clear();
        self.active_tab = None;
        self.context = None;
        let Some(data_dir) = self.default_data_dir.clone() else {
            return;
        };
        self.context = Some(wry::WebContext::new(Some(data_dir.clone())));
        self.bookmarks_path = Some(data_dir.join("bookmarks.json"));
        self.history_path = Some(data_dir.join("history.json"));
        self.bookmarks = Some(read_json(&self.bookmarks_path));
        self.history = Some(read_json(&self.history_path));
        self.active_vault = None;
        self.create_tab(HOME_PAGE);
    }
}

#[derive(Default)]
struct AuroraApp {
    window: Option<Window>,
    chrome: Option<WebView>,
    tabs: Vec<BrowserTab>,
    active_tab: Option<u64>,
    next_tab_id: u64,
    context: Option<wry::WebContext>,
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
}

impl ApplicationHandler<UserEvent> for AuroraApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attributes = Window::default_attributes()
            .with_title("Aurora")
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

        let chrome = WebViewBuilder::new()
            .with_html(CHROME_HTML)
            .with_focused(true)
            .with_bounds(Rect {
                position: LogicalPosition::new(0, 0).into(),
                size: LogicalSize::new(size.width, CHROME_HEIGHT).into(),
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
        self.profiles_root = Some(data_dir.parent().unwrap_or(&data_dir).join("Profiles"));
        self.default_data_dir = Some(data_dir.clone());
        self.chrome = Some(chrome);
        self.window = Some(window);
        self.context = Some(context);
        self.create_tab(HOME_PAGE);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
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
                if let Some(chrome) = self.chrome.as_ref() {
                    let _ = chrome.evaluate_script("window.syncMenu(false)");
                }
            }
            UserEvent::OverlayReady => self.update_overlay(true),
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
                        let url = resolve_input(input, &self.settings.search_engine);
                        if let Some(tab) = self.active_tab_mut() {
                            tab.url = url.clone();
                            let _ = tab.view.load_url(&url);
                        }
                        self.refresh_chrome();
                    }
                }
                "open_url" => {
                    if let Some(url) = command.value.as_deref() {
                        if let Some(tab) = self.active_tab_mut() {
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
                                self.activate_vault(created.vault);
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
                                Ok(profile) => self.activate_vault(profile),
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
                                Ok(profile) => self.activate_vault(profile),
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
                "bookmark" => self.save_bookmark(),
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
                    }
                }
                "select_tab" => {
                    if let Some(id) = command.tab_id {
                        self.select_tab(id);
                    }
                }
                "close_tab" => {
                    if let Some(id) = command.tab_id.or(self.active_tab) {
                        self.close_tab(id);
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
                    if let Some(history) = self.history.as_mut() {
                        if let Some(entry) =
                            history.iter_mut().find(|entry| entry.url == current_url)
                        {
                            entry.title = current_title;
                        }
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
                    tab.is_loading = false;
                    tab.url = if url == "about:blank" {
                        HOME_PAGE.into()
                    } else {
                        url.clone()
                    };
                    self.record_history(id, url);
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
                            size: LogicalSize::new(size.width, CHROME_HEIGHT).into(),
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
                }
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            _ => {}
        }
    }
}

fn resolve_input(input: &str, search_engine: &str) -> String {
    let has_scheme = input.split_once("://").is_some();
    let looks_like_host = input.contains('.') && !input.chars().any(char::is_whitespace);
    if has_scheme {
        input.to_owned()
    } else if input.starts_with("localhost") || input.starts_with("127.0.0.1") {
        format!("http://{input}")
    } else if looks_like_host {
        format!("https://{input}")
    } else {
        let query: String = url::form_urlencoded::byte_serialize(input.as_bytes()).collect();
        if search_engine == "google" {
            format!("https://www.google.com/search?q={query}")
        } else {
            format!("https://duckduckgo.com/?q={query}")
        }
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
    fs::write(&temporary, json)?;
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
) -> Vec<T> {
    vault
        .list(domain)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(_, bytes)| serde_json::from_slice(&bytes).ok())
        .collect()
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
