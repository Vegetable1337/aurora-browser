# Windows Alpha verification — 2026-10-23

## Native WebView2 extension probe

Ran `packaging/test-extensions-windows.ps1` from the current Alpha-preparation
commit on Windows with WebView2 Runtime `155.0.4283.62`. All 16 scripted checks
passed across separate disposable profiles A and B. The probe creates a fresh
process per operation and uses the repository's local MV3 fixture.

Verified behaviors: install and enumerate the extension, worker/content-script
handshake on a local test page, extension state persistence across process
restarts, independent extension state in A and B, disable/enable behavior,
expected non-response while disabled, and removal without affecting profile B.
The negative disabled-extension probe exited with its expected timeout result;
that check passed because the extension did not respond. Raw command results
are saved in `mv3-webview2-results.json`.

This exercises WebView2 APIs in the native probe. It does not prove that Aurora's
visual extension manager or browser UI was interacted with. Chrome Web Store and
Firefox Add-ons distribution are not enabled in this Alpha.

## Windows Alpha packaging

- Release EXE PE subsystem is `2` (Windows GUI); embedded product/version/icon
  metadata was inspected.
- Release launch smoke used a disposable `LOCALAPPDATA`, removed Rust toolchain
  directories from `PATH`, found a visible Aurora window without a console, and
  observed clean exit after Win32 `WM_CLOSE`.
- Inno Setup 6.7.3 compiled the per-user installer. Earlier isolated smoke
  exercised install, repeat install, Start menu/desktop shortcut handling, and
  uninstall. It did not use a clean Windows VM, and the install smoke was not
  rerun after the later PE-subsystem-only change.
- The portable ZIP was inspected: exactly `Aurora.exe`, `LICENSE`, and `README.txt`.
  It contains no profiles, tests, fixture source, or credentials.
- SHA-256 hashes are in the generated `dist/alpha/SHA256SUMS.txt`.

## Automated checks on the pushed commit

GitHub Actions run [39948687861](https://github.com/Vegetable1337/aurora-browser/actions/runs/39948687861)
completed successfully for commit `06ae451`. Both the Rust and legacy Python
jobs passed, including format, tests, Clippy, UI JavaScript syntax, MV3 probe
build, Windows build, dependency audit, and Python tests.

Local validation also passed `cargo build --release --locked`, the Release
launch smoke, UI syntax, `cargo audit`, and 19 Python unittests. `cargo audit`
reports three non-failing warnings for the future Linux GTK/WebKit backend; see
`../p0-audit.md`. No clean-VM install, no-WebView2 installer branch, or visual
GUI regression was run. Use `manual-checklist.md` for those checks.
