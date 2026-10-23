# P0 audit — 2026-10-22

Historical audit snapshot. For the subsequent working MV3 experiment and
its measured limitations, see [extensions-prototype.md](extensions-prototype.md).

## Scope and current architecture

The main application is `rust-browser/src/main.rs`: Winit event loop, Wry child
WebViews, WebView2 on Windows. The Python app is a legacy reference. The older
architecture and threat-model documents describe Qt and must not be interpreted
as a security description of the Rust application.

Rust UI commands, profile lifecycle, persistence and navigation currently share
one event-loop implementation. Argon2, file dialogs and backups run synchronously
there and can freeze input. Persistence errors are discarded in several helpers;
corrupt encrypted records can appear as an empty list. These need explicit errors
and bounded background work before daily use. Do not overwrite unreadable stores.

## Confirmed defects and fixes in this stage

* Existing stores called `init_store` on every connection. If nonce metadata was
  deleted, it was recreated, allowing AES-GCM nonce reuse after records were
  deleted. A regression test reproduced this. Existing stores now open without
  CREATE and never regenerate nonce metadata. Creation and verified restore still
  initialize new stores. Missing/corrupt metadata fails writing; no automatic repair.
* `key()` returned master-key copies. It now borrows the stored key. Lock clears
  the actual stored buffer before removing it. This reduces copies but does not
  guarantee erasure of all crypto-library, OS or compiler temporaries.
* Privileged toolbar/overlay WebViews accepted navigation. Their navigation and
  popup callbacks now deny navigation away from their embedded HTML. Content
  WebViews have no Aurora IPC handler. Further frame/origin and command validation,
  CSP, and automated hostile-content integration tests remain necessary.
* The Python privacy mock lacked `resourceType`, which the real request API uses.
  The fixture now supplies the image resource type; assertions remain unchanged.

## Profile encryption and disk protection

Each vault generates an independent 32-byte random master key, wraps it with
Argon2id/AES-GCM, recovery HKDF/AES-GCM, and user-scoped Windows DPAPI. Records use
domain-specific HKDF keys and authenticated AES-GCM. Format is unchanged.

Only Aurora bookmarks/history in protected profiles are currently connected to
this store. Settings remain shared, plaintext. WebView2 cookies, cache, site data,
IndexedDB, LocalStorage, credentials, tokens and future extension data are NOT
protected by Aurora's vault. Backup does NOT include WebView2 data. DPAPI alone
does not require a separate password/biometric challenge against the logged-in user.

A safe interim at-rest deployment is an OS-managed encrypted volume containing
ALL profile/runtime data, with OS access control and shutdown before dismount.
BitLocker protects an offline volume, not a profile from its logged-in OS user.
Do not edit Chromium databases while running, or decrypt archives to an ordinary
temporary folder and call them encrypted profiles. Whole-profile password-based
protection needs a reviewed encrypted volume/filesystem and process lifecycle.

Lock/switch destroys tab and overlay objects before replacing the context and
dropping the old vault. Engine process exit and plaintext cache eviction have not
been verified. Record-operation and password-wrapping keys now use RAII
zeroization; other derived keys and decrypted buffers still need this treatment.
Restore validates archive/authentication/SQLite structure, but should additionally
authenticate all records before publishing a restored profile. Concurrent snapshot
export, rollback protection and bounded decompression also need further review.

## Engine and extensions: investigation, no migration decision yet

WebView2 has official extension APIs, including installation from a folder:
https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2profile.addbrowserextensionasync
and an environment option to enable extensions. Wry's current high-level API does
not expose a complete extension manager; Windows COM integration is an option.
Thus “WebView2 cannot run extensions” is not an established reason to migrate.

CEF Chrome runtime is an alternative, with additional distribution, sandbox,
Rust-binding and update responsibilities:
https://github.com/chromiumembedded/cef/blob/master/docs/architecture.md
A Chromium fork offers the widest browser integration but a much larger ongoing
security patch burden. Firefox WebExtensions compatibility is API-dependent, not
equivalent to running Firefox-specific APIs on Chromium.

Next gate: a separate executable/example inside this repository using a temporary
profile, a local MV3 extension with service worker and content-script handshake,
then install/list/disable/enable/remove, restart persistence, permission denial,
two-profile isolation and sandbox observations. Record runtime versions and actual
results. Compare the same fixture with CEF only if WebView2 fails requirements.
No working extension prototype or extension support is claimed at this stage.
Major engine migration requires the user's approval after this evidence.

## Security verification limits and next steps

No explicit sandbox, TLS verification or Site Isolation disabling switches were
found in application sources. This is source inspection, not runtime verification:
WebView2 environment/registry configuration and actual subprocess mitigation flags
still require inspection. No Brave filtering is connected in Rust. Permission
handling delegates to runtime defaults. HTTPS-first, third-party cookie policy,
DNS controls and fingerprinting mitigation remain unimplemented in Rust.

Added Windows CI builds, Rust vault regressions, legacy tests and cargo-audit.
Audit results must be reviewed before describing dependencies as vulnerability-free.
Linux/macOS builds are unverified and use different Wry engines; extension parity
cannot be inferred from the Windows implementation. No release or cloud account
service is created by this stage.

## Local validation results

Windows: `cargo test --offline --locked` passed 4 tests; `cargo build --offline
--locked` succeeded. `cargo fmt --check` and `git diff --check` passed. The nonce
metadata regression failed before the fix and passed after it. Existing `.venv`
Python: all 19 unittest tests passed, including DPAPI. System Python lacked the
required packages, so it was not used for the final legacy checks.

`cargo-audit` was not installed initially. Its installation was interrupted after
prolonged dependency downloads blocked Cargo verification. No local dependency
advisory result is available yet; the added CI workflow has not run locally.
No GUI startup, sandbox runtime or extension execution success is claimed.

## Follow-up: blank toolbar regression

The initial navigation restriction incorrectly assumed `with_html` would only
navigate to `about:blank`. Runtime diagnostics showed an initial `data:` URL,
so the toolbar document was rejected. Toolbar/overlay navigation now decodes
bounded HTML data URLs and accepts only bytes identical to their compiled-in
document. Arbitrary HTML data URLs, HTTP(S), JavaScript URLs and blank navigation
are rejected; popup denial remains in place. The regression test covers both
percent-encoded and base64 HTML plus rejected foreign documents.

Windows build and the targeted navigation regression passed. After launch,
Windows accessibility inspection confirmed the tab, new-tab button, address bar,
navigation controls and settings button exist again. Screenshot capture timed
out and button input failed because geometry was unavailable, so interactive
new-tab/menu smoke checks have not been confirmed through this tool.

## Follow-up: Windows security CI audit failure (2026-10-23)

GitHub Actions runs 1–5 of `Windows security regression` were inspected. Rust
format, unit tests, UI syntax and Windows build steps succeeded; the legacy
Python job also succeeded in each run. The only failing step was `cargo audit`.
Runs 1–5 reported `RUSTSEC-2026-0384` for Tokio 1.54.0 and three ZIP extraction
advisories (`RUSTSEC-2026-0367`, `-0368`, `-0369`) for zip 2.6.1. Tokio is now
locked at 1.54.1 and the backup archive dependency is upgraded to zip 9.0.5.
The backup reader handles the new fallible entry-name API by rejecting invalid
names as `InvalidBackup`; the existing encrypted backup format is unchanged.

Local validation after the dependency update: `cargo fmt --check`, `cargo test
--locked` (21 tests), `cargo clippy --all-targets --locked -- -D warnings`,
`cargo build --locked`, `cargo build --example extensions_probe --locked`,
`node packaging/check-ui-syntax.cjs`, and `python -m unittest discover -s tests
-v` (19 tests) passed. `cargo audit` exited successfully with three advisories
reported as non-failing warnings: unmaintained `proc-macro-error` and
`ttf-parser`, and the existing `glib::VariantStrIter` unsoundness advisory.
These warnings are not suppressed. The new GitHub Actions run for the fix must
complete before CI is reported green.
