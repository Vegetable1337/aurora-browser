//! HTTPS-first address handling and WebView2 failed-navigation observation.

pub fn is_local_http_url(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    if url.scheme() != "http" {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        return true;
    }
    match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Some(url::Host::Ipv6(ip)) => {
            ip.is_loopback() || (ip.segments()[0] & 0xfe00) == 0xfc00 || ip.is_unicast_link_local()
        }
        _ => false,
    }
}

pub fn fallback_url(https_url: &str) -> Option<String> {
    let mut url = url::Url::parse(https_url).ok()?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return None;
    }
    url.set_scheme("http").ok()?;
    let candidate = url.to_string();
    (!is_local_http_url(&candidate)).then_some(candidate)
}

pub fn should_offer_fallback(status: i32) -> bool {
    // Connection/name resolution/time-out failures only. Certificate and policy errors are excluded.
    matches!(status, 6 | 7 | 9 | 10 | 11 | 12 | 13)
}

#[cfg(windows)]
pub fn observe_failed_navigation(
    view: &wry::WebView,
    mut callback: impl FnMut(String, bool, i32) + 'static,
) -> Result<(), String> {
    use webview2_com::{Microsoft::Web::WebView2::Win32::*, NavigationCompletedEventHandler};
    use windows::core::{BOOL, PWSTR};
    use wry::WebViewExtWindows;
    let core = unsafe { view.controller().CoreWebView2() }.map_err(|error| error.to_string())?;
    let handler = NavigationCompletedEventHandler::create(Box::new(move |sender, args| {
        let result = (|| -> windows::core::Result<(String, bool, i32)> {
            let sender = sender.ok_or_else(windows::core::Error::from_thread)?;
            let args = args.ok_or_else(windows::core::Error::from_thread)?;
            let mut source = PWSTR::null();
            let mut succeeded = BOOL(0);
            let mut status = COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN;
            unsafe {
                sender.Source(&mut source)?;
                args.IsSuccess(&mut succeeded)?;
                args.WebErrorStatus(&mut status)?;
            }
            Ok((
                webview2_com::take_pwstr(source),
                succeeded.as_bool(),
                status.0,
            ))
        })();
        if let Ok((source, succeeded, status)) = result {
            callback(source, succeeded, status);
        }
        Ok(())
    }));
    unsafe { core.add_NavigationCompleted(&handler, &mut 0) }.map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{fallback_url, is_local_http_url, should_offer_fallback};

    #[test]
    fn local_targets_are_exempted_from_http_warning() {
        for url in [
            "http://localhost:3000/",
            "http://127.0.0.1/",
            "http://192.168.1.10/",
            "http://printer.local/",
        ] {
            assert!(is_local_http_url(url), "{url}");
        }
        assert!(!is_local_http_url("http://example.com/"));
        assert!(!is_local_http_url("https://localhost/"));
    }

    #[test]
    fn fallback_preserves_site_path_and_network_errors_only() {
        assert_eq!(
            fallback_url("https://example.com/a?q=1#part").as_deref(),
            Some("http://example.com/a?q=1#part")
        );
        assert!(fallback_url("http://example.com").is_none());
        assert!(should_offer_fallback(12));
        assert!(!should_offer_fallback(1));
        assert!(!should_offer_fallback(5));
    }
}
