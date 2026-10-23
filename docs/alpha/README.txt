Aurora Browser 0.1.0 Alpha for Windows x64
==========================================

SYSTEM REQUIREMENTS
- 64-bit Windows 10 version 1709 or later, or Windows 11.
- Microsoft Edge WebView2 Evergreen Runtime.
- Internet access is needed to update EasyList/EasyPrivacy and visit websites.
- The installer does not require administrator rights.

INSTALL
Run Aurora-0.1.0-alpha-Windows-x64-Setup.exe. Setup installs to the current
user's LocalAppData\Programs folder, adds a Start menu shortcut, and offers an
optional desktop shortcut. If WebView2 Runtime is missing, Setup opens the
official Microsoft download page and exits; install the Runtime and run Setup
again. You can also use the portable ZIP by extracting all files and running
Aurora.exe.

UPDATE
Run a newer Aurora Setup with the same AppId. It installs over the previous
application files. Aurora user data is stored outside the installation folder
under %LOCALAPPDATA%\Aurora\Rust and is not removed by updates.

UNINSTALL
Use Windows Settings > Apps > Installed apps > Aurora Browser > Uninstall, or
the Start menu uninstall entry. Uninstall removes the application and its
shortcuts only. User profiles and browser data are intentionally retained.
To remove profile data manually, first export an encrypted backup if needed,
then remove %LOCALAPPDATA%\Aurora\Rust yourself.

SUPPORTED IN THIS ALPHA
- Tabs, address/search navigation, history, bookmarks, settings and private tabs.
- Local unpacked Chrome Manifest V3 extensions through a limited WebView2 API.
- Brave adblock-rust request filtering for supported HTTP(S) requests.
- HTTPS-first navigation, origin-scoped site permissions and site-data controls.
- Protected Aurora profile records and encrypted backup export/restore.

EXPERIMENTAL OR UNSUPPORTED
- Extension support is limited to local unpacked Manifest V3 extensions.
  Chrome Web Store installation/update, Firefox add-ons, full Chrome API coverage,
  and action popups are not supported or verified.
- WebView2 data (cookies, cache, IndexedDB, LocalStorage and extension storage)
  is not encrypted by Aurora's protected-profile vault.
- Ad blocking does not cover DNS, WebView2 internal traffic, or cosmetic page
  element hiding. Blocking is not comprehensive.
- No account sync, automatic browser update service, code signing, or anonymity
  guarantee is provided.

SECURITY AND BACKUPS
Protected profiles encrypt Aurora-managed records, not every browser-engine file.
Export an encrypted profile backup before updates or experiments and keep its
password/recovery key separately. Never send the recovery key with the backup.
This alpha has not received an independent security audit. Avoid sensitive use.

Microsoft documents the WebView2 Windows support matrix and Runtime detection
here: https://learn.microsoft.com/en-us/microsoft-edge/webview2 and
https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution

Manual GUI regression checklist: docs/alpha/manual-checklist.md
Known limitations and test evidence: docs/p0-audit.md
Source code and third-party notices are available in the project repository:
https://github.com/Vegetable1337/aurora-browser
