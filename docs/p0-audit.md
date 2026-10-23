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
These warnings are not suppressed. The initial audit failures were subsequently
resolved by `ae94e48`; runs on later commits were checked below.

## Follow-up: fail-closed profile activation and WebView2 extension restart probe (2026-10-23)

Protected profile activation now loads and decrypts all bookmark/history records
and prepares the profile's WebView2 data directory before destroying the current
profile's tabs/context. Decryption or JSON corruption fails activation instead of
silently presenting empty data. A regression test writes an authenticated but
invalid JSON record to a disposable vault and confirms loading fails. WebView
construction and site-permission policy attachment now report recoverable errors
instead of panicking; a page is not loaded when permission protection could not
be attached.

The Windows WebView2 MV3 probe exposed a race after environment restart: persisted
extension enumeration was still pending when the next page navigation began. The
probe now waits for `GetBrowserExtensions` and confirms the local fixture exists
before navigation. A fresh 16-check run passed on WebView2 Runtime 155.0.4283.62,
including extension worker/content-script handshakes, enable/disable/remove,
restart persistence, and profile A/B isolation. Evidence and limits are recorded
in `docs/extensions-prototype.md`; this is a native WebView2 probe, not a visual
test of Aurora's main window.

Post-change checks passed locally: `cargo fmt --check`, `cargo test --locked`
(22 tests), `cargo clippy --all-targets --locked -- -D warnings`,
`cargo build --locked`, `node packaging/check-ui-syntax.cjs`, and the 19 legacy
Python unittests. GitHub Actions run [39940708619](https://github.com/Vegetable1337/aurora-browser/actions/runs/39940708619)
completed successfully: formatting, tests, UI syntax, probe build, Windows app
build, dependency audit, and the legacy Python job all passed.

## Follow-up: protected profile auxiliary state (2026-10-23)

Profile activation also validates encrypted site-permission policy, adblock site
exceptions, and download history before switching away from the active profile.
Missing records remain compatible with older profiles and use safe defaults;
present records that cannot be decrypted or decoded now abort activation. This
prevents corrupted data from silently erasing explicit permission or privacy
choices. The regression suite now covers malformed encrypted bookmark and
permission-policy JSON records.

Verification after this change: `cargo fmt --check`, `cargo test --locked`
(23 tests), `cargo clippy --all-targets --locked -- -D warnings`, and
`cargo build --locked` passed. GitHub Actions runs for `808e5d7` and `afeb144`
also completed successfully.

## Windows 0.1.0 Alpha preparation (2026-10-23)

### P0 — HEAD, CI, dependency audit

At the start of this stage, local `HEAD` and `origin/main` were both `afeb144` and
the worktree was clean. GitHub Actions run
[39943350716](https://github.com/Vegetable1337/aurora-browser/actions/runs/39943350716)
for that exact SHA completed successfully. Earlier failed runs on `b8b5bb4`,
`bef1610`, `2660b30`, and `33897ef` failed only in `cargo audit`; the Rust tests,
builds and legacy Python jobs shown in their job summaries completed. The logs
identify yanked/vulnerable Tokio 1.54.0 and vulnerable zip 2.6.1. The dependency
update in `ae94e48` moved to Tokio 1.54.1 and zip 9.0.5; Actions run
[39940708619](https://github.com/Vegetable1337/aurora-browser/actions/runs/39940708619)
and the later runs for `808e5d7` and `afeb144` all passed. The audit still emits
three allowed warnings for Linux-only backend dependencies, listed below.

`cargo audit` exits 0 with three allowed warnings:

- `proc-macro-error` is unmaintained and enters through GTK/GLib macros for the
  Linux WebKit backend; it is not in the Windows target dependency graph.
- `ttf-parser` is unmaintained and enters through `sctk-adwaita` for the Linux
  winit backend; it is not in the Windows target dependency graph.
- `glib::VariantStrIter` has an unsound iterator advisory in the Linux GTK/WebKit
  backend. Aurora does not call that API directly. The warning remains relevant
  to a future Linux build and is not suppressed.

The warning paths were checked with `cargo tree --locked --target all -i ...`.
These are not known exploitable findings in the Windows Alpha target, but the
Linux backend must not be described as security-cleared.

### P1/P2 — installer and release package

Selected Inno Setup 6.7.3 for a per-user install under
`%LOCALAPPDATA%\Programs\Aurora Browser`. The stable `AppId` supports installing
over the same application; user profiles remain under `%LOCALAPPDATA%\Aurora\Rust`,
outside the install directory, and no uninstall-delete rule targets them. Setup
adds a Start menu entry, offers a desktop shortcut unchecked by default, does not
add a Run-at-startup entry, and requires no elevation. It checks the documented
WebView2 `pv` registry value; if absent, it opens Microsoft's official download
page and exits. Runtime detection follows Microsoft's documented registry paths:
[WebView2 distribution and runtime detection](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution).

The executable version is now `0.1.0-alpha`; PE metadata was inspected and reports
ProductName `Aurora Browser`, ProductVersion `0.1.0-alpha`, FileVersion `0.1.0.0`,
and OriginalFilename `Aurora.exe`. `cargo build --release --locked` succeeded.
Inno Setup 6.7.3 compiled the installer without warnings after setting the
architecture to `x64compatible`. A per-user installation, Start menu and desktop
shortcuts, repeat installation, and uninstall were exercised in an isolated
temporary directory; the directory and shortcuts were removed. This did not use
a clean Windows VM and does not prove the missing-WebView2 branch or preservation
of a real existing user profile.

The portable ZIP was inspected and contains exactly `Aurora.exe`, `LICENSE`, and
`README.txt`; it contains no tests or user profiles. `dist/alpha/SHA256SUMS.txt`
contains SHA-256 hashes for both the installer and ZIP. The release workflow now
uses `workflow_dispatch` only and uploads artifacts; it has no release-publish
step or write permission. No GitHub Release was created.

### P3/P4/P6/P7 — compatibility test and user docs

A 1,553-byte inner ZIP fixture was generated with the previous `zip 2.6.1` library.
The new `restores_profile_backup_payload_written_by_zip_261` test wraps that old
payload in the unchanged authenticated backup envelope and restores a real
encrypted vault record. Both this test and the current-format backup round trip
pass. The full current Windows GUI checklist is in `docs/alpha/manual-checklist.md`;
it is explicitly a plan, not claimed GUI evidence. No clean Windows VM was
available for the missing-runtime installer path, first-run visual checks, or
profile-preservation install/uninstall check.

Windows Alpha docs include install/update/remove instructions, requirements,
supported and experimental capabilities, known security limits, and backup
recommendations. `CHANGELOG.md` and the root README link to them.

### Final Release launch smoke

The first Release launch smoke exposed a real packaging defect: the optimized
EXE still had PE subsystem `3` (console), so an unwanted console window appeared.
`#![windows_subsystem = "windows"]` alone did not change the linked executable
in this setup. `build.rs` now adds `/SUBSYSTEM:WINDOWS` for the Windows Release
binary only; Debug test harnesses remain console applications. The final EXE was
inspected and reports PE subsystem `2` (Windows GUI). The bounded smoke test ran
the Release EXE with a temporary profile and a PATH containing Windows system
directories only, found a visible `Aurora` HWND, found no visible console HWND,
sent `WM_CLOSE`, and observed exit code 0. This checks process launch and window
lifecycle, not interaction with page controls. Packages and SHA-256 sums were
regenerated after the subsystem fix.

Final local checks: `cargo fmt --check`, `cargo test --locked` (24 tests),
`cargo clippy --all-targets --locked -- -D warnings`, UI/MV3 fixture JavaScript
syntax check, `cargo audit` (three documented Linux-backend warnings), and
`python -m unittest discover -s tests -v` (19 tests) passed. The final optimized
Release build succeeded. Installer install/update/uninstall was exercised in an
isolated temporary directory earlier in this work; it was not rerun after the
subsystem-only binary change. The missing-WebView2 branch and clean-VM behavior
remain unverified.
