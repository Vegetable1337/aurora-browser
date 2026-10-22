# Aurora Rust prototype

This folder starts the gradual Rust migration. It builds the browser window and Chromium-based page using Rust with Wry and the installed Microsoft Edge WebView2 Runtime. The current Python/Qt app remains the feature-complete prototype while Rust is developed.

## Windows build prerequisites

- Rust stable (`rustup` and `cargo`)
- Visual Studio Build Tools with the **Desktop development with C++** workload and a Windows SDK
- Microsoft Edge WebView2 Runtime

Build and launch from this folder:

```powershell
cargo run
```

The page profile and `bookmarks.json`, `history.json` and `settings.json` are stored under `%LOCALAPPDATA%\Aurora\Rust\Default`. The current Rust milestone has tabs, address/search navigation, back/forward/reload, bookmarks, visit history, history clearing, basic downloads to the Windows Downloads folder, and custom window controls. Tabs show a loading indicator and can scroll horizontally when the strip fills; middle-click closes a tab. New tabs use a configurable Aurora or DuckDuckGo start page, and a separate animated overlay provides the library and settings. Settings include the search provider, light/dark appearance, and motion effects.

The Aurora mark is maintained as a vector in `assets/aurora.svg`; generated PNG and multi-size ICO versions provide the window and Windows executable icons.

Downloads currently go directly to Downloads with collision-safe filenames; there is not yet a save-location prompt or a downloads manager. Searches use DuckDuckGo. Private windows, website permission prompts, tracker blocking, encrypted profiles and the rest of the privacy protections from the Python version are not yet migrated. The Rust version is still an incomplete prototype; use the Python version for those features.
