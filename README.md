# Aurora

Aurora is an early Windows desktop browser alpha built with Qt for Python and Qt WebEngine (Chromium). It is a real embedded browser window with tabs, navigation, search, persistent cookies/cache, browser history supplied by Chromium, and user-selected downloads. It is not yet a secure or privacy-complete browser.

## Run on Windows 10/11

1. Install 64-bit Python 3.10 or newer.
2. In PowerShell, from this folder:

   ```powershell
   py -m venv .venv
   .\.venv\Scripts\Activate.ps1
   python -m pip install -r requirements.txt
   python app\main.py
   ```

The Qt WebEngine runtime is large. On first launch, Chromium stores the default profile under `%LOCALAPPDATA%\Aurora\Default`. Downloads are saved only after the user chooses a destination.

Use **Profiles → Create password-protected profile** to create an isolated profile. Aurora asks for a password of at least 12 characters and displays a one-time recovery key. Save that key before closing the dialog. The protected profile keeps bookmarks encrypted and stores its Chromium data in its own folder; cookies, history, cache and saved website credentials in that folder are still plaintext. **Profiles → Lock** closes its tabs and returns to the unprotected Default profile.

## Current functionality

- Chromium-based website rendering through Qt WebEngine.
- Multiple tabs, address/search bar, back/forward/reload, and keyboard shortcuts.
- Persistent default browser profile and cookies.
- SQLite bookmark save/list/open, stored alongside the default profile.
- Separate password-protected browser profiles with encrypted bookmarks and password/recovery unlock.
- Explicit save dialog for downloads.
- Original dark glass-inspired navigation chrome.

Profile management, encryption, private browsing, tracker blocking, permissions UI, account/sync, and extension installation are not implemented. Do not use this alpha for sensitive browsing or treat it as a hardened browser.

## Architecture and limitations

See [docs/architecture.md](docs/architecture.md) and [docs/threat-model.md](docs/threat-model.md). Qt WebEngine provides a maintained Chromium integration and, in current Qt releases, a Chrome extension manager API. That API's presence does not establish that Aurora has extension support; Aurora currently has no extension UI, permission review, or compatibility suite. Firefox WebExtensions compatibility is unimplemented. Chromium-managed profile data is presently plaintext at rest, including cookies and cache. There is no whole-profile encryption or isolation claim.

Password-protected profiles use the key-vault in [app/crypto_profiles.py](app/crypto_profiles.py) for encrypted bookmarks. Their Chromium cookies, history, cache, saved website credentials and other engine files remain plaintext at rest. The unprotected `Default` profile also remains available. Coverage and limitations are recorded in [docs/profile-crypto.md](docs/profile-crypto.md).

Qt WebEngine embeds a particular Chromium revision and does not automatically update in lockstep with Google Chrome. Release packaging must pin and verify Qt/Chromium versions, ship signed updates, and track Qt security releases before distribution.

## Packaging status

PyInstaller completed a Windows folder build, but its executable failed at startup while importing `PySide6.QtCore` (`DLL load failed ... The specified procedure could not be found`) on the available Python 3.15 environment. Treat `dist/` output as a failed diagnostic build, not a usable installer. Run from source for now. The packaging script is present, but production packaging remains blocked until the frozen Qt runtime starts successfully.

## Development status

This repository is the first executable milestone, not the seven-day alpha acceptance state. The supplied brief references a visual reference image, but no image was present in the attachment or workspace when implementation began.
