# Third-party browser sources

Aurora uses upstream projects as dependencies or engineering references. It does not copy another browser's full application source. This inventory names direct dependencies and major engine/runtime components; transitive packages are recorded in `requirements.txt` and `rust-browser/Cargo.lock` and may have additional notices.

## Python application

- **PySide6 / Qt WebEngine** — <https://code.qt.io/cgit/qt/qtwebengine.git/>; Qt libraries are available under LGPLv3/GPLv3 or a commercial license, subject to the relevant module and distribution terms. Qt WebEngine embeds Chromium and has additional notices. Confirm the exact obligations for the package being distributed.
- **cryptography** — <https://github.com/pyca/cryptography>; Apache-2.0 OR BSD-3-Clause.
- **argon2-cffi** — <https://github.com/hynek/argon2-cffi>; MIT.
- Python transitive dependencies and their installed license metadata are resolved from `requirements.txt`.

## Brave adblock-rust

- Source: <https://github.com/brave/adblock-rust>
- License: Mozilla Public License 2.0 (MPL-2.0)
- Rust crate `adblock` is pinned in `rust-browser/Cargo.toml`/`Cargo.lock` and powers HTTP(S) request matching in WebView2. The Rust app fetches EasyList and EasyPrivacy over HTTPS at startup and has a built-in fallback rule set.
- Python bindings: <https://github.com/ArniDagur/python-adblock>, pinned in `requirements.txt` as `adblock==0.6.0`; used by `app/privacy.py`.
- The adblock-rust upstream is MPL-2.0; Python bindings are MIT OR Apache-2.0. Review all direct and transitive license obligations before distributing a built application.

No upstream implementation source files are copied into Aurora.

## qutebrowser

- Source: <https://github.com/qutebrowser/qutebrowser>
- License: GPL-3.0-or-later
- Used in Aurora: architectural reference for a Python + Qt WebEngine browser. No source code was copied.

## Chromium

- Source: <https://github.com/chromium/chromium>
- License: Chromium's repository is BSD-3-Clause with additional third-party notices and licenses.
- Used in Aurora: embedded through Qt WebEngine; Aurora does not build a Chromium fork.

## Wry and winit

- Sources: <https://github.com/tauri-apps/wry> and <https://github.com/rust-windowing/winit>
- Licenses: Apache-2.0 OR MIT
- Used in Aurora: Rust browser window and Chromium-based WebView2 integration in `rust-browser/`.
- Aurora uses the published Rust crates; no browser source files are copied.

## Other direct Rust dependencies

- **serde**, **serde_json**, and **url** — <https://github.com/serde-rs/serde> and <https://github.com/servo/rust-url>; MIT OR Apache-2.0.
- **image** — <https://github.com/image-rs/image>; MIT OR Apache-2.0. Aurora uses its PNG decoder for the app icon.
- **winres** — <https://github.com/mxre/winres>; MIT. It embeds the Windows icon resource during a Windows build.
- **ureq** — <https://github.com/algesten/ureq>; MIT OR Apache-2.0. Aurora uses its Rustls-backed HTTPS client to update bounded filter-list files.
- WebView2 is a Microsoft runtime installed separately on Windows. Its redistribution and runtime terms are governed by Microsoft, not this repository's MIT license.

## Distribution notes

When distributing Aurora, include the license and notice files required by Qt/PySide6, Chromium/Qt WebEngine, Brave adblock-rust, Python bindings, and all transitive dependencies. This source inventory is not a complete generated bill of materials; produce and review one before distributing executable builds. The Aurora source files are offered under the MIT license in `LICENSE`; that license does not replace third-party licenses.
