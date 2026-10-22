# Initial threat model and known gaps

> This document describes the legacy Python/Qt implementation. Current Rust
> boundaries and unverified security properties are recorded in
> [the P0 audit](p0-audit.md). Python protections must not be assumed to exist in Rust.

## Intended scope

This Windows alpha renders websites using Qt WebEngine/Chromium and supports tabs, navigation, search, bookmarks, recent visits, downloads, private windows, and password-protected profiles. It has basic website permission prompts, but it is not yet a privacy-hardened daily browser.

## Security properties inherited from the engine

Aurora does not add Chromium command-line switches that disable sandboxing, certificate verification, or site isolation. The embedded runtime's actual sandbox behavior depends on the bundled Qt/Chromium build and platform; it still needs verification in packaged Windows, macOS, and Linux builds. This statement is not a claim that Aurora has completed a sandbox audit.

## Data and threats

- Persistent cookies, cache, local storage and other Chromium profile data are kept under `%LOCALAPPDATA%\Aurora\Default` without Aurora application-layer encryption.
- Aurora supports password-protected profiles with separate Chromium data directories and an encrypted store for bookmarks and Aurora's own visit list. The `Default` profile remains unprotected.
- In a protected profile, cookies, history, cache, local storage, saved website credentials, permissions and other Chromium-managed files are still plaintext at rest. Profile isolation separates directories; it does not encrypt them.
- Protected-profile bookmark and Aurora visit-list values are encrypted, and URLs are not used directly as SQLite record IDs. Profile display names and basic record metadata remain visible.
- Locking destroys Aurora's active tabs and Qt profile object, then clears its vault buffer. It cannot promise renderer exit, key erasure from RAM/swap, or encrypted Chromium files. Do not treat Aurora as a whole-profile encrypted product.
- Downloads are written to a user-selected location and are outside the profile directory; Aurora does not encrypt them.
- Private windows use an off-the-record Qt WebEngine profile with memory-only HTTP cache, non-persistent cookies, and no Aurora visit-list writes. User-selected downloads remain on disk, and websites/network providers still observe connections.
- Supported website permission requests are prompted in the UI and decisions are held in memory for the active profile session. No dedicated permission-management screen is implemented.
- A small built-in host list blocks some known tracking domains through Qt WebEngine's request interceptor. It is not a full EasyList/EasyPrivacy filter engine, has no automatic updates, and can break features on affected sites. It can be disabled in the Privacy menu.
- Session cookies are not forced onto disk for crash recovery. No third-party-cookie restriction, secure DNS controls, or fingerprinting protections have been implemented.
- No telemetry or Aurora account service is implemented by the app. Web pages still make their own network requests.
- No automatic updater or release signature verification is implemented.

## Claims we do not make

Aurora does not promise anonymity, full profile encryption, secure erasure, extension compatibility, or protection from a compromised OS account, malicious website, compromised Qt/Chromium dependency, or malicious browser extension.

## Work required before sensitive use

Encrypt or remove Chromium-managed storage at rest; audit all profile and auxiliary storage paths; isolate profile processes and extension state; adopt and update a complete, tested filter-list engine and third-party-cookie controls; verify sandbox and site isolation in packaged builds; establish signed, promptly updated releases; and complete threat-model review and security regression testing.
