# Aurora architecture

## Current primary application

The Windows desktop browser is implemented in Rust with Wry and the installed Microsoft Edge WebView2 Runtime. The older Python/Qt application is retained as a reference and is not the primary executable. Aurora does not build or bundle Chromium.

The main process owns the native window, toolbar, browser tabs, profile switching and privileged UI commands. Each ordinary profile has a distinct WebView2 user-data directory. Web pages use a persistent WebView2 context; private tabs use WebView2 incognito mode. Profile locking drops the current tabs and context before activating the default profile.

The privileged toolbar and library/settings/extension-manager overlay use their own persistent WebView2 context with extensions disabled. Website tabs use a separate context with WebView2 extensions enabled. This separation is required because content scripts execute in the browser context and must not be able to reach Aurora's privileged IPC surface. Both privileged WebViews accept navigation only to their compiled-in HTML payload and reject popups.

## Extension management

On Windows, Aurora supports local unpacked Chrome Manifest V3 packages through WebView2's profile extension APIs. The manager stages a bounded copy in the current profile's `Extensions` directory, presents the copied manifest's API and host permissions for explicit confirmation, and then calls `ICoreWebView2Profile7.AddBrowserExtension`. Management operations match the WebView2 extension ID against an Aurora-owned per-profile registry before enabling or removing it. Normal profiles have separate WebView2 directories and extension state; private tabs do not load extensions.

This is a limited WebView2 integration, not Chrome or Firefox compatibility. WebView2 lacks Chrome's browser chrome integration (for example, a full extension toolbar/action popup experience) and exposes no Aurora implementation of every Chrome permission/API. Firefox packages and Firefox-specific APIs are not supported. Do not install untrusted extensions. Aurora's UI lists permissions declared in the manifest; it does not mediate every runtime API request or replace the engine's own extension permission checks.

See [the implementation and test status](extensions-prototype.md). The native probe exercises service workers, content scripts, messaging, storage persistence and cross-profile isolation, while the main application manager has compile/unit coverage. Full extension-manager lifecycle tests against real Aurora windows, hostile extension tests, popup/action behavior and operating-system sandbox verification remain outstanding.

## Network privacy filtering

On Windows, every regular and private page WebView receives an HTTP/HTTPS `WebResourceRequested` handler. It maps the WebView2 resource context to Brave `adblock-rust` request types, evaluates the destination against the current top-level source URL, and returns an empty 204 response only for requests the engine marks blocked. The UI counter increments only after such a match. A per-profile host exception bypasses the engine for requests whose source host equals that site; protected-profile exceptions are records in the encrypted vault.

EasyList and EasyPrivacy are fetched at startup from fixed HTTPS URLs using a bounded response reader. A redirect must remain HTTPS and on the expected host; each list must be <=12 MiB and pass a basic ABP structure check. Valid snapshots are atomically cached. On network/validation failure the last valid cache stays in use; without a cache Aurora has a small built-in fallback list. Fetching, parsing and engine construction run on a worker thread, then a read/write lock swaps the fully built engine snapshot. The request callback does only request construction and matching.

WebView2 exposes resource context and request URL to this handler but the current implementation obtains source context from the WebView's top-level `Source`, including for subframes. Thus it does not have precise iframe initiator context. It filters HTTP(S) requests surfaced by WebResourceRequested, not DNS, runtime-internal traffic or cosmetic DOM elements. It is network filtering, not a privacy/anonymity guarantee. Automatic list refresh currently happens on browser startup; there is no user-configurable schedule or list subscription UI.

## Data protection boundary

Protected Aurora profiles encrypt Aurora-owned bookmarks and visit records. WebView2 stores cookies, cache, site data, extension data and other Chromium profile state in separate profile directories that are not encrypted by Aurora. The `Extensions` source copy and its permission registry are also ordinary files under that profile directory. This architecture must not be described as whole-profile encryption.

Profile switching destroys Aurora's tab views and drops the old context before replacing the active vault. This prevents a later Aurora IPC command from targeting the old active tab. It does not prove that every WebView2 extension service worker or renderer process has terminated, nor does it guarantee secure erasure from memory, swap or disk.

## Other engine options

No migration to CEF or a Chromium fork is approved or required by current evidence. Reconsider only if a documented WebView2 limitation blocks a required, security-relevant feature and a tested prototype establishes the alternative's behavior, maintenance cost and update model.
