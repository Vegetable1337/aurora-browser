//! Origin-scoped Aurora permission policy. WebView2 never persists our decisions.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    #[default]
    Ask,
    Allow,
    Deny,
}

pub const KINDS: [&str; 4] = ["camera", "microphone", "geolocation", "notifications"];
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Policy(pub BTreeMap<String, BTreeMap<String, Decision>>);
pub type SharedPolicy = Arc<RwLock<Policy>>;

pub fn origin(address: &str) -> Option<String> {
    let url = url::Url::parse(address).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| url.origin().ascii_serialization())
}
impl Policy {
    pub fn decision(&self, address: &str, kind: &str) -> Decision {
        origin(address)
            .and_then(|o| self.0.get(&o))
            .and_then(|m| m.get(kind))
            .copied()
            .unwrap_or_default()
    }
    pub fn set(&mut self, address: &str, kind: &str, decision: Decision) -> Result<(), String> {
        if !KINDS.contains(&kind) {
            return Err("Неподдерживаемое разрешение".into());
        }
        let key = origin(address).ok_or("Разрешения доступны только для HTTP(S) origin")?;
        self.0.entry(key).or_default().insert(kind.into(), decision);
        Ok(())
    }
    pub fn reset(&mut self, address: &str) {
        if let Some(key) = origin(address) {
            self.0.remove(&key);
        }
    }
}

#[cfg(windows)]
pub fn attach(
    view: &wry::WebView,
    policy: SharedPolicy,
) -> Result<Arc<std::sync::atomic::AtomicU64>, String> {
    use webview2_com::{
        Microsoft::Web::WebView2::Win32::*, PermissionRequestedEventHandler, take_pwstr,
    };
    use windows::core::Interface;
    use wry::WebViewExtWindows;
    let core = unsafe { view.controller().CoreWebView2() }.map_err(|e| e.to_string())?;
    let requests = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let observed = requests.clone();
    let handler = PermissionRequestedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else {
            return Ok(());
        };
        unsafe {
            // Deny first: unsupported runtimes and policy failures must not grant access.
            args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)?;
            args.cast::<ICoreWebView2PermissionRequestedEventArgs3>()?
                .SetSavesInProfile(false)?;
            let mut uri = windows::core::PWSTR::null();
            args.Uri(&mut uri)?;
            let uri = take_pwstr(uri);
            let Some(origin) = origin(&uri) else {
                return Ok(());
            };
            let mut kind = COREWEBVIEW2_PERMISSION_KIND_UNKNOWN_PERMISSION;
            args.PermissionKind(&mut kind)?;
            let (key, label) = match kind {
                COREWEBVIEW2_PERMISSION_KIND_CAMERA => ("camera", "камера"),
                COREWEBVIEW2_PERMISSION_KIND_MICROPHONE => ("microphone", "микрофон"),
                COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION => ("geolocation", "геолокация"),
                COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS => ("notifications", "уведомления"),
                _ => return Ok(()),
            };
            observed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let decision = policy
                .read()
                .map(|p| p.decision(&origin, key))
                .unwrap_or(Decision::Deny);
            let allow = match decision {
                Decision::Allow => true,
                Decision::Deny => false,
                Decision::Ask => matches!(
                    rfd::MessageDialog::new()
                        .set_title("Разрешение сайта — Aurora")
                        .set_description(format!(
                            "{origin}\n\nРазрешить: {label}?\nРешение действует для этого запроса."
                        ))
                        .set_buttons(rfd::MessageButtons::YesNo)
                        .show(),
                    rfd::MessageDialogResult::Yes
                ),
            };
            if allow {
                args.SetState(COREWEBVIEW2_PERMISSION_STATE_ALLOW)?;
            }
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { core.add_PermissionRequested(&handler, &mut token) }
        .map(|_| requests)
        .map_err(|e| e.to_string())
}

/// Native settings are returned to the application for encrypted migration
/// before the engine's plaintext decisions are cleared.
#[derive(Clone, Serialize, Deserialize)]
pub struct LegacyPermission {
    pub kind: i32,
    pub origin: String,
    pub state: i32,
}
impl Policy {
    pub fn import_legacy(&mut self, settings: &[LegacyPermission]) {
        for setting in settings {
            let kind = match setting.kind {
                1 => "microphone",
                2 => "camera",
                3 => "geolocation",
                4 => "notifications",
                _ => continue,
            };
            let Some(key) = origin(&setting.origin) else {
                continue;
            };
            let decision = match setting.state {
                1 => Decision::Allow,
                2 => Decision::Deny,
                _ => Decision::Ask,
            };
            self.0
                .entry(key)
                .or_default()
                .entry(kind.into())
                .or_insert(decision);
        }
    }
}
#[cfg(windows)]
pub fn read_legacy(
    view: &wry::WebView,
    done: impl FnOnce(Result<Vec<LegacyPermission>, String>) + 'static,
) -> Result<(), String> {
    use webview2_com::{
        GetNonDefaultPermissionSettingsCompletedHandler, Microsoft::Web::WebView2::Win32::*,
        take_pwstr,
    };
    use windows::core::Interface;
    use wry::WebViewExtWindows;
    let profile = (|| unsafe {
        view.controller()
            .CoreWebView2()?
            .cast::<ICoreWebView2_13>()?
            .Profile()?
            .cast::<ICoreWebView2Profile4>()
    })()
    .map_err(|e: windows::core::Error| e.to_string())?;
    let handler =
        GetNonDefaultPermissionSettingsCompletedHandler::create(Box::new(move |hr, list| {
            let result = (|| -> windows::core::Result<Vec<LegacyPermission>> {
                hr?;
                let list = list.ok_or_else(windows::core::Error::from_thread)?;
                let mut count = 0;
                unsafe {
                    list.Count(&mut count)?;
                }
                let mut settings = Vec::new();
                for i in 0..count {
                    unsafe {
                        let item = list.GetValueAtIndex(i)?;
                        let mut kind = COREWEBVIEW2_PERMISSION_KIND_UNKNOWN_PERMISSION;
                        item.PermissionKind(&mut kind)?;
                        let mut state = COREWEBVIEW2_PERMISSION_STATE_DEFAULT;
                        item.PermissionState(&mut state)?;
                        let mut uri = windows::core::PWSTR::null();
                        item.PermissionOrigin(&mut uri)?;
                        settings.push(LegacyPermission {
                            kind: kind.0,
                            origin: take_pwstr(uri),
                            state: state.0,
                        });
                    }
                }
                Ok(settings)
            })();
            done(result.map_err(|e| e.to_string()));
            Ok(())
        }));
    unsafe { profile.GetNonDefaultPermissionSettings(&handler) }.map_err(|e| e.to_string())
}
#[cfg(windows)]
pub fn reset_legacy(
    view: &wry::WebView,
    settings: Vec<LegacyPermission>,
    done: impl FnOnce(Result<(), String>) + 'static,
) -> Result<(), String> {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    use windows::core::Interface;
    use wry::WebViewExtWindows;
    let profile = (|| unsafe {
        view.controller()
            .CoreWebView2()?
            .cast::<ICoreWebView2_13>()?
            .Profile()?
            .cast::<ICoreWebView2Profile4>()
    })()
    .map_err(|e: windows::core::Error| e.to_string())?;
    reset_next(
        profile,
        settings
            .into_iter()
            .map(|s| (COREWEBVIEW2_PERMISSION_KIND(s.kind), s.origin))
            .collect(),
        Box::new(done),
    );
    Ok(())
}
#[cfg(windows)]
fn reset_next(
    profile: webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Profile4,
    mut pending: Vec<(
        webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_PERMISSION_KIND,
        String,
    )>,
    done: Box<dyn FnOnce(Result<(), String>)>,
) {
    use std::{cell::RefCell, rc::Rc};
    use webview2_com::{Microsoft::Web::WebView2::Win32::*, SetPermissionStateCompletedHandler};
    let Some((kind, origin)) = pending.pop() else {
        done(Ok(()));
        return;
    };
    let slot = Rc::new(RefCell::new(Some(done)));
    let on_error = slot.clone();
    let copy = profile.clone();
    let handler = SetPermissionStateCompletedHandler::create(Box::new(move |hr| {
        let done = slot.borrow_mut().take();
        if let Some(done) = done {
            match hr {
                Ok(()) => reset_next(copy, pending, done),
                Err(e) => done(Err(e.to_string())),
            }
        }
        Ok(())
    }));
    if let Err(e) = unsafe {
        profile.SetPermissionState(
            kind,
            &windows::core::HSTRING::from(origin),
            COREWEBVIEW2_PERMISSION_STATE_DEFAULT,
            &handler,
        )
    } {
        let done = on_error.borrow_mut().take();
        if let Some(done) = done {
            done(Err(e.to_string()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_migration_preserves_explicit_choices() {
        let mut policy = Policy::default();
        policy
            .set("https://example.test", "camera", Decision::Ask)
            .unwrap();
        let settings = vec![
            LegacyPermission {
                kind: 2,
                origin: "https://example.test".into(),
                state: 1,
            },
            LegacyPermission {
                kind: 3,
                origin: "https://example.test".into(),
                state: 2,
            },
        ];
        policy.import_legacy(&settings);
        assert_eq!(
            policy.decision("https://example.test", "camera"),
            Decision::Ask
        );
        assert_eq!(
            policy.decision("https://example.test", "geolocation"),
            Decision::Deny
        );
        #[cfg(windows)]
        {
            use webview2_com::Microsoft::Web::WebView2::Win32::*;
            assert_eq!(COREWEBVIEW2_PERMISSION_KIND_CAMERA.0, 2);
            assert_eq!(COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION.0, 3);
        }
    }
    #[test]
    fn protected_policy_survives_reopen_and_lock() {
        use crate::vault::ProfileVault;
        let root =
            std::env::temp_dir().join(format!("aurora-permission-test-{}", uuid::Uuid::new_v4()));
        let mut first = ProfileVault::create(&root, "A", "test-only-password-123")
            .unwrap()
            .vault;
        let second = ProfileVault::create(&root, "B", "test-only-password-456")
            .unwrap()
            .vault;
        let mut policy = Policy::default();
        policy
            .set("https://example.test:8443", "camera", Decision::Deny)
            .unwrap();
        first
            .put(
                "site-permissions",
                "policy",
                &serde_json::to_vec(&policy).unwrap(),
            )
            .unwrap();
        assert!(second.list("site-permissions").unwrap().is_empty());
        let directory = first.directory().to_owned();
        first.lock();
        assert!(first.list("site-permissions").is_err());
        let reopened = ProfileVault::unlock_password(&directory, "test-only-password-123").unwrap();
        let records = reopened.list("site-permissions").unwrap();
        let restored: Policy = serde_json::from_slice(&records[0].1).unwrap();
        assert_eq!(
            restored.decision("https://example.test:8443/path", "camera"),
            Decision::Deny
        );
        assert_eq!(
            restored.decision("https://example.test", "camera"),
            Decision::Ask
        );
        drop(reopened);
        drop(first);
        drop(second);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn policy_is_bound_to_scheme_host_and_port() {
        let mut p = Policy::default();
        p.set("https://EXAMPLE.com:443/a", "camera", Decision::Allow)
            .unwrap();
        assert_eq!(
            p.decision("https://example.com/b", "camera"),
            Decision::Allow
        );
        for other in [
            "http://example.com",
            "https://example.com:444",
            "https://sub.example.com",
        ] {
            assert_eq!(p.decision(other, "camera"), Decision::Ask);
        }
        assert!(
            p.set("data:text/html,test", "camera", Decision::Allow)
                .is_err()
        );
        assert!(
            p.set("https://example.com", "unknown", Decision::Allow)
                .is_err()
        );
        let independent = Policy::default();
        assert_eq!(
            independent.decision("https://example.com", "camera"),
            Decision::Ask
        );
        p.reset("https://example.com/a");
        assert_eq!(p.decision("https://example.com", "camera"), Decision::Ask);
    }
}
