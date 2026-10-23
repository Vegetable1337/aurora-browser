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

## UI Automation discovery

`packaging/test-ui-automation-smoke.ps1` launched the real Release app with a
disposable data directory and exposed a visible `Aurora` window to Windows UI
Automation. The tree contained 38 nodes, but the WebView2 contents appeared as
opaque panes; the only invokable controls were the native minimize, restore,
and close buttons. UI Automation did not expose the toolbar, tabs, menus, or page
controls as actionable elements. This is why the remaining interactions require
the manual checklist; the earlier coordinate-input problem was not retried.
The observed UIA tree is saved in `evidence/ui-automation-window.json`. The test
closed the real window with the UIA Window pattern, waited for exit, and removed
its disposable profile using bounded retries for delayed WebView2 file release.

## Visible window capture

`packaging/capture-alpha-window.ps1` launches the actual Release executable with
a disposable profile, waits for the visible Aurora window, captures the desktop
window, closes it through UI Automation, and removes the disposable data. The
captured `evidence/main-window.png` shows the rendered Aurora new-tab page.
The initial capture exposed a first-paint race: WebView2 reported load completion
before its first composed frame, so Aurora showed its window over an empty dark
viewport. Startup now keeps the top-level window hidden for one second after
making the loaded first tab visible, then shows it through a separate event so
the UI thread does not sleep. Three immediate captures after the window became
visible showed the complete page. This adds up to one second to initial window
presentation; the existing 12-second fallback still shows the window if startup
does not complete. The capture script can seed only its disposable profile with
light or dark theme settings; both actual window captures rendered the expected
Aurora start page (`main-window.png` and `main-window-light.png`). The capture
can also resize the actual window through Win32 to its declared 720x480 minimum.
That exposed a second layout issue: the embedded page subtracted the 104px
browser toolbar from its own viewport a second time. The page now uses its full
WebView viewport and a compact-height layout. Dark and light captures at 720x480
show the search form, all four shortcuts, and footer without vertical or
horizontal scrolling (`minimum-window.png` and `minimum-window-light.png`).
The captures verify rendering only; UIA still cannot operate the HTML controls,
so they do not replace the manual interaction checklist. DPI 125/150/200,
reduced-motion transitions, and small-window menus remain manual checks.

## Windows Alpha packaging

- Release EXE PE subsystem is `2` (Windows GUI); embedded product/version/icon
  metadata was inspected.
- Release launch smoke used a disposable `LOCALAPPDATA`, removed Rust toolchain
  directories from `PATH`, found a visible Aurora window without a console, and
  observed clean exit after Win32 `WM_CLOSE`.
- Inno Setup 6.7.3 compiled the per-user installer. A final smoke on the current
  installer passed first install, repeat install, Start menu shortcut and Windows
  uninstall registration checks, and cleanup through its uninstaller. It used a
  unique custom install directory under `%TEMP%`, refused to run if an Aurora
  process or installation was present, and confirmed the standard profile path
  is outside the install path. It did not create a disposable Windows account,
  so preservation of actual profile contents was not exercised. Detailed output
  is saved in `evidence/installer-smoke.json`.
- The portable ZIP was inspected: exactly `Aurora.exe`, `LICENSE`, and `README.txt`.
  It contains no profiles, tests, fixture source, or credentials.
- SHA-256 hashes are in the generated `dist/alpha/SHA256SUMS.txt`.

## Automated checks on the pushed commit

GitHub Actions runs [39948687861](https://github.com/Vegetable1337/aurora-browser/actions/runs/39948687861),
[39951158572](https://github.com/Vegetable1337/aurora-browser/actions/runs/39951158572),
[39954380715](https://github.com/Vegetable1337/aurora-browser/actions/runs/39954380715),
[39957084924](https://github.com/Vegetable1337/aurora-browser/actions/runs/39957084924),
and [39962596138](https://github.com/Vegetable1337/aurora-browser/actions/runs/39962596138)
completed successfully for the pushed Alpha-preparation commits. Both
Rust and legacy Python jobs passed, including format, tests, Clippy, UI
JavaScript syntax, MV3 probe build, Windows build, dependency audit, and Python
tests.

After run 39962596138 passed on commit `5f08d77`, a local responsive-layout fix
was rebuilt and checked: `cargo fmt --check`, 24 Rust tests, Clippy, Release
build, UI JavaScript syntax, actual dark/light window captures, Release launch
smoke, and installer lifecycle smoke all passed. The layout change is now in the
worktree and its new commit still needs its own Actions run.

### Light-theme contrast follow-up

Contrast inspection found several muted labels and placeholders below the WCAG
AA 4.5:1 text contrast target. The light theme now uses a darker secondary text
color across the new-tab page, toolbar, and overlay. Success, warning, and error
states in extension/download cards also have explicit light-theme colors. The
updated actual Release screenshots were captured in both themes at maximized and
720x480 minimum window sizes; the minimum light screenshot confirms the search,
four shortcuts, and footer remain visible. Contrast ratios for the adjusted text
pairs range from 4.76:1 to 5.26:1. UI/MV3 JavaScript syntax validation passed.

The current local worktree passed `cargo fmt --check`, `cargo test --locked`
(24 Rust tests), `cargo clippy --all-targets --locked -- -D warnings`,
`cargo build --release --locked`, 19 Python unittests, UI/MV3 JavaScript syntax,
and `cargo audit`. Audit completed with three warnings for unmaintained or
unsound transitive GTK/WebKit-related dependencies (`proc-macro-error`,
`ttf-parser`, and `glib`); no advisory was reported as a vulnerability requiring
an immediate Windows-path update. The full security audit is in
[`p0-audit.md`](../p0-audit.md).

The installer and portable archive were rebuilt from that Release binary. The
installer smoke passed first install, repeat install, Start menu shortcut,
uninstall registration, and uninstall. Release launch smoke confirmed a visible
window and clean `WM_CLOSE` exit without Rust tools on `PATH`. Windows retained
one disposable WebView2 profile under `%TEMP%` after shutdown, so temporary-data
cleanup was not fully clean in that run. The portable archive contains only
`Aurora.exe`, `LICENSE`, and `README.txt`. Current artifact hashes:

```text
b6d7876897e288cf40dfb68dd760145d1c40e8c5b422f070a97987e0eefe44ad  Aurora-0.1.0-alpha-Windows-x64-Setup.exe
aa941c599e57d1e183513a1731cf50cf91a4b73da956bd093e8a204f0d91dc77  Aurora-0.1.0-alpha-Windows-x64-Portable.zip
```

GitHub Actions run [39965491576](https://github.com/Vegetable1337/aurora-browser/actions/runs/39965491576)
for commit `44a01f0` passed its Rust formatting, tests, UI syntax, WebView2 probe
build, Windows build, and Python job at the latest check. `cargo install
cargo-audit --locked` was still running, so that run is not yet recorded as
successful. The light-theme contrast follow-up is local and requires a new CI
run after it is committed and pushed.

Local validation also passed `cargo build --release --locked`, the Release
launch smoke, UI syntax, `cargo audit`, and 19 Python unittests. `cargo audit`
reports three non-failing warnings for the future Linux GTK/WebKit backend; see
`../p0-audit.md`. No clean-VM install, no-WebView2 installer branch, or visual
GUI regression was run. The screenshot checks cover rendering but not
interaction. No clean-VM install, no-WebView2 installer branch, DPI 125/150/200,
or menu/panel GUI check was run. Use `manual-checklist.md` for those checks.
