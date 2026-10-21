# Architecture decision record: initial browser engine

**Status:** Accepted for the initial Windows alpha; reassess before production hardening.

## Context

Aurora needs a real Chromium renderer, a portable native interface, and a plausible path to extension support. The development machine is Windows 10 x64 and initially had no CMake, Ninja, Rust, .NET SDK, Qt installation, or C++ compiler available. Python 3.15 was installed.

## Decision

Use Qt 6 / Qt WebEngine for the browser engine integration and Qt Widgets for the first interface. The executable is currently Python using the official PySide6 bindings, with the browser surface provided by Qt WebEngine's Chromium build. Keep browser-specific UI and profile code in the application layer so security-sensitive components can be moved to native C++ or Rust if needed.

Qt WebEngine is cross-platform across Windows, macOS, and Linux. Current Qt API documentation lists `QWebEngineExtensionManager` for loading Chrome extensions from the filesystem. This is a capability to evaluate, not evidence of complete Chrome or Firefox compatibility. Aurora's extension integration remains unimplemented.

## Alternatives considered

- **CEF:** mature Chromium embedding with native C++ integration, but does not provide full Chrome extension support out of the box and requires a maintained native integration/build pipeline. It is a reasonable fallback if Qt WebEngine's extension and security constraints prove insufficient.
- **A maintained Chromium fork:** gives the greatest control over browser behavior and extension parity, but requires a large ongoing Chromium merge, build, and security response operation. That infrastructure is absent, so it is not appropriate for the initial executable milestone.
- **Microsoft WebView2:** immediately available on this Windows machine and Chromium-based, but Windows-specific, and not a good cross-platform foundation for the stated scope.

## Consequences

- We can ship a cross-platform UI and embedded Chromium without maintaining a Chromium fork.
- Qt owns the Chromium integration layer and release cadence; Aurora must monitor and update Qt promptly.
- Qt WebEngine is not the same product/runtime as Google Chrome. Extension API coverage, service worker behavior, profile storage encryption, and Chromium command-line policy need independent validation.
- Python is suitable for the alpha UI but not yet an established packaging or security boundary. A production build needs a pinned, reproducible native build and signed distribution pipeline.
- The key-vault code encrypts Aurora-owned records through an explicit API, but it is not connected to the browser window or bookmarks. Qt's persistent profile directories remain ordinary on-disk Chromium data.
- A PyInstaller Windows folder build completed, but its frozen executable failed during `PySide6.QtCore` import on the available Python 3.15 installation. Source execution works; packaging is an open issue, not a verified deliverable.
