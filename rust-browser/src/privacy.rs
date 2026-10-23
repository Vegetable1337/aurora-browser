//! Network request filtering for WebView2 tabs, backed by Brave's adblock-rust.
//!
//! WebView2 exposes HTTP(S) subresource requests through WebResourceRequested.
//! This module does not claim to filter browser-internal traffic, DNS, or every
//! request handled below that event by the runtime.

use adblock::{Engine, FilterSet, lists::ParseOptions, request::Request};
use std::collections::HashSet;
use std::io::Read;
use std::sync::{
    Arc, Mutex, RwLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

const BUILTIN_RULES: &str = "||doubleclick.net^\n||googlesyndication.com^\n||google-analytics.com^\n||googletagmanager.com^\n||facebook.net^\n||facebook.com/tr^\n||scorecardresearch.com^\n||adservice.google.com^\n";

pub struct PrivacyState {
    engine: Arc<RwLock<Arc<Engine>>>,
    enabled: AtomicBool,
    blocked: AtomicU64,
    allowed_sites: Mutex<HashSet<String>>,
}

impl PrivacyState {
    pub fn new(
        engine: Arc<RwLock<Arc<Engine>>>,
        enabled: bool,
        allowed_sites: HashSet<String>,
    ) -> Self {
        Self {
            engine,
            enabled: AtomicBool::new(enabled),
            blocked: AtomicU64::new(0),
            allowed_sites: Mutex::new(allowed_sites),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn blocked_count(&self) -> u64 {
        self.blocked.load(Ordering::Relaxed)
    }

    pub fn site_allowed(&self, host: &str) -> bool {
        self.allowed_sites
            .lock()
            .map(|sites| sites.contains(&host.trim_end_matches('.').to_ascii_lowercase()))
            .unwrap_or(false)
    }

    pub fn toggle_site(&self, host: &str) -> bool {
        let Ok(mut sites) = self.allowed_sites.lock() else {
            return false;
        };
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if !valid_site(&host) {
            return false;
        }
        if !sites.remove(&host) {
            sites.insert(host);
        }
        true
    }

    pub fn allowed_sites(&self) -> HashSet<String> {
        self.allowed_sites
            .lock()
            .map(|sites| sites.clone())
            .unwrap_or_default()
    }

    pub fn should_block(&self, url: &str, source: &str, kind: &str, method: &str) -> bool {
        if !self.is_enabled() {
            return false;
        }
        if url::Url::parse(source)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .is_some_and(|host| self.site_allowed(&host))
        {
            return false;
        }
        let Ok(request) = Request::new(url, source, kind, method) else {
            return false;
        };
        let Ok(engine) = self.engine.read() else {
            return false;
        };
        let blocked = engine.check_network_request(&request).should_block();
        if blocked {
            self.blocked.fetch_add(1, Ordering::Relaxed);
        }
        blocked
    }
}

pub fn build_engine(lists: &[String]) -> Engine {
    let mut filters = FilterSet::new(false);
    filters.add_filter_list(BUILTIN_RULES.to_owned(), ParseOptions::default());
    for list in lists {
        filters.add_filter_list(list.clone(), ParseOptions::default());
    }
    Engine::new_with_filter_set(filters)
}

#[cfg(windows)]
pub fn attach_to_view(view: &wry::WebView, state: Arc<PrivacyState>) -> Result<(), String> {
    use webview2_com::{
        Microsoft::Web::WebView2::Win32::*, WebResourceRequestedEventHandler, take_pwstr,
    };
    use windows::core::HSTRING;
    use wry::WebViewExtWindows;

    let core = unsafe { view.controller().CoreWebView2() }.map_err(|e| e.to_string())?;
    unsafe {
        core.AddWebResourceRequestedFilter(
            &HSTRING::from("https://*"),
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
        )
        .map_err(|e| e.to_string())?;
        core.AddWebResourceRequestedFilter(
            &HSTRING::from("http://*"),
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
        )
        .map_err(|e| e.to_string())?;
    }
    let environment = view.environment();
    let handler = WebResourceRequestedEventHandler::create(Box::new(move |sender, args| {
        let (Some(sender), Some(args)) = (sender, args) else {
            return Ok(());
        };
        unsafe {
            let request = args.Request()?;
            let mut uri = windows::core::PWSTR::null();
            let mut method = windows::core::PWSTR::null();
            request.Uri(&mut uri)?;
            request.Method(&mut method)?;
            let url = take_pwstr(uri);
            let method = take_pwstr(method);
            let mut source = windows::core::PWSTR::null();
            sender.Source(&mut source)?;
            let source = take_pwstr(source);
            let mut context = COREWEBVIEW2_WEB_RESOURCE_CONTEXT_OTHER;
            args.ResourceContext(&mut context)?;
            let kind = match context {
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT => "document",
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET => "stylesheet",
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE => "image",
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA => "media",
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT => "font",
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT => "script",
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST => "xmlhttprequest",
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH => "xhr",
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_EVENT_SOURCE => "xmlhttprequest",
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_WEBSOCKET => "websocket",
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING => "ping",
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_TEXT_TRACK => "other",
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MANIFEST => "other",
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SIGNED_EXCHANGE => "other",
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_CSP_VIOLATION_REPORT => "other",
                _ => "other",
            };
            if state.should_block(&url, &source, kind, &method) {
                let response = environment.CreateWebResourceResponse(
                    None::<&windows::Win32::System::Com::IStream>,
                    204,
                    &HSTRING::from("No Content"),
                    &HSTRING::from("Content-Length: 0\r\nCache-Control: no-store\r\n"),
                )?;
                args.SetResponse(&response)?;
            }
        }
        Ok(())
    }));
    let mut token = 0i64;
    unsafe { core.add_WebResourceRequested(&handler, &mut token) }.map_err(|e| e.to_string())
}

pub fn validate_filter_list(text: &str) -> Result<(), String> {
    if text.len() > 12 * 1024 * 1024 {
        return Err("Файл списка фильтров превышает 12 МиБ".into());
    }
    if text.lines().count() < 20
        || !text
            .lines()
            .any(|line| line.starts_with("||") || line.starts_with("@@"))
    {
        return Err("Список фильтров не прошёл проверку формата".into());
    }
    Ok(())
}

pub fn download_filter_lists() -> Result<Vec<String>, String> {
    const MAX_LIST_BYTES: u64 = 12 * 1024 * 1024;
    const SOURCES: [&str; 2] = [
        "https://easylist.to/easylist/easylist.txt",
        "https://easylist.to/easylist/easyprivacy.txt",
    ];
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout_read(std::time::Duration::from_secs(20))
        .build();
    SOURCES
        .iter()
        .map(|source| {
            let response = agent
                .get(source)
                .call()
                .map_err(|_| "Не удалось загрузить список фильтров".to_owned())?;
            let final_url = url::Url::parse(response.get_url())
                .map_err(|_| "Некорректный адрес списка фильтров".to_owned())?;
            if final_url.scheme() != "https" {
                return Err("Источник списка перенаправил на небезопасный адрес".into());
            }
            let source_url = url::Url::parse(source).expect("built-in HTTPS filter list URL");
            if final_url.host_str() != source_url.host_str() {
                return Err("Источник списка перенаправил на другой сервер".into());
            }
            if response
                .header("Content-Length")
                .and_then(|value| value.parse::<u64>().ok())
                .is_some_and(|size| size > MAX_LIST_BYTES)
            {
                return Err("Список фильтров превышает допустимый размер".into());
            }
            let mut text = String::new();
            response
                .into_reader()
                .take(MAX_LIST_BYTES + 1)
                .read_to_string(&mut text)
                .map_err(|_| "Не удалось прочитать список фильтров".to_owned())?;
            validate_filter_list(&text)?;
            Ok(text)
        })
        .collect()
}

pub fn load_cached_filter_lists(path: &std::path::Path) -> Vec<String> {
    const MAX_CACHE_BYTES: u64 = 26 * 1024 * 1024;
    let Ok(metadata) = std::fs::metadata(path) else {
        return Vec::new();
    };
    if !metadata.is_file() || metadata.len() > MAX_CACHE_BYTES {
        return Vec::new();
    }
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut json = String::new();
    if file
        .take(MAX_CACHE_BYTES + 1)
        .read_to_string(&mut json)
        .is_err()
        || json.len() as u64 > MAX_CACHE_BYTES
    {
        return Vec::new();
    }
    let Ok(lists) = serde_json::from_str::<Vec<String>>(&json) else {
        return Vec::new();
    };
    if lists.len() != 2 || !lists.iter().all(|list| validate_filter_list(list).is_ok()) {
        return Vec::new();
    }
    lists
}

fn valid_site(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|part| {
            !part.is_empty()
                && part.len() <= 63
                && part.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
                && !part.starts_with('-')
                && !part.ends_with('-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> PrivacyState {
        PrivacyState::new(
            Arc::new(RwLock::new(Arc::new(build_engine(&[])))),
            true,
            HashSet::new(),
        )
    }

    #[test]
    fn blocks_matching_third_party_resources_and_counts_real_matches() {
        let state = state();
        assert!(state.should_block(
            "https://google-analytics.com/collect",
            "https://news.example/",
            "xmlhttprequest",
            "GET"
        ));
        assert_eq!(state.blocked_count(), 1);
        assert!(!state.should_block(
            "https://cdn.example/app.js",
            "https://news.example/",
            "script",
            "GET"
        ));
    }

    #[test]
    fn disable_and_site_exception_bypass_matching_rules() {
        let state = state();
        assert!(state.toggle_site("News.Example"));
        assert!(!state.should_block(
            "https://google-analytics.com/collect",
            "https://news.example/article",
            "xmlhttprequest",
            "GET"
        ));
        assert_eq!(state.blocked_count(), 0);
        state.set_enabled(false);
        assert!(!state.should_block(
            "https://google-analytics.com/collect",
            "https://other.example/",
            "xmlhttprequest",
            "GET"
        ));
    }

    #[test]
    fn rejects_invalid_filter_payloads_and_site_values() {
        assert!(validate_filter_list("not a filter list").is_err());
        let state = state();
        assert!(!state.toggle_site("https://example.com/path"));
    }

    #[test]
    fn loaded_abp_rules_and_exceptions_are_applied() {
        let engine = build_engine(&[
            "||ads.cdn.example^\n@@||ads.cdn.example/allowed.js$domain=news.example".into(),
        ]);
        let state = PrivacyState::new(
            Arc::new(RwLock::new(Arc::new(engine))),
            true,
            HashSet::new(),
        );
        assert!(state.should_block(
            "https://ads.cdn.example/banner",
            "https://news.example/",
            "image",
            "GET"
        ));
        assert!(!state.should_block(
            "https://ads.cdn.example/allowed.js",
            "https://news.example/",
            "script",
            "GET"
        ));
        assert_eq!(state.blocked_count(), 1);
    }

    #[test]
    fn site_exceptions_do_not_cross_profiles() {
        let first = state();
        let second = state();
        first.toggle_site("news.example");
        assert!(first.site_allowed("news.example"));
        assert!(!second.site_allowed("news.example"));
    }
}
