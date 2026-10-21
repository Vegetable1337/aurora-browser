# Initial threat model and known gaps

## Intended scope

This alpha can render websites using Qt WebEngine/Chromium, open multiple tabs, search, navigate, and save user-approved downloads. It is useful for evaluating the window and engine integration only.

## Security properties inherited from the engine

Aurora does not add Chromium command-line switches that disable sandboxing, certificate verification, or site isolation. The embedded runtime's actual sandbox behavior depends on the bundled Qt/Chromium build and platform; it still needs verification in packaged Windows, macOS, and Linux builds. This statement is not a claim that Aurora has completed a sandbox audit.

## Data and threats

- Persistent cookies, cache, local storage and other Chromium profile data are kept under `%LOCALAPPDATA%\Aurora\Default` without Aurora application-layer encryption.
- There are no separate authenticated browser profiles. Anyone with access to the signed-in Windows account and its files may read the profile using suitable tools.
- The bookmark database is SQLite in that same default profile directory and is also plaintext.
- A separate encrypted vault API exists for Aurora-owned records, but the browser does not yet use it. It does not protect the active Chromium profile or bookmarks database. Do not treat the current browser as a whole-profile encrypted product.
- Downloads are written to a user-selected location and are outside the profile directory; Aurora does not encrypt them.
- No tracker blocklist, third-party-cookie policy, secure DNS controls, per-site permission controls, or fingerprinting protections have been implemented.
- No telemetry or Aurora account service is implemented by the app. Web pages still make their own network requests.
- No automatic updater or release signature verification is implemented.

## Claims we do not make

Aurora does not promise anonymity, full profile encryption, secure erasure, extension compatibility, or protection from a compromised OS account, malicious website, compromised Qt/Chromium dependency, or malicious browser extension.

## Work required before sensitive use

Implement and test profile-specific key generation and OS-protected key wrapping; audit all Chromium and auxiliary storage paths; isolate profile processes and extension state; add explicit privacy controls backed by browser APIs; verify sandbox and site isolation in packaged builds; establish signed, promptly updated releases; and complete threat-model review and security regression testing.
