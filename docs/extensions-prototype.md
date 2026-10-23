# Windows WebView2 extension integration

Aurora remains Rust/Wry/WebView2. The initial native experiment remains
available in `rust-browser/examples/extensions_probe.rs`; a profile-scoped
manager is now integrated into the main application's Library menu. Code lives
in `rust-browser/src/extensions.rs` and the UI in `rust-browser/ui/overlay.html`.

## Main application status

- **Implemented:** separate extension-free WebView2 context for Aurora's toolbar
  and privileged Library/settings overlay. Ordinary website tabs use a separate
  context with extensions enabled. Private tabs are created with extensions
  disabled. Switching/locking drops tabs and the previous profile context.
- **Implemented:** local folder selection; Manifest V3 metadata validation;
  bounded copying into profile-owned `Extensions/` storage; rejection of
  symlinks/reparse points; explicit permission and host-access confirmation;
  profile-local ID registry; list, enable, disable and remove by exact WebView2
  extension ID. The UI operations only arrive through the trusted toolbar or
  extension-free overlay IPC, never through website tabs.
- **Tested:** manifest v2 rejection and managed-copy independence are unit tests.
  Main app and the standalone native probe compile on Windows. UI script syntax
  and all Rust tests passed in the current checkout.
- **Not yet tested in the main window:** end-to-end install/enable/disable/remove
  through the manager, hostile content-script attempts against Aurora UI,
  service-worker termination after profile lock, action popups, and installer
  runtime compatibility. The 16-check result below belongs to the isolated
  disposable probe, not the main-app manager.
- **Unsupported/unverified:** full Chrome API compatibility, Chrome Web Store
  install/update, Firefox package/API compatibility, per-host runtime permission
  management, and extension toolbar/action popup integration. Aurora displays
  permissions declared in the manifest; WebView2 remains responsible for its
  own API permission enforcement.

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

Wry 0.57 exposes environment extension enablement and an unpacked-extension
path. Management uses COM:
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
file access restrictions and hostile extensions still require separate
verification. Main-app UI separation is implemented but hostile injection has
not yet been tested end-to-end.

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
The integrated manager copies packages into a profile-owned, validated directory
only after the user selects a folder; it then presents permissions from the
staged copy for explicit consent. WebView2 does not copy those files itself, so
the managed files must remain available for as long as the extension is
installed. Aurora does not expose installation through webpage IPC.

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
# Windows WebView2 extension probe — 2026-10-23

The 16-check run completed against the native WebView2 runtime 155.0.4283.62
using isolated temporary profiles A and B. It verified local MV3 installation,
enumeration, content-script/service-worker execution, persisted state across
WebView2 process restarts, enable/disable/remove, and profile isolation. A disabled
extension produced no handshake and was reported as the expected 20-second
negative probe; enabling it restored the handshake. Profile B retained its own
extension after the copy in A was removed.

The first run exposed a restart race: the probe navigated immediately after
creating a WebView2 environment, before the profile's extension enumeration had
completed. The harness now waits for `GetBrowserExtensions` to enumerate the
fixture and read its enabled state before navigating. The repeated full run
passed all 16 checks. Result JSON and per-operation output were retained under
the ignored disposable directory `rust-browser/target/extensions-check-ba5598dd4b2c45d8acd60f31a3f1a900/`.

This is a real WebView2 API integration test through the standalone probe
harness. It does not claim that the Aurora main-window GUI was visually tested.
