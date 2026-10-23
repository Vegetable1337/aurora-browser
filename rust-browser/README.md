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

The Library menu includes an experimental Extensions manager. It accepts local
unpacked Manifest V3 folders, stages an independent copy per WebView2 profile,
shows the declared API/host permissions for confirmation, and supports listing,
enabling, disabling and removing Aurora-managed extensions. The toolbar and
Library overlay use a separate extension-free context. Firefox packages, Chrome
Web Store install/update, complete Chrome API coverage, and extension action
popups are unsupported or unverified. See [extension status](../docs/extensions-prototype.md).

Downloads now ask for a save location, but there is not yet a downloads manager. Searches use DuckDuckGo. Separate private browser windows, website permission management and tracker blocking from the Python version are not yet migrated. The Rust version remains incomplete; a private tab is not a separate private window.

Protected profile management is available from the library menu: create, switch, lock, change the password, delete, export an encrypted backup, and restore one. Password and recovery-key unlock use Argon2id, AES-256-GCM and HKDF-SHA-256. On Windows the master key also has a user-scoped DPAPI wrapper. Aurora bookmarks/history are encrypted in SQLite. WebView2 data is kept in a separate directory for each protected profile, but is not encrypted; it includes cookies, cache, and other Chromium data. Do not treat this as whole-profile encryption.
