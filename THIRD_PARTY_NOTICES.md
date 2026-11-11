# Third-party license notices for Aurora Browser

Aurora's own source code is licensed under MIT (see `LICENSE`). This file acknowledges software and resources used by Aurora that are **not** relicensed by Aurora's MIT license. A library being linked or a runtime installed separately does not mean Aurora has copied that library's source files.

## Components used by the Windows Rust release

| Component | How used | Upstream and license |
| --- | --- | --- |
| Brave adblock-rust (`adblock`) | Network request filtering | [Brave adblock-rust](https://github.com/brave/adblock-rust), **MPL-2.0**; the full upstream license text is in `third_party/licenses/MPL-2.0.txt` |
| Wry | Browser WebView integration | [Wry](https://github.com/tauri-apps/wry), MIT OR Apache-2.0 |
| winit | Native window handling | [winit](https://github.com/rust-windowing/winit), Apache-2.0 |
| Microsoft Edge WebView2 | Browser engine/runtime installed separately | [WebView2](https://developer.microsoft.com/microsoft-edge/webview2/); Microsoft runtime license terms and Chromium third-party notices apply |
| EasyList and EasyPrivacy | Filter lists downloaded over HTTPS at runtime, not embedded in the installer | [EasyList](https://easylist.to/) and [EasyPrivacy](https://easylist.to/); review each list's current license and attribution terms before redistributing snapshots |
| Other Rust crates | Cryptography, SQLite, networking, parsing, images and Windows APIs | Direct dependencies in `rust-browser/Cargo.toml`; exact resolved versions in `rust-browser/Cargo.lock`. Each crate retains its own license and notices |

The MPL-2.0 license applies to Brave's covered code, **not** automatically to all of Aurora. If you modify and distribute MPL-covered files, its source-availability and notice requirements may apply. MIT and Apache-2.0 also require retention of applicable copyright/license notices.

## Python reference application (not part of the Rust release)

| Component | Upstream and license |
| --- | --- |
| PySide6 / Qt / Qt WebEngine | [Qt for Python](https://doc.qt.io/qtforpython-6/), LGPLv3 / GPL options or commercial licensing depending on module; Qt WebEngine includes Chromium with additional third-party notices |
| Chromium embedded via Qt WebEngine | [Chromium](https://www.chromium.org/), BSD-style source license plus numerous third-party notices |
| cryptography | [pyca/cryptography](https://github.com/pyca/cryptography), Apache-2.0 OR BSD-3-Clause |
| argon2-cffi | [argon2-cffi](https://github.com/hynek/argon2-cffi), MIT |
| python-adblock | [python-adblock](https://github.com/ArniDagur/python-adblock), MIT OR Apache-2.0; upstream licensing files are `LICENSE-MIT` and `LICENSE-APACHE` |

For this Python variant, comply with Qt LGPL/GPL requirements (where applicable), the installed Qt WebEngine/Chromium notices, and all Python package metadata before distributing a frozen application.

## Source-reference acknowledgements

[qutebrowser](https://github.com/qutebrowser/qutebrowser) (GPL-3.0-or-later) was an architectural reference only, as recorded in `docs/third-party-sources.md`. No qutebrowser source files are identified as copied into Aurora. Referencing its design does not impose its GPL license on independently written Aurora source code.

## What this notice does and does not cover

- **Included in release packages:** This manifest and Brave's full MPL-2.0 text, alongside Aurora's MIT `LICENSE`.
- **Not a complete automated SBOM:** The precise Rust dependency graph includes transitive crates. Before public distribution, generate notices for **all** resolved crates and check their license expressions against the shipped binary.
- **Not a substitute for distribution compliance:** Include any additional applicable upstream copyright statements, Apache NOTICE files, MPL notices/source offers, and notices for binary-bundled libraries. Verify the exact installed versions. For any separately packaged Qt/Chromium components, preserve their original license directories and satisfy LGPL obligations.
- **No assertion of copied code:** Dependencies and architectural references should not be described as source copied verbatim without provenance evidence. If Aurora contains adapted upstream files, preserve their original headers and identify file-level source, authors, modifications and license.

See [the detailed source inventory](docs/third-party-sources.md) in the source repository and the two dependency manifests (`requirements.txt`, `rust-browser/Cargo.lock`).
