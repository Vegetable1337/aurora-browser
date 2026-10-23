# Aurora threat model and known gaps

## Scope

Aurora's primary application is a Windows Rust/Wry shell around the system Microsoft Edge WebView2 Runtime. This document describes implemented boundaries and open work; it is not a security certification.

## Implemented boundaries

- Website tabs do not receive Aurora's toolbar or overlay IPC handler. The privileged toolbar and library/settings/extension-manager overlay are separate WebView2 views in a context with extensions disabled.
- Privileged UI navigation is restricted to the exact compiled-in HTML document; popups are denied.
- Ordinary profiles use separate WebView2 user-data directories. Private tabs use WebView2's incognito mode and do not enter Aurora's visit history.
- Local unpacked Manifest V3 extensions are copied into profile-owned storage, and requested manifest permissions are displayed before installation. Enable/remove operations are restricted to stable IDs in Aurora's profile registry.
- The extension source copy is capped at 20,000 files and 256 MiB. Symlinks/reparse points and non-file filesystem objects are rejected.
- Aurora does not add Chromium command-line switches intended to disable sandboxing, TLS checks or site isolation. This code review does not verify the effective WebView2 process mitigations or sandbox token.

## Data and extension risks

- Protected profiles encrypt Aurora-owned bookmark and visit records. WebView2 cookies, cache, history, local storage, IndexedDB, credentials, permissions and extension storage remain WebView2-managed data on disk and are not encrypted by Aurora.
- Managed extension source files and the extension ID/permission registry are not encrypted. The profile owner or malware running as that user can modify them.
- The permission confirmation displays the manifest's declared API and host permissions. Aurora does not implement a complete Chrome permission broker, optional-host-permission prompt flow, or per-host runtime permission editor.
- WebView2 extension APIs provide a subset and a different browser UI from Chrome. Extension action popups and complete Chrome Web Store behavior are not supported or claimed. Firefox packages and Firefox-specific API coverage are not tested.
- Dropping the profile's WebView2 views/context closes Aurora's tab handles. Whether all extension service workers and renderer processes have stopped at profile lock is not verified.
- The Rust app filters HTTP(S) requests surfaced by WebView2 `WebResourceRequested` with Brave adblock-rust, EasyList/EasyPrivacy startup refresh and built-in fallback rules. It does not filter DNS, WebView2 internal traffic or cosmetic elements. For subframes it uses the top-level page source as initiator context. The displayed count is blocked requests in the active profile state, not all ads/tracking attempts on the network.
- Filter lists come from fixed HTTPS hosts; the updater rejects insecure or cross-host redirects, caps each list at 12 MiB, checks basic ABP structure and retains the last valid cache if refresh fails. It does not verify publisher signatures. A compromised list source/CA or locally modified ordinary profile cache remains a risk.
- HTTPS-first, third-party-cookie controls, DNS-over-HTTPS configuration, WebRTC controls, fingerprinting defenses and site permission management are not implemented in the Rust app.
- Downloads are saved outside encrypted Aurora storage. The ordinary profile is not password-locked.
- There is no Aurora telemetry/account sync service or automatic application updater. Web pages make their own network requests and can identify users through normal web mechanisms.

## Claims Aurora does not make

Aurora does not promise anonymity, whole-profile encryption, secure erasure, full Chrome/Firefox extension compatibility, complete tracker blocking, or protection from a compromised operating-system account, malicious website, malicious extension, or vulnerable WebView2 runtime.

## Verification still required

Run the extension lifecycle against the main Aurora window, verify the toolbar and overlay remain free of extension injection using hostile test packages, inspect WebView2 process mitigations in packaged builds, and test behavior after locking/switching profiles. Exercise live EasyList updates and real WebView2 blocking across request contexts, measure UI responsiveness under full lists, and test the privacy panel on normal/private tabs and after profile switching. Review and test all data paths before describing protected profiles as encrypted browser profiles.
