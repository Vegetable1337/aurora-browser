# Aurora

Aurora is an early Windows desktop browser alpha built with Qt for Python and Qt WebEngine (Chromium). It is a real embedded browser window with tabs, navigation, search, persistent cookies/cache, browser history supplied by Chromium, and user-selected downloads. It is not yet a secure or privacy-complete browser.

## Run on Windows 10/11

1. Install 64-bit Python 3.13 for the verified Windows build. Python 3.15 source execution works on this machine, but its frozen PyInstaller build failed during Qt startup.
2. In PowerShell, from this folder:

   ```powershell
   py -3.13 -m venv .venv
   .\.venv\Scripts\Activate.ps1
   python -m pip install -r requirements.txt
   python app\main.py
   ```

The Qt WebEngine runtime is large. On first launch, Chromium stores the default profile under `%LOCALAPPDATA%\Aurora\Default`. Downloads are saved only after the user chooses a destination. The Windows portable build is a folder: run `dist\Nuitka\main.dist\Aurora.exe` and keep the whole `main.dist` folder together.

Use **Profiles → Create password-protected profile** to create an isolated profile. Aurora asks for a password of at least 12 characters and displays a one-time recovery key. Save that key before closing the dialog. The protected profile keeps Aurora's bookmarks and visit list encrypted and stores Chromium data in its own folder; cookies, Chromium's history, cache and saved website credentials in that folder are still plaintext. **Profiles → Lock** closes its tabs and returns to the unprotected Default profile.

Press **Ctrl+Shift+N** to open a private window. It uses an off-the-record Chromium profile with in-memory cache and non-persistent cookies; Aurora does not save that window's visit list or bookmarks. Files you explicitly download still go to the location you choose. Websites and network providers can still observe connections; private mode is not anonymity.

## Current functionality

- Chromium-based website rendering through Qt WebEngine.
- Multiple tabs, address/search bar, back/forward/reload, and keyboard shortcuts.
- Persistent default browser profile and cookies.
- SQLite bookmark save/list/open, stored alongside the default profile.
- Recent visit list, including encrypted records for password-protected profiles; clearing it removes Aurora's list but may not erase Chromium's own on-disk records.
- Find text on the current page with `Ctrl+F`; use `Enter`/`Ctrl+G` for the next match and `Ctrl+Shift+G` for the previous match.
- Private windows with off-the-record profiles, memory-only HTTP cache, and non-persistent cookies.
- Website prompts for camera, microphone, location, notifications, clipboard and other supported browser permissions. Decisions are kept in memory for the profile session.
- A small built-in tracker-domain blocker, enabled by default and switchable in the Privacy menu. The list is limited and does not update automatically.
- Separate password-protected browser profiles with encrypted bookmarks and password/recovery unlock.
- Explicit save dialog for downloads.
- Original dark glass-inspired navigation chrome.

Whole-profile encryption, a full filter-list engine, third-party-cookie blocking, permissions management, account/sync, and extension installation are not implemented. Do not use this alpha for sensitive browsing or treat it as a hardened browser.

## Architecture and limitations

See [docs/architecture.md](docs/architecture.md) and [docs/threat-model.md](docs/threat-model.md). Qt WebEngine provides a maintained Chromium integration and, in current Qt releases, a Chrome extension manager API. That API's presence does not establish that Aurora has extension support; Aurora currently has no extension UI, permission review, or compatibility suite. Firefox WebExtensions compatibility is unimplemented. Chromium-managed profile data is presently plaintext at rest, including cookies and cache. There is no whole-profile encryption or isolation claim.

Password-protected profiles use the key-vault in [app/crypto_profiles.py](app/crypto_profiles.py) for encrypted bookmarks and Aurora's visit list. Their Chromium cookies, history, cache, saved website credentials and other engine files remain plaintext at rest. The unprotected `Default` profile also remains available. Coverage and limitations are recorded in [docs/profile-crypto.md](docs/profile-crypto.md).

Qt WebEngine embeds a particular Chromium revision and does not automatically update in lockstep with Google Chrome. Release packaging must pin and verify Qt/Chromium versions, ship signed updates, and track Qt security releases before distribution.

## Packaging status

Build the Windows portable folder from PowerShell with `.\packaging\build-windows.ps1`. The script uses Python 3.13 and Nuitka with its PySide6 plugin, then loads a local HTTP test page in packaged Chromium before reporting success. The output is `dist\Nuitka\main.dist\Aurora.exe`; keep that whole folder together. It is not a signed installer or update channel.

## Development status

This repository is the first executable milestone, not the seven-day alpha acceptance state. The supplied brief references a visual reference image, but no image was present in the attachment or workspace when implementation began.
