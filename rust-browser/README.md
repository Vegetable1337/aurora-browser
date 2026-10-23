# Aurora Rust prototype

Current security review and outstanding requirements: [P0 audit](../docs/p0-audit.md).
Protected profiles do not yet encrypt WebView2 data on disk. Extension compatibility
is limited to local unpacked Chrome Manifest V3 extensions through WebView2; it
is not full Chrome or Firefox compatibility.

This folder contains Aurora's primary Rust application. It builds the browser window and Chromium-based page using Rust, Wry and the installed Microsoft Edge WebView2 Runtime. The former Python/Qt app remains in the repository as a migration reference.

## Windows build prerequisites

- Rust stable (`rustup` and `cargo`)
- Visual Studio Build Tools with the **Desktop development with C++** workload and a Windows SDK
- Microsoft Edge WebView2 Runtime

Build and launch from this folder:

```powershell
cargo run
```

The app requires the Microsoft Edge WebView2 Runtime. Do not treat the current
extension integration as a general-purpose Chrome/Firefox extension platform.

The page profile and `bookmarks.json`, `history.json` and `settings.json` are stored under `%LOCALAPPDATA%\Aurora\Rust\Default`. The current Rust milestone has tabs, address/search navigation, back/forward/reload, find-in-page (`Ctrl+F`, Enter and `Ctrl+G`), bookmarks, visit history, history clearing, basic downloads to the Windows Downloads folder, and custom window controls. Tabs show a loading indicator and can scroll horizontally when the strip fills; middle-click closes a tab. New tabs use a configurable Aurora or DuckDuckGo start page, and a separate animated overlay provides the library and settings. Settings include the search provider, light/dark appearance, and motion effects. A private tab can be opened from the library menu; it uses WebView2's incognito mode and its page visits are excluded from Aurora history.

The Aurora mark is maintained as a vector in `assets/aurora.svg`; generated PNG and multi-size ICO versions provide the window and Windows executable icons.

Network privacy filtering uses Brave `adblock-rust` in WebView2's HTTP(S) `WebResourceRequested` callback. EasyList and EasyPrivacy are fetched over HTTPS from fixed hosts at startup, checked for HTTPS/same-host redirects, size and basic ABP structure, and cached through an atomic replacement. The current-site exception is stored per profile; exceptions for password-protected profiles are encrypted in their vault. The privacy panel shows the active toggle, real blocked-request count and current-site exception. When lists are unavailable, a small built-in rule set remains active.

The callback sees WebView2 resource contexts (documents, scripts, stylesheets, images, media, fonts, XHR/fetch, WebSocket and related HTTP(S) requests). DNS, WebView2 internal traffic, non-HTTP(S) protocols and cosmetic page-element hiding are outside this filter. For subframes, WebView2's current top-level source URL is used as the page context. Filter parsing and engine construction run on a worker thread; the active engine is replaced as one shared snapshot. This does not provide full anonymity or guarantee that every tracker is blocked.

The Library menu includes an experimental Extensions manager. It accepts local
unpacked Manifest V3 folders, stages an independent copy per WebView2 profile,
shows the declared API/host permissions for confirmation, and supports listing,
enabling, disabling and removing Aurora-managed extensions. The toolbar and
Library overlay use a separate extension-free context. Firefox packages, Chrome
Web Store install/update, complete Chrome API coverage, and extension action
popups are unsupported or unverified. See [extension status](../docs/extensions-prototype.md).

Downloads now ask for a save location, but there is not yet a downloads manager. Searches use DuckDuckGo. Separate private browser windows and website permission management from the Python version are not yet migrated. The Rust version remains incomplete; a private tab is not a separate private window.

Protected profile management is available from the library menu: create, switch, lock, change the password, delete, export an encrypted backup, and restore one. Password and recovery-key unlock use Argon2id, AES-256-GCM and HKDF-SHA-256. On Windows the master key also has a user-scoped DPAPI wrapper. Aurora bookmarks/history are encrypted in SQLite. WebView2 data is kept in a separate directory for each protected profile, but is not encrypted; it includes cookies, cache, and other Chromium data. Do not treat this as whole-profile encryption.

Privacy Center now includes origin-scoped camera/microphone/geolocation/notification policy, encrypted protected-profile settings and private-tab isolation. See [implementation, limits and manual checklist](../docs/privacy-center.md). Site data cleanup, certificate details and HTTPS-first failure handling remain pending.
