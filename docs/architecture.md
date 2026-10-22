# Architecture decision record: initial browser engine

**Status:** Accepted for the initial Windows alpha; reassess before production hardening.

## Context

Aurora needs a real Chromium renderer and a portable native interface. The development machine is Windows 10 x64. Python 3.15 was installed initially; packaging verification later used a separate Python 3.13 environment.

## Decision

Use Qt 6 / Qt WebEngine for the browser engine integration and Qt Widgets for the first interface. The executable is currently Python using the official PySide6 bindings, with the browser surface provided by Qt WebEngine's Chromium build. Keep browser-specific UI and profile code in the application layer so security-sensitive components can be moved to native C++ or Rust if needed.

Qt WebEngine is cross-platform across Windows, macOS, and Linux. Current Qt API documentation lists `QWebEngineExtensionManager` for loading Chrome extensions from the filesystem. This is a capability to evaluate, not evidence of complete Chrome or Firefox compatibility. Aurora's extension integration remains unimplemented.

## Alternatives considered

- **CEF:** mature Chromium embedding with native C++ integration, but does not provide full Chrome extension support out of the box and requires a maintained native integration/build pipeline. It is a reasonable fallback if Qt WebEngine's extension and security constraints prove insufficient.
- **A maintained Chromium fork:** gives the greatest control over browser behavior and extension parity, but requires a large ongoing Chromium merge, build, and security response operation. That infrastructure is absent, so it is not appropriate for the initial executable milestone.
- **Microsoft WebView2:** immediately available on this Windows machine and Chromium-based, but Windows-specific, and not a good cross-platform foundation for the stated scope.

## Browser source projects consulted

- **Brave adblock-rust** is integrated as a dependency through the community Python bindings (`adblock==0.6.0`). Aurora expresses its existing bundled tracker host set as standard ABP rules and passes Chromium request URLs, first-party URLs, and resource types to this engine. The Rust engine is MPL-2.0; the Python bindings are MIT OR Apache-2.0. See `docs/third-party-sources.md` for links and scope. The Python bindings are an older release, so upgrading them and validating the Windows package is required before shipping broadly.
- **qutebrowser** is a Python/Qt WebEngine browser consulted as an architectural reference for keeping browser behavior in the Qt integration layer. No qutebrowser source code was copied; its GPL-3.0-or-later license is therefore not added to Aurora by this work.
- **Chromium** is the embedded browser engine supplied by Qt WebEngine. Aurora does not contain or build the Chromium source tree itself.

## Consequences

- We can ship a cross-platform UI and embedded Chromium without maintaining a Chromium fork.
- Qt owns the Chromium integration layer and release cadence; Aurora must monitor and update Qt promptly.
- Qt WebEngine is not the same product/runtime as Google Chrome. Extension API coverage, service worker behavior, profile storage encryption, and Chromium command-line policy need independent validation.
- Python is suitable for the alpha UI but not yet an established packaging or security boundary. A production build needs a pinned, reproducible native build and signed distribution pipeline.
- Password-protected profiles connect the key-vault to bookmark records and use separate Qt WebEngine profile directories. Qt's persistent profile directories remain ordinary on-disk Chromium data.
- A Windows folder build succeeds with Python 3.13 and Nuitka 4.3.1. Its packaged smoke mode loads a local HTML page in Qt WebEngine and exits successfully. PyInstaller builds failed during `PySide6.QtCore` import, so the documented script uses Nuitka. The output is a portable folder, not a signed installer; updates are not implemented.
