# Changelog

## 0.1.0-alpha — 2026-10-23

This is the first prepared Windows x64 Alpha distribution of the Rust/Wry app.

- Per-user Windows installer with Start menu shortcut and optional desktop shortcut.
- Portable ZIP containing the executable, license, and user quickstart.
- Setup checks for Microsoft Edge WebView2 Evergreen Runtime and points to the
  official installer page when it is missing.
- Installer upgrades the app in place and leaves `%LOCALAPPDATA%\Aurora\Rust`
  profile data untouched on update and uninstall.
- Local unpacked Chrome Manifest V3 extensions, with limited WebView2 API support.
- Brave adblock-rust filtering, HTTPS-first, origin-scoped website permissions,
  Privacy Center, site-data controls, downloads, protected Aurora records, and
  encrypted profile backup/restore.

### Known limitations

- Aurora's vault does not encrypt WebView2 engine files such as cookies, cache,
  IndexedDB, LocalStorage, and extension storage.
- Chrome Web Store install/update, Firefox add-ons, full Chrome API coverage, and
  extension action popups are unsupported or unverified.
- The network filter does not cover DNS, WebView2 internal traffic, or cosmetic
  filtering and does not provide anonymity.
- This alpha has not received an independent security audit. See
  [`docs/p0-audit.md`](docs/p0-audit.md) and [`docs/threat-model.md`](docs/threat-model.md).
