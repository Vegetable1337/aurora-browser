# Privacy Center and origin permissions (2026-10-23)

The first implementation is available in Settings → Privacy. It retains Aurora's existing glass panel, light/dark themes and reduced motion support.

## Implemented

- Cookies for the active HTTP(S) site are queried using the WebView2 profile cookie manager. The interface lists names and cookie attributes but never reads or displays values. Removal uses WebView2's domain/path/name API and Aurora verifies the cookie domain belongs to the active host.
- "Clear site data" uses WebView2's DevTools `Storage.clearDataForOrigin` for the exact HTTP(S) origin. This clears supported origin-scoped storage such as cookies, local storage, IndexedDB, Cache Storage and service workers. It does not clear history or downloaded files. WebView2's origin storage command has no private-browsing context parameter; it runs against the active view's own profile/context.
- "Clear profile cache" uses WebView2 `ICoreWebView2Profile2::ClearBrowsingData(DISK_CACHE)`. WebView2 only exposes this cache operation at profile scope, so the confirmation and label explain it affects every site in the current WebView2 profile. It does not remove cookies or site storage.
- Both destructive actions ask for confirmation. Cookie values are omitted from IPC and logs. Async results are discarded if the active tab or origin changed before completion.
- HTTPS-first is on by default for new hostnames entered without a scheme. Localhost, loopback, RFC1918/link-local IPs and `.local` hosts use HTTP directly for local services. An explicitly entered HTTP URL or an HTTP redirect is stopped in WebView2's navigation handler and requires a native Yes/No confirmation. Consent is remembered only for that origin in the current tab.
- On an entered HTTPS URL, WebView2 `NavigationCompleted` is observed. Only connection/DNS/timeout failures offer an HTTP retry; certificate failures, cancellations and other errors never do. Aurora does not relax TLS checks or install interception certificates. Users can disable HTTPS-first in Privacy settings; old settings files default to enabled.

- Displays the current HTTP(S) origin and explicitly distinguishes HTTP from an HTTPS URL. This is not a certificate viewer or a mixed-content audit; the UI states that those details have not been retrieved. TLS verification remains the engine's responsibility and is not bypassed.
- Shows separate profile and current-origin blocking counters. Counts are acknowledged only after WebView2 accepts the replacement response, not merely after an ABP match. Subframe attribution uses the top-level WebView Source, as in the existing filter.
- Site filter exceptions use the existing host-based policy. The UI continues to label this as a site/host exception, not a permission origin. A private tab gets independent in-memory filter exceptions and counters.
- Camera, microphone, geolocation and notifications support Ask / Allow / Deny. Settings are keyed by normalized URL origin: scheme, hostname and non-default port; paths and default ports do not create distinct entries. Other permission kinds are denied.
- WebView2 PermissionRequested enforces the policy for each requesting origin, including frame-origin requests. Ask presents a native Yes/No prompt for that request. No sensitive access is granted by default. OS-level privacy restrictions can still deny an allowed request.
- WebView2 SavesInProfile is false: permission policy is saved only in Aurora. Default-profile settings use atomic JSON replacement; protected profiles use the existing encrypted vault under the site-permissions/policy record. Private-tab policy remains in memory and starts from Ask.
- Before permitting HTTP(S) navigation in a new content view, Aurora enumerates and resets legacy native permission decisions using Profile4. Existing supported choices are imported into Aurora policy without overriding an explicit Aurora choice; a UUID snapshot of native settings is saved (encrypted for protected profiles) before clearing the native entries. No new plaintext native Allow decisions are stored. If initialization fails, network navigation stays blocked and stderr identifies the failure.
- Setting/resetting checks the active tab ID and origin to reject delayed commands after navigation/profile switching. Successful changes reload matching tabs so existing media streams close. Closing or switching a profile drops its tabs and policy references.

## Checks actually executed

- Latest `cargo test --locked`: 20 tests passed: 18 shared-component tests and 2 main-app tests. Tests include HTTPS-first resolution, network-error classification and local/LAN handling, exact-origin validation, cookie-domain scope validation, counter acknowledgment and legacy-permission migration.
- Added origin/scheme/port/reset validation and encrypted policy round-trip, independent profile and locked-vault tests.
- `cargo run --example site_permissions_probe --locked`: actual WebView2 HTTP loopback page requested geolocation and received PERMISSION_DENIED; the test also asserted that the production Aurora permission callback was actually invoked. Uses a new UUID temporary profile, no user data and a 15-second watchdog. This is an API integration test, not a mouse-driven GUI result.
- `cargo run --example site_data_probe --locked`: real WebView2 queried a test cookie, removed it through CookieManager, cleared the exact origin through `Storage.clearDataForOrigin`, cleared the profile disk cache, then checked that the cookie and localStorage value were gone. It used a new UUID temporary profile and a 15-second watchdog.
- HTTPS-first unit tests cover default address resolution, local/LAN exemptions and the network-error allowlist. The native modal confirmation and a live real-site HTTPS failure were not GUI-tested in this run.
- `node --check` on extracted inline scripts: chrome.html and overlay.html passed.

## Remaining stages and manual checks

Cookie listing/deletion, origin storage cleanup and profile-wide cache clearing are implemented through WebView2 APIs and exercised by the disposable native integration probe. Per-site HTTP cache clearing is not supported by the exposed WebView2 API; only the profile-wide cache action is available. Certificate details, HTTPS-first failure handling and download history remain pending.

GUI checks still required: navigate two HTTPS origins with distinct ports; open Settings → Privacy; change each selector and confirm reload; verify a native Ask prompt; repeat in a protected and a private tab; switch/lock profiles and verify defaults and persisted choices. This has not been claimed as a completed GUI test.

Existing WebView2 files (cookies/cache/IndexedDB) remain unencrypted on disk. Encrypting Aurora permission policy does not encrypt the browser's entire user-data directory.

## Official API references

- https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2permissionrequestedeventargs3
- https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2profile4

Final Windows build and `cargo clippy --all-targets --locked -- -D warnings` passed. No GUI click or visual result is claimed. The permission integration test used disposable profile `aurora-permissions-probe-e589a909-4813-4b54-924c-6e253383b21d` and exited successfully.
