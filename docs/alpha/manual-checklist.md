# Aurora Browser 0.1.0 Alpha — manual Windows checklist

Run this checklist on a disposable Windows user account or VM. Back up any
profile you care about before installing a new build. Do not use real browser
profiles for destructive checks.

## Install and update

- [ ] Install with the default options as a standard user; confirm no admin prompt.
- [ ] Confirm Start menu entry, application icon, and optional desktop shortcut.
- [ ] Install a newer build over the existing one; confirm app files update.
- [ ] Uninstall through Windows Settings; confirm shortcuts and application files
  are removed while `%LOCALAPPDATA%\Aurora\Rust` remains.
- [ ] On a clean VM without WebView2 Runtime, confirm setup shows the requirement,
  opens Microsoft's official download page, and does not install a broken app.

## Core browser

- [ ] Launch from Start menu and portable ZIP; open and close tabs, including the
  last tab; navigate using address bar, back, forward, reload and search.
- [ ] Create a bookmark, revisit it, inspect history, then clear history.
- [ ] Open a private tab, browse, close it, and confirm its visit is not in history.
- [ ] Resize to a narrow window; inspect tab strip, menus, settings, privacy and
  download panels. Repeat at Windows scaling 125%, 150%, and 200%.
- [ ] Switch light/dark themes, test keyboard focus, and reduce/disable animation.

## Privacy, profiles, and downloads

- [ ] Visit a known HTTPS site; inspect the Privacy Center and real blocked count.
- [ ] Add/remove a site filter exception and confirm it is profile-local.
- [ ] Test camera/microphone permission prompt on a site that requests it; confirm
  Ask/Allow/Deny are scoped to the exact origin and reset works.
- [ ] Inspect and remove cookies/site data for one test site; verify another site
  remains signed in. Confirm the UI describes unsupported cleanup types honestly.
- [ ] Enter an HTTP URL without a scheme; confirm HTTPS is tried first and any
  fallback to HTTP requires an explicit confirmation. Verify localhost remains usable.
- [ ] Create two password-protected test profiles, add different bookmarks and
  permissions, switch between them, lock/unlock each, and try a wrong password.
- [ ] Export an encrypted backup, restore it to a separate test profile, and
  compare Aurora-owned records. Verify the backup is rejected after tampering.
- [ ] Download a harmless file; inspect progress, cancellation, completed history,
  open-file/open-folder controls, and retained history after restart.
- [ ] Close Aurora normally; relaunch and confirm state persists as expected.

## Extensions (local test fixture only)

- [ ] Install the repository's local MV3 fixture into a disposable profile.
- [ ] Confirm it runs on ordinary pages, cannot control Aurora's privileged UI,
  can be disabled/enabled/removed, and does not appear in another profile.
- [ ] Do not use Chrome Web Store or Firefox add-ons; these are not supported in
  this alpha.

Record Windows version, Runtime version, display scaling, and failures before
reporting a regression. This checklist is a manual test plan, not evidence that
the GUI scenarios have already passed.
