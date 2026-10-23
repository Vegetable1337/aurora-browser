# Windows extension experiment

Aurora remains Rust/Wry/WebView2. This experiment lives in
`rust-browser/examples/extensions_probe.rs`, separate from the user's browser.
No extension manager is integrated into the main application yet.

## Reproduce

From the repository root on Windows:

```powershell
cargo build --manifest-path rust-browser/Cargo.toml --example extensions_probe --locked
./packaging/test-extensions-windows.ps1
```

The runner creates marked, unique `target/extensions-check-*/A` and `B` folders.
Each operation starts a new process. The executable rejects unmarked profile
roots. Data and detailed stdout/stderr plus `results.json` are retained there
for investigation. It never needs Aurora's existing profile directories.

## Implementation and measured result

WebView2 runtime tested locally: **155.0.4283.62**. The fixture uses Manifest V3,
a background service worker and a content script on an ephemeral loopback HTTP
server. The server's HTML has no JavaScript. Only the content script sets a
success title after receiving a valid worker reply. No Aurora IPC is exposed.
The experiment uses native COM callbacks and the normal Windows message loop;
it is a real WebView2 execution, not a mocked extension API.

Contrary to the older audit's assumption, Wry 0.57 already exposes environment
extension enablement and an unpacked-extension path. Management still needs COM:
`ICoreWebView2Profile7.AddBrowserExtension/GetBrowserExtensions` and
`ICoreWebView2BrowserExtension.Enable/Remove`.

The first lifecycle run passed 12 checks. The expanded run additionally installs
the same extension into B and verifies independent `storage.local` counters,
that disabling A does not disable B, and that removing A does not remove B.
The runner asserts actual callbacks, enabled state, worker replies and storage
counts; disabled execution must time out without a success title.

WebView2 lists built-in components alongside the fixture (Microsoft Clipboard
Extension and Microsoft Edge PDF Viewer in this environment). Management operations filter
by the fixture's exact name and refuse ambiguous matches, rather than modifying
the first returned extension. A production manager must use stable extension IDs.

No sandbox-disabling, TLS-bypass or Site-Isolation-disabling flags are added.
An observed test renderer had none of those flags. This is not proof of its
Windows token mitigations or a complete sandbox/security audit. Native messaging,
file access restrictions, hostile extensions and extension access to Aurora's
privileged UI still require separate verification before integration.

## Compatibility matrix

| Capability | Evidence / current status |
| --- | --- |
| MV3 background service worker | Runtime handshake passed |
| Content scripts, loopback HTTP match | Executed against actual local HTTP document |
| `chrome.runtime.sendMessage/onMessage`, asynchronous response | Passed |
| `chrome.storage.local` | Counters persisted across process restarts; profile isolation checked by runner |
| Cookies API without cookies permission | Not exposed to fixture worker |
| Install/list/enable/disable/remove unpacked extension | COM lifecycle exercised |
| Firefox extension using tested `chrome.*` common subset | Candidate compatibility only; Firefox package not installed/tested |
| Firefox `browser.*` Promise namespace | Not implemented or claimed |
| Firefox-specific APIs / signed XPI packages | Unsupported |
| Chrome action popup/toolbar badges | Not implemented by Aurora |
| CRX store installation, authenticated updates | Not implemented |
| Per-host optional permissions / revocation | Not verified |

## Integration gate

Keep privileged toolbar/settings WebViews in an extension-free environment;
only website content environments should enable extensions. Existing overlay
creation shares a content context, so enabling extensions indiscriminately is
not a safe integration. Introduce a Windows management adapter with explicit
profile lifecycle and callbacks, then a permission-review/install flow and
profile-scoped manager. Local source folders must be copied into a managed,
validated directory only with user installation consent; arbitrary source-folder
changes remove a WebView2 extension. Do not expose installation through webpage IPC.

No evidence currently requires a CEF or Chromium migration. Keep that decision
open until the remaining permissions/UI/API compatibility tests are complete.

The final marked-directory run on 2026-10-23 passed all **16** lifecycle and
cross-profile checks. Evidence was saved locally under
`rust-browser/target/extensions-check-2867cf9f66834734b460823cff130f5e/results.json`.
Native UI automation was not used to install into an end-user browser; this is a
disposable COM test executable. Windows token/sandbox mitigation tests are still
outstanding and must not be reported as passing based on these results.

## Primary references

- [Microsoft profile management API](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2profile7?view=webview2-1.0.3485.44)
- [Microsoft native management sample](https://github.com/MicrosoftEdge/WebView2Samples/blob/main/SampleApps/WebView2APISample/ScenarioExtensionsManagement.cpp)

Microsoft documents that extensions are persisted, unpacked files are not copied
into the profile, and Chrome toolbar UI entry points are absent in WebView2.
