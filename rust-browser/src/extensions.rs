//! Profile-scoped WebView2 extension management.
//!
//! Only unpacked Manifest V3 packages are currently accepted. The browser stores
//! a private copy under the active WebView2 user-data directory and never points
//! WebView2 at the user's mutable source folder.

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

const MAX_FILES: usize = 20_000;
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExtensionManifest {
    pub manifest_version: u8,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub host_permissions: Vec<String>,
    #[serde(default)]
    pub optional_permissions: Vec<String>,
    #[serde(default)]
    pub optional_host_permissions: Vec<String>,
    #[serde(default)]
    pub icons: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExtensionInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub permissions: Vec<String>,
    pub host_permissions: Vec<String>,
    pub icon_data_url: Option<String>,
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ManagedExtension {
    id: String,
    directory: String,
    manifest: ExtensionManifest,
}

#[derive(Debug, thiserror::Error)]
pub enum ExtensionError {
    #[error("Не удалось прочитать расширение: {0}")]
    Io(#[from] std::io::Error),
    #[error("Некорректный manifest.json: {0}")]
    Manifest(#[from] serde_json::Error),
    #[error("Поддерживаются только распакованные расширения Manifest V3")]
    UnsupportedManifest,
    #[error("Пакет расширения содержит небезопасный путь или ссылку")]
    UnsafePath,
    #[error("Пакет расширения превышает допустимый размер или число файлов")]
    PackageTooLarge,
    #[error("Имя, версия или описание расширения некорректны")]
    InvalidMetadata,
}

pub fn read_manifest(source: &Path) -> Result<ExtensionManifest, ExtensionError> {
    let manifest: ExtensionManifest =
        serde_json::from_slice(&fs::read(source.join("manifest.json"))?)?;
    if manifest.manifest_version != 3 {
        return Err(ExtensionError::UnsupportedManifest);
    }
    if manifest.name.trim().is_empty()
        || manifest.name.len() > 256
        || manifest.version.trim().is_empty()
        || manifest.version.len() > 128
        || manifest.description.len() > 4096
        || manifest.permissions.len()
            + manifest.host_permissions.len()
            + manifest.optional_permissions.len()
            + manifest.optional_host_permissions.len()
            > 512
        || manifest
            .permissions
            .iter()
            .chain(&manifest.host_permissions)
            .chain(&manifest.optional_permissions)
            .chain(&manifest.optional_host_permissions)
            .any(|permission| permission.len() > 512)
    {
        return Err(ExtensionError::InvalidMetadata);
    }
    Ok(manifest)
}

/// Copy an extension into a newly created, profile-owned directory. Symlinks,
/// reparse points, path traversal and oversized packages are rejected.
pub fn stage_package(source: &Path, managed_root: &Path) -> Result<PathBuf, ExtensionError> {
    let source = source.canonicalize()?;
    if !source.is_dir() {
        return Err(ExtensionError::UnsafePath);
    }
    let manifest = read_manifest(&source)?;
    fs::create_dir_all(managed_root)?;
    let managed_root = managed_root.canonicalize()?;
    let target = managed_root.join(uuid::Uuid::new_v4().to_string());
    fs::create_dir(&target)?;
    let result = copy_tree(&source, &target);
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&target);
        return Err(error);
    }
    // Parse the staged copy too, so permissions reviewed later are from the
    // exact bytes sent to WebView2, not a mutable source directory.
    let finalize = (|| {
        read_manifest(&target)?;
        let metadata = serde_json::to_vec(&manifest)?;
        fs::write(target.join(".aurora-manifest.json"), metadata)?;
        if !target.starts_with(&managed_root) {
            return Err(ExtensionError::UnsafePath);
        }
        Ok(())
    })();
    if let Err(error) = finalize {
        let _ = fs::remove_dir_all(&target);
        return Err(error);
    }
    Ok(target)
}

fn copy_tree(source: &Path, target: &Path) -> Result<(), ExtensionError> {
    let mut pending = vec![(source.to_path_buf(), target.to_path_buf())];
    let mut file_count = 0usize;
    let mut total_bytes = 0u64;
    while let Some((source_dir, target_dir)) = pending.pop() {
        for entry in fs::read_dir(source_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let relative = Path::new(&name);
            if relative
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
            {
                return Err(ExtensionError::UnsafePath);
            }
            let metadata = fs::symlink_metadata(entry.path())?;
            if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
                return Err(ExtensionError::UnsafePath);
            }
            let destination = target_dir.join(name);
            if metadata.is_dir() {
                fs::create_dir(&destination)?;
                pending.push((entry.path(), destination));
            } else if metadata.is_file() {
                file_count += 1;
                total_bytes = total_bytes.saturating_add(metadata.len());
                if file_count > MAX_FILES || total_bytes > MAX_TOTAL_BYTES {
                    return Err(ExtensionError::PackageTooLarge);
                }
                fs::copy(entry.path(), destination)?;
            } else {
                return Err(ExtensionError::UnsafePath);
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_: &fs::Metadata) -> bool {
    false
}

#[cfg(windows)]
mod webview2 {
    use super::{ExtensionInfo, ExtensionManifest, ManagedExtension};
    use std::{
        fs,
        path::{Component, Path, PathBuf},
        sync::{Arc, Mutex},
    };
    use webview2_com::{
        BrowserExtensionEnableCompletedHandler, BrowserExtensionRemoveCompletedHandler,
        Microsoft::Web::WebView2::Win32::*, ProfileAddBrowserExtensionCompletedHandler,
        ProfileGetBrowserExtensionsCompletedHandler,
    };
    use windows::core::{HSTRING, Interface, PWSTR};
    use wry::{WebView, WebViewExtWindows};

    pub fn install(
        view: &WebView,
        directory: PathBuf,
        registry: PathBuf,
        manifest: ExtensionManifest,
        done: impl FnOnce(Result<ExtensionInfo, String>) + 'static,
    ) -> Result<(), String> {
        let profile = profile(view)?;
        let path = HSTRING::from(directory.as_path());
        let handler =
            ProfileAddBrowserExtensionCompletedHandler::create(Box::new(move |hr, extension| {
                let mut installed = None;
                let mut installed_id = None;
                let result = hr
                    .and_then(|_| extension.ok_or_else(windows::core::Error::from_thread))
                    .and_then(|extension| {
                        installed = Some(extension.clone());
                        let id = extension_string(&extension, |extension, value| unsafe {
                            extension.Id(value)
                        })?;
                        installed_id = Some(id.clone());
                        let name = extension_string(&extension, |extension, value| unsafe {
                            extension.Name(value)
                        })?;
                        let record = ManagedExtension {
                            id: id.clone(),
                            directory: directory.to_string_lossy().into_owned(),
                            manifest: manifest.clone(),
                        };
                        upsert_registry(&registry, record).map_err(|error| {
                            windows::core::Error::new(
                                windows::core::HRESULT(0x80004005u32 as i32),
                                error.to_string(),
                            )
                        })?;
                        let mut enabled = windows::core::BOOL(0);
                        unsafe {
                            extension.IsEnabled(&mut enabled)?;
                        }
                        Ok(ExtensionInfo {
                            id,
                            name,
                            version: manifest.version.clone(),
                            description: manifest.description.clone(),
                            permissions: manifest.permissions.clone(),
                            host_permissions: manifest.host_permissions.clone(),
                            icon_data_url: extension_icon(&directory, &manifest),
                            enabled: enabled.as_bool(),
                        })
                    })
                    .map_err(|error| error.to_string());
                if result.is_err() {
                    if let Some(id) = installed_id {
                        if let Ok(mut records) = read_registry(&registry) {
                            records.retain(|record| record.id != id);
                            let _ = write_registry(&registry, &records);
                        }
                    }
                    if let Some(extension) = installed {
                        let cleanup_directory = directory.clone();
                        let cleanup = BrowserExtensionRemoveCompletedHandler::create(Box::new(
                            move |remove_result| {
                                if remove_result.is_ok() {
                                    let _ = fs::remove_dir_all(cleanup_directory);
                                }
                                Ok(())
                            },
                        ));
                        if let Err(error) = unsafe { extension.Remove(&cleanup) } {
                            eprintln!(
                                "Aurora could not roll back a failed extension install: {error}"
                            );
                        }
                    } else {
                        let _ = fs::remove_dir_all(&directory);
                    }
                }
                done(result);
                Ok(())
            }));
        unsafe { profile.AddBrowserExtension(&path, &handler) }.map_err(|e| e.to_string())
    }

    pub fn list(
        view: &WebView,
        registry: PathBuf,
        done: impl FnOnce(Result<Vec<ExtensionInfo>, String>) + 'static,
    ) -> Result<(), String> {
        let profile = profile(view)?;
        let handler =
            ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |hr, extensions| {
                let result = (|| -> windows::core::Result<Vec<ExtensionInfo>> {
                    hr?;
                    let extensions = extensions.ok_or_else(windows::core::Error::from_thread)?;
                    let managed = read_registry(&registry).map_err(|error| {
                        windows::core::Error::new(
                            windows::core::HRESULT(0x80004005u32 as i32),
                            error.to_string(),
                        )
                    })?;
                    let mut count = 0;
                    unsafe {
                        extensions.Count(&mut count)?;
                    }
                    let mut results = Vec::new();
                    for index in 0..count {
                        let extension = unsafe { extensions.GetValueAtIndex(index)? };
                        let id = extension_string(&extension, |extension, value| unsafe {
                            extension.Id(value)
                        })?;
                        if let Some(record) = managed.iter().find(|record| record.id == id) {
                            let name = extension_string(&extension, |extension, value| unsafe {
                                extension.Name(value)
                            })?;
                            let mut enabled = windows::core::BOOL(0);
                            unsafe {
                                extension.IsEnabled(&mut enabled)?;
                            }
                            results.push(ExtensionInfo {
                                id,
                                name,
                                version: record.manifest.version.clone(),
                                description: record.manifest.description.clone(),
                                permissions: record.manifest.permissions.clone(),
                                host_permissions: record.manifest.host_permissions.clone(),
                                icon_data_url: extension_icon(
                                    Path::new(&record.directory),
                                    &record.manifest,
                                ),
                                enabled: enabled.as_bool(),
                            });
                        }
                    }
                    Ok(results)
                })()
                .map_err(|error| error.to_string());
                done(result);
                Ok(())
            }));
        unsafe { profile.GetBrowserExtensions(&handler) }.map_err(|e| e.to_string())
    }

    pub fn set_enabled(
        view: &WebView,
        registry: PathBuf,
        id: String,
        enabled: bool,
        done: impl FnOnce(Result<(), String>) + 'static,
    ) -> Result<(), String> {
        let completion = shared_done(done);
        let callback_completion = completion.clone();
        for_extension(
            view,
            registry,
            id,
            move |extension| {
                let callback_completion = callback_completion.clone();
                let invocation_completion = callback_completion.clone();
                let handler = BrowserExtensionEnableCompletedHandler::create(Box::new(move |hr| {
                    finish(&callback_completion, hr.map_err(|error| error.to_string()));
                    Ok(())
                }));
                if let Err(error) = unsafe { extension.Enable(enabled, &handler) } {
                    finish(&invocation_completion, Err(error.to_string()));
                }
            },
            completion,
        )
    }

    pub fn remove(
        view: &WebView,
        registry: PathBuf,
        id: String,
        done: impl FnOnce(Result<(), String>) + 'static,
    ) -> Result<(), String> {
        let completion = shared_done(done);
        let callback_completion = completion.clone();
        for_extension(
            view,
            registry.clone(),
            id.clone(),
            move |extension| {
                let callback_completion = callback_completion.clone();
                let invocation_completion = callback_completion.clone();
                let handler = BrowserExtensionRemoveCompletedHandler::create(Box::new(move |hr| {
                    let result = hr.map_err(|error| error.to_string()).and_then(|_| -> Result<(), String> {
                    let mut items = read_registry(&registry).map_err(|e| e.to_string())?;
                    if let Some(index) = items.iter().position(|item| item.id == id) {
                        let item = items.remove(index);
                        write_registry(&registry, &items).map_err(|e| e.to_string())?;
                        let directory = PathBuf::from(item.directory);
                        let managed_root = registry.parent().ok_or("extension registry has no parent")?.join("Extensions");
                        let canonical_root = managed_root.canonicalize().map_err(|e| e.to_string())?;
                        let canonical_directory = directory.canonicalize().map_err(|e| e.to_string())?;
                        if canonical_directory.parent() != Some(canonical_root.as_path()) {
                            return Err("refusing to remove extension outside managed profile storage".into());
                        }
                        fs::remove_dir_all(canonical_directory).map_err(|e| e.to_string())?;
                    }
                    Ok(())
                });
                    finish(&callback_completion, result);
                    Ok(())
                }));
                if let Err(error) = unsafe { extension.Remove(&handler) } {
                    finish(&invocation_completion, Err(error.to_string()));
                }
            },
            completion,
        )
    }

    fn for_extension(
        view: &WebView,
        registry: PathBuf,
        id: String,
        action: impl FnOnce(ICoreWebView2BrowserExtension) + 'static,
        done: SharedDone,
    ) -> Result<(), String> {
        let profile = profile(view)?;
        let handler =
            ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |hr, extensions| {
                let result = (|| -> Result<(), String> {
                    hr.map_err(|e| e.to_string())?;
                    let extensions = extensions
                        .ok_or_else(|| "WebView2 returned no extension list".to_owned())?;
                    let managed = read_registry(&registry).map_err(|e| e.to_string())?;
                    if !managed.iter().any(|item| item.id == id) {
                        return Err("extension ID is not registered as Aurora-managed".into());
                    }
                    let mut count = 0;
                    unsafe {
                        extensions.Count(&mut count).map_err(|e| e.to_string())?;
                    }
                    for index in 0..count {
                        let extension = unsafe {
                            extensions
                                .GetValueAtIndex(index)
                                .map_err(|e| e.to_string())?
                        };
                        let candidate = extension_string(&extension, |extension, value| unsafe {
                            extension.Id(value)
                        })
                        .map_err(|e| e.to_string())?;
                        if candidate == id {
                            action(extension);
                            return Ok(());
                        }
                    }
                    Err("extension ID not found in active WebView2 profile".into())
                })();
                if let Err(error) = result {
                    finish(&done, Err(error));
                }
                Ok(())
            }));
        unsafe { profile.GetBrowserExtensions(&handler) }.map_err(|e| e.to_string())
    }

    type SharedDone = Arc<Mutex<Option<Box<dyn FnOnce(Result<(), String>) + 'static>>>>;

    fn shared_done(done: impl FnOnce(Result<(), String>) + 'static) -> SharedDone {
        Arc::new(Mutex::new(Some(Box::new(done))))
    }

    fn finish(done: &SharedDone, result: Result<(), String>) {
        if let Ok(mut slot) = done.lock() {
            if let Some(done) = slot.take() {
                done(result);
            }
        }
    }

    fn profile(view: &WebView) -> Result<ICoreWebView2Profile7, String> {
        unsafe {
            view.controller()
                .CoreWebView2()
                .and_then(|core| core.cast::<ICoreWebView2_13>())
                .and_then(|core| core.Profile())
                .and_then(|profile| profile.cast::<ICoreWebView2Profile7>())
                .map_err(|error| error.to_string())
        }
    }

    fn extension_string(
        extension: &ICoreWebView2BrowserExtension,
        getter: impl FnOnce(&ICoreWebView2BrowserExtension, *mut PWSTR) -> windows::core::Result<()>,
    ) -> windows::core::Result<String> {
        let mut value = PWSTR::null();
        getter(extension, &mut value)?;
        Ok(webview2_com::take_pwstr(value))
    }

    fn extension_icon(directory: &Path, manifest: &ExtensionManifest) -> Option<String> {
        use base64::Engine;
        let icon_path = manifest
            .icons
            .iter()
            .filter_map(|(size, path)| Some((size.parse::<u32>().ok()?, path)))
            .max_by_key(|(size, _)| *size)
            .map(|(_, path)| path)?;
        let relative = Path::new(icon_path);
        if relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            return None;
        }
        let path = directory.join(relative);
        let metadata = fs::symlink_metadata(&path).ok()?;
        if metadata.file_type().is_symlink()
            || super::is_reparse_point(&metadata)
            || metadata.len() > 512 * 1024
        {
            return None;
        }
        let bytes = fs::read(path).ok()?;
        let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            "image/png"
        } else if bytes.starts_with(b"\xff\xd8\xff") {
            "image/jpeg"
        } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            "image/gif"
        } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            "image/webp"
        } else {
            return None;
        };
        Some(format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ))
    }

    fn read_registry(path: &Path) -> Result<Vec<ManagedExtension>, Box<dyn std::error::Error>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    }

    fn upsert_registry(
        path: &Path,
        item: ManagedExtension,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut items = read_registry(path)?;
        items.retain(|old| old.id != item.id);
        items.push(item);
        write_registry(path, &items)
    }

    fn write_registry(
        path: &Path,
        items: &[ManagedExtension],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let parent = path.parent().ok_or("registry has no parent")?;
        fs::create_dir_all(parent)?;
        let temporary = parent.join(format!(".aurora-extensions-{}.tmp", uuid::Uuid::new_v4()));
        let bytes = serde_json::to_vec_pretty(items)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        use std::io::Write;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        let replace = (|| -> Result<(), Box<dyn std::error::Error>> {
            use windows::Win32::Storage::FileSystem::{
                MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
            };
            let source = HSTRING::from(temporary.as_path());
            let destination = HSTRING::from(path);
            unsafe {
                MoveFileExW(
                    &source,
                    &destination,
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )?;
            }
            Ok(())
        })();
        if let Err(error) = replace {
            let _ = fs::remove_file(temporary);
            return Err(error);
        }
        Ok(())
    }

    pub use install as install_extension;
    pub use list as list_extensions;
    pub use remove as remove_extension;
    pub use set_enabled as set_extension_enabled;
}

#[cfg(windows)]
pub use webview2::{install_extension, list_extensions, remove_extension, set_extension_enabled};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_mv3_extension() {
        let temp = test_directory();
        fs::write(
            temp.join("manifest.json"),
            r#"{"manifest_version":2,"name":"x","version":"1"}"#,
        )
        .unwrap();
        assert!(matches!(
            read_manifest(&temp),
            Err(ExtensionError::UnsupportedManifest)
        ));
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn stages_copy_and_does_not_follow_mutable_source() {
        let temp = test_directory();
        let source = temp.join("source");
        let managed = temp.join("managed");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("manifest.json"),
            r#"{"manifest_version":3,"name":"Example","version":"1.2","permissions":["storage"]}"#,
        )
        .unwrap();
        fs::write(source.join("worker.js"), "first").unwrap();
        let staged = stage_package(&source, &managed).unwrap();
        fs::write(source.join("worker.js"), "modified").unwrap();
        assert_eq!(
            fs::read_to_string(staged.join("worker.js")).unwrap(),
            "first"
        );
        assert_eq!(read_manifest(&staged).unwrap().permissions, ["storage"]);
        fs::remove_dir_all(temp).unwrap();
    }

    fn test_directory() -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("aurora-extension-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
