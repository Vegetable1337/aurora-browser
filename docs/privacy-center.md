# Privacy Center and origin permissions (2026-10-23)

The first implementation is available in Settings → Privacy. It retains Aurora's existing glass panel, light/dark themes and reduced motion support.

## Implemented

- Displays the current HTTP(S) origin and explicitly distinguishes HTTP from an HTTPS URL. This is not a certificate viewer or a mixed-content audit; the UI states that those details have not been retrieved. TLS verification remains the engine's responsibility and is not bypassed.
- Shows separate profile and current-origin blocking counters. Counts are acknowledged only after WebView2 accepts the replacement response, not merely after an ABP match. Subframe attribution uses the top-level WebView Source, as in the existing filter.
- Site filter exceptions use the existing host-based policy. The UI continues to label this as a site/host exception, not a permission origin. A private tab gets independent in-memory filter exceptions and counters.
- Camera, microphone, geolocation and notifications support Ask / Allow / Deny. Settings are keyed by normalized URL origin: scheme, hostname and non-default port; paths and default ports do not create distinct entries. Other permission kinds are denied.
- WebView2 PermissionRequested enforces the policy for each requesting origin, including frame-origin requests. Ask presents a native Yes/No prompt for that request. No sensitive access is granted by default. OS-level privacy restrictions can still deny an allowed request.
- WebView2 SavesInProfile is false: permission policy is saved only in Aurora. Default-profile settings use atomic JSON replacement; protected profiles use the existing encrypted vault under the site-permissions/policy record. Private-tab policy remains in memory and starts from Ask.
- Before permitting HTTP(S) navigation in a new content view, Aurora enumerates and resets legacy native permission decisions using Profile4. Existing supported choices are imported into Aurora policy without overriding an explicit Aurora choice; a UUID snapshot of native settings is saved (encrypted for protected profiles) before clearing the native entries. No new plaintext native Allow decisions are stored. If initialization fails, network navigation stays blocked and stderr identifies the failure.
- Setting/resetting checks the active tab ID and origin to reject delayed commands after navigation/profile switching. Successful changes reload matching tabs so existing media streams close. Closing or switching a profile drops its tabs and policy references.

## Checks actually executed

- `cargo test --locked`: 15 tests passed: 14 shared-component tests plus one internal-UI navigation test. Tests cover counter acknowledgment and legacy migration without overwriting an explicit choice.
- Added origin/scheme/port/reset validation and encrypted policy round-trip, independent profile and locked-vault tests.
- `cargo run --example site_permissions_probe --locked`: actual WebView2 HTTP loopback page requested geolocation and received PERMISSION_DENIED; the test also asserted that the production Aurora permission callback was actually invoked. Uses a new UUID temporary profile, no user data and a 15-second watchdog. This is an API integration test, not a mouse-driven GUI result.
- `node --check` on extracted inline scripts: chrome.html and overlay.html passed.

## Remaining stages and manual checks

Cookie listing/deletion, origin storage cleanup, cache clearing, certificate details, HTTPS-first failure handling and download history are not implemented by this change. No placeholder controls are shown for these operations. A complete Privacy Center including site-data management remains pending.

GUI checks still required: navigate two HTTPS origins with distinct ports; open Settings → Privacy; change each selector and confirm reload; verify a native Ask prompt; repeat in a protected and a private tab; switch/lock profiles and verify defaults and persisted choices. This has not been claimed as a completed GUI test.

Existing WebView2 files (cookies/cache/IndexedDB) remain unencrypted on disk. Encrypting Aurora permission policy does not encrypt the browser's entire user-data directory.

## Official API references

- https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2permissionrequestedeventargs3
- https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2profile4

Final Windows build and `cargo clippy --all-targets --locked -- -D warnings` passed. No GUI click or visual result is claimed. The permission integration test used disposable profile `aurora-permissions-probe-e589a909-4813-4b54-924c-6e253383b21d` and exited successfully.
