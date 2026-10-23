//! WebView2 site-data operations. Cookie values are never returned to the UI.

#[derive(Clone, Debug, serde::Serialize)]
pub struct CookieInfo {
    pub name: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub session: bool,
}

pub fn is_exact_web_origin(origin: &str) -> bool {
    let Ok(url) = url::Url::parse(origin) else {
        return false;
    };
    matches!(url.scheme(), "http" | "https") && url.origin().ascii_serialization() == origin
}

pub fn cookie_domain_matches_host(domain: &str, host: &str) -> bool {
    let domain = domain.trim_start_matches('.');
    !domain.is_empty() && (host == domain || host.ends_with(&format!(".{domain}")))
}

#[cfg(windows)]
pub fn list_cookies(
    view: &wry::WebView,
    uri: &str,
    done: impl FnOnce(Result<Vec<CookieInfo>, String>) + 'static,
) -> Result<(), String> {
    use webview2_com::{
        GetCookiesCompletedHandler, Microsoft::Web::WebView2::Win32::*, take_pwstr,
    };
    use windows::core::{BOOL, HSTRING, Interface, PWSTR};
    use wry::WebViewExtWindows;

    let manager = (|| unsafe {
        view.controller()
            .CoreWebView2()?
            .cast::<ICoreWebView2_13>()?
            .Profile()?
            .cast::<ICoreWebView2Profile5>()?
            .CookieManager()
    })()
    .map_err(|error: windows::core::Error| error.to_string())?;
    let uri = HSTRING::from(uri);
    let handler = GetCookiesCompletedHandler::create(Box::new(move |hr, list| {
        let result = (|| -> windows::core::Result<Vec<CookieInfo>> {
            hr?;
            let list = list.ok_or_else(windows::core::Error::from_thread)?;
            let mut count = 0;
            unsafe {
                list.Count(&mut count)?;
            }
            let mut cookies = Vec::with_capacity(count as usize);
            for index in 0..count {
                let cookie = unsafe { list.GetValueAtIndex(index)? };
                let mut name = PWSTR::null();
                let mut domain = PWSTR::null();
                let mut path = PWSTR::null();
                let mut secure = BOOL(0);
                let mut http_only = BOOL(0);
                let mut session = BOOL(0);
                unsafe {
                    cookie.Name(&mut name)?;
                    cookie.Domain(&mut domain)?;
                    cookie.Path(&mut path)?;
                    cookie.IsSecure(&mut secure)?;
                    cookie.IsHttpOnly(&mut http_only)?;
                    cookie.IsSession(&mut session)?;
                }
                cookies.push(CookieInfo {
                    name: take_pwstr(name),
                    domain: take_pwstr(domain),
                    path: take_pwstr(path),
                    secure: secure.as_bool(),
                    http_only: http_only.as_bool(),
                    session: session.as_bool(),
                });
            }
            Ok(cookies)
        })();
        done(result.map_err(|error| error.to_string()));
        Ok(())
    }));
    unsafe { manager.GetCookies(&uri, &handler) }.map_err(|error| error.to_string())
}

#[cfg(windows)]
pub fn delete_cookie(
    view: &wry::WebView,
    uri: &str,
    name: &str,
    domain: &str,
    path: &str,
    done: impl FnOnce(Result<(), String>) + 'static,
) -> Result<(), String> {
    use webview2_com::{
        GetCookiesCompletedHandler, Microsoft::Web::WebView2::Win32::*, take_pwstr,
    };
    use windows::core::{HSTRING, Interface, PWSTR};
    use wry::WebViewExtWindows;
    if name.is_empty() || name.chars().any(char::is_control) || domain.is_empty() || path.is_empty()
    {
        return Err("Недопустимый идентификатор cookie".into());
    }
    let manager = (|| unsafe {
        view.controller()
            .CoreWebView2()?
            .cast::<ICoreWebView2_13>()?
            .Profile()?
            .cast::<ICoreWebView2Profile5>()?
            .CookieManager()
    })()
    .map_err(|error: windows::core::Error| error.to_string())?;
    let uri = HSTRING::from(uri);
    let wanted = (name.to_owned(), domain.to_owned(), path.to_owned());
    let callback_manager = manager.clone();
    let handler = GetCookiesCompletedHandler::create(Box::new(move |hr, list| {
        let result = (|| -> windows::core::Result<()> {
            hr?;
            let list = list.ok_or_else(windows::core::Error::from_thread)?;
            let mut count = 0;
            unsafe {
                list.Count(&mut count)?;
            }
            for index in 0..count {
                let cookie = unsafe { list.GetValueAtIndex(index)? };
                let mut actual_name = PWSTR::null();
                let mut actual_domain = PWSTR::null();
                let mut actual_path = PWSTR::null();
                unsafe {
                    cookie.Name(&mut actual_name)?;
                    cookie.Domain(&mut actual_domain)?;
                    cookie.Path(&mut actual_path)?;
                }
                let actual = (
                    take_pwstr(actual_name),
                    take_pwstr(actual_domain),
                    take_pwstr(actual_path),
                );
                if actual == wanted {
                    return unsafe { callback_manager.DeleteCookie(&cookie) };
                }
            }
            Err(windows::core::Error::new(
                windows::core::HRESULT(0x8007_0490u32 as i32),
                "Cookie not found for this URL",
            ))
        })();
        done(result.map_err(|error| error.to_string()));
        Ok(())
    }));
    unsafe { manager.GetCookies(&uri, &handler) }.map_err(|error| error.to_string())
}

#[cfg(windows)]
pub fn clear_origin_storage(
    view: &wry::WebView,
    origin: &str,
    done: impl FnOnce(Result<(), String>) + 'static,
) -> Result<(), String> {
    use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
    use windows::core::HSTRING;
    use wry::WebViewExtWindows;
    if !is_exact_web_origin(origin) {
        return Err("Ожидается точный HTTP(S) origin".into());
    }
    let core = unsafe { view.controller().CoreWebView2() }.map_err(|error| error.to_string())?;
    let params = HSTRING::from(
        serde_json::json!({
            "origin": origin,
            "storageTypes": "all"
        })
        .to_string(),
    );
    let method = HSTRING::from("Storage.clearDataForOrigin");
    let handler =
        CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |hr, response| {
            let result = hr.map_err(|error| error.to_string()).and_then(|_| {
                let parsed: serde_json::Value =
                    serde_json::from_str(&response).map_err(|error| error.to_string())?;
                if parsed.get("error").is_some() {
                    Err("WebView2 отклонил очистку origin".into())
                } else {
                    Ok(())
                }
            });
            done(result);
            Ok(())
        }));
    unsafe { core.CallDevToolsProtocolMethod(&method, &params, &handler) }
        .map_err(|error| error.to_string())
}

#[cfg(windows)]
pub fn clear_profile_cache(
    view: &wry::WebView,
    done: impl FnOnce(Result<(), String>) + 'static,
) -> Result<(), String> {
    use webview2_com::{ClearBrowsingDataCompletedHandler, Microsoft::Web::WebView2::Win32::*};
    use windows::core::Interface;
    use wry::WebViewExtWindows;
    let profile = (|| unsafe {
        view.controller()
            .CoreWebView2()?
            .cast::<ICoreWebView2_13>()?
            .Profile()?
            .cast::<ICoreWebView2Profile2>()
    })()
    .map_err(|error: windows::core::Error| error.to_string())?;
    let handler = ClearBrowsingDataCompletedHandler::create(Box::new(move |hr| {
        done(hr.map_err(|error| error.to_string()));
        Ok(())
    }));
    unsafe { profile.ClearBrowsingData(COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE, &handler) }
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{cookie_domain_matches_host, is_exact_web_origin};

    #[test]
    fn origin_cleanup_requires_exact_http_origin() {
        assert!(is_exact_web_origin("https://example.com"));
        assert!(is_exact_web_origin("http://localhost:8080"));
        assert!(!is_exact_web_origin("https://example.com/path"));
        assert!(!is_exact_web_origin("file:///tmp/page"));
        assert!(!is_exact_web_origin("https://example.com/"));
    }

    #[test]
    fn cookie_removal_is_limited_to_current_host_or_parent_domain() {
        assert!(cookie_domain_matches_host(
            ".example.com",
            "shop.example.com"
        ));
        assert!(cookie_domain_matches_host(
            "shop.example.com",
            "shop.example.com"
        ));
        assert!(!cookie_domain_matches_host("ample.com", "shop.example.com"));
        assert!(!cookie_domain_matches_host("", "shop.example.com"));
    }
}
