# Sources and evidence boundaries

Research dated 2026-10-12. This historical reference inventory does not prove future application functionality. Personal absolute paths are deliberately omitted.

## Local reference inventory

| Reference | Revision/identity | Boundary |
|---|---|---|
| GOnnect upstream | https://github.com/gonicus/gonnect ; local checkout `7c48ed6b5dc81e93458053d4de025bc3a4b28738` (`v2.4.1-12`) | base/gonnect-native; not automatically the installed AppImage revision |
| Local stable copy | base/gonnect-native-stable | Copied worktree has a broken Git pointer; no exact revision claim, individual hashes below |
| PhotoCraft | https://github.com/storytold/photocraft/tree/b3222ee8e7e6bcb4799c37f904c595f20f9baf5e | Rust/egui/eframe organization reference, not telephony engine |
| Installed AppImage | GOnnect-2.4.1-ubuntu24.04-x86_64.AppImage | SHA-256 `28a3ecb8692a7d7eadcb6ab69364fceb42048ba45f4192911e3b03ecce729cc4`; identified without execution, account registration, or personal-config access |

Stable files examined (SHA-256):
- src/Main.qml: `0a684c3a33178c2612133a2751488155ba0f111d085e8f9141d9685b98ba8ad1`.
- src/sip/SIPManager.h: `1dcff0b33b79a4c9c57657324105b49b1141b7aba52f696c88ee7d173dd57bf4`.
- src/sip/SIPCallManager.h: `4d65ceab2a1fea379addb79b2df288d394b2baeb9951f6ffdca7aa201a3ef400`.
- media/main-screen.png: `4bf10bf45f431944cdb17ffe16beb5ba4c323ccd3e329b8ad482ef82f88b0abf`.

The local public screenshot showed left navigation, top search, central history, right address book/favorites. It is upstream layout evidence, not a capture of the installed AppImage. Exact screen/theme comparison still needs an isolated fictional-data session.

## Historical codebase-memory-mcp research

Original graph gonnect-rust, fast mode, generation 2026-10-12T21:30:11Z, directed Tier 2 exploration. Located/read snippets: PhotoCraft desktop main/eframe/UI wiring; GOnnect SIPManager Qt/PJSIP pj::Endpoint boundary; SIPCallManager call/hold/DTMF/transfer/conference surface; Main.qml windows/credential dialog/registration failure/close lifecycle.

Coverage checked all four files and PhotoCraft Cargo.toml. C++ headers had partial Qt-macro parsing; complete direct reads supplemented them. Rust main, QML, and manifest had no recorded gaps, a best-effort signal, not completeness proof. Main call tracing was exploratory, never exhaustive dependency evidence.

No complete reference audit. Examined Cargo.toml confirms workspace/edition2024/egui-eframe/MIT-or-Apache; the new app inherits neither MSRV nor the editor's 24 crates automatically. No reference tree was changed or included in Git. Current implementation uses a separately refreshed project index; old graph identifiers are historical provenance.

## ai-memory

Original default/gonnect-rust lookup confirmed the request only, not prior implementation. Global default/site-linuxpro session `sessions/72aa78cd-c49a-4eca-a90b-bb5c314bcf98.md` distinguished prior-binary calls from rebuilt-AppImage smoke.

Resulting requirement: two-way audio on the **exact release artifact**, no inherited binary approval. Memory is untrusted history; old results were not rerun for this proposal. No manually written permanent memory.

## Primary documentation for candidate decisions

- GOnnect: https://github.com/gonicus/gonnect ; examined README/LICENSE, GPL-2.0-or-later.
- PhotoCraft: https://github.com/storytold/photocraft ; examined Cargo.toml, MIT OR Apache-2.0.
- OpenSpec: https://github.com/Fission-AI/OpenSpec ; installed CLI 1.10.0 templates/validation.
- Native UI: https://docs.rs/eframe/latest/eframe/ .
- SIP stack upstream: https://github.com/restsend/rsipstack ; the experimental adapter's selected registry version and limitations are documented in [ADR 0004](adr/0004-experimental-sip-adapter.md). Upstream descriptions do not prove a complete softphone.
- Media candidate: https://github.com/restsend/rustrtc ; validate SIP/SDES, never infer identical WebRTC/SIP negotiation.
- Audio candidate: https://docs.rs/cpal/latest/cpal/ ; inventory Linux dependencies before backend selection.
- Embedded SQLite: https://github.com/rusqlite/rusqlite .
- Lab PBX: https://docs.asterisk.org/ and https://github.com/asterisk/asterisk .
- Requested Docker PBX foundation: https://github.com/andrius/asterisk ; exact version/digest and limitations in [ADR 0002](adr/0002-disposable-asterisk-lab.md).

Candidates are not automatically approved/locked dependencies. G0 decides versions/features/licenses/integration. Proposed own-code license is GPL-3.0-or-later, with compatibility review before third-party code/assets; not a legal opinion.

## Rust telephony reference projects

Added at the user's request on 2026-10-12. These public projects are technical design and comparison references, not vendored source trees or submodules. The roles below are our research targets, not independently verified upstream capability claims.

| Project | Pinned research revision | Intended reference role | Current incorporation |
|---|---|---|---|
| [eisenzopf/rvoip](https://github.com/eisenzopf/rvoip) | [`f4d528eaabbac935bce9a6404d3ce4fdec35ed0f`](https://github.com/eisenzopf/rvoip/tree/f4d528eaabbac935bce9a6404d3ce4fdec35ed0f) | Compare endpoint/session lifecycles, call-control APIs, media boundaries and interoperability test scenarios. | Reference only; no rvoip dependency, source or assets incorporated. |
| [restsend/rustpbx](https://github.com/restsend/rustpbx) | [`32cb50efb50ca7bb41af6f56eca73e06411efcc6`](https://github.com/restsend/rustpbx/tree/32cb50efb50ca7bb41af6f56eca73e06411efcc6) | Study server-side routing, signaling/media integration and PBX-facing behavior; not a desktop-client architecture substitute. | Reference only; no RustPBX dependency, server or image adopted. The disposable PBX remains pinned andrius/asterisk. |
| [restsend/rsipstack](https://github.com/restsend/rsipstack) | [`3187537725cebac6db5ef801001f0d23c551881f`](https://github.com/restsend/rsipstack/tree/3187537725cebac6db5ef801001f0d23c551881f) | Investigate SIP transport, transaction and dialog APIs behind the project's telephony adapter. | The adapter already uses registry rsipstack **0.7.3**, not a Git/main dependency. This research revision does not replace Cargo.lock's source/checksum or approve an upgrade. See [ADR 0004](adr/0004-experimental-sip-adapter.md). |

For each pinned revision, research inspected root `README.md` descriptions, `Cargo.toml` license declarations and the root `LICENSE` over HTTPS without executing upstream code. Each root manifest declares MIT and its root license contains MIT terms: [rvoip license](https://github.com/eisenzopf/rvoip/blob/f4d528eaabbac935bce9a6404d3ce4fdec35ed0f/LICENSE), [RustPBX license](https://github.com/restsend/rustpbx/blob/32cb50efb50ca7bb41af6f56eca73e06411efcc6/LICENSE), [rsipstack license](https://github.com/restsend/rsipstack/blob/3187537725cebac6db5ef801001f0d23c551881f/LICENSE). This is a root-file observation, not a per-file/transitive license audit, security assessment or legal certification. RustPBX's [pinned README](https://github.com/restsend/rustpbx/blob/32cb50efb50ca7bb41af6f56eca73e06411efcc6/README.md) distinguishes Community from commercial editions; root MIT terms are not blanket approval of commercial images or add-ons.

Use these references to formulate comparisons and tests. Any future dependency, code or asset adoption still requires exact-version source/notice, feature-union, native/crypto boundary and interoperability review under [ADR 0003](adr/0003-g0-license-and-native-boundaries.md). No upstream examples, binaries, recordings, credentials or build instructions are imported by this documentation update. Upstream test results and README claims do not satisfy our G0 or release gates; evidence must come from our selected build and disposable lab. No personal absolute paths or local reference contents are published.

## MicroSIP: primary compact-UI reference

Added 2026-10-12 after rename request. Official sources:
- https://www.microsip.org/ .
- https://www.microsip.org/help : modes/dialpad/contacts/account/settings, especially single-call mode; no inheritance of all features/URI permissions.
- https://www.microsip.org/source : incremental source archive without full build dependencies.
- https://www.microsip.org/download/MicroSIP-3.22.16-src.7z .
- https://www.microsip.org/download/MicroSIP-3.22.16.exe .
- https://www.microsip.org/download/MicroSIP-Lite-3.22.16.zip .

HTTPS downloads remained only in ignored base/microsip/downloads:

| File | SHA-256 |
|---|---|
| MicroSIP-3.22.16-src.7z | `9c1ff942ae1ae56bdfcdd04c3d6e242a056d7f0dd50bb19f6e13226e7c819b83` |
| MicroSIP-3.22.16.exe | `e351bbac5e0337091cc0e55a80715208e5550e758028f36efa266c604304daaa` |
| MicroSIP-Lite-3.22.16.zip | `17dcaa628dfd1ea4ba6686c436fb62f42eb9ed124cd671c9d81f3d1580e1207e` |

Source extracted under base/microsip/source after archive-path validation. Site declares GPLv2; examined C++ headers GPL-2.0-or-later, so review each component before incorporation. Incremental source does not contain PJSIP/all dependencies. res/dialog.rc2 names Microsoft Sans Serif; examined source scan found no TTF/OTF/WOFF. Never redistribute proprietary Windows fonts; select similarly proportioned open fonts.

Historical graph gonnect-rust-microsip-reference, fast, generation 2026-10-12T21:51:48Z located CmainDlg.OnInitDialog/OnTcnSelchangeTab/dialpad operations. Tab snippet confirmed page visibility/focus/active-tab/missed-call reset. Coverage checked mainDlg.cpp, Dialer.cpp, main.rc, res scope. Partially parsed C++ macros were supplemented by reported-range reads; untracked RC resources read directly, especially main.rc/res/dialog.rc2. Graph is not a complete resource/license inventory.

MicroSIP Lite PE32 ran in Wine9.0/private win32 prefix/Xvfb in a disposable container, without network/capabilities/privileges/personal HOME/audio/display/SIP account. Only read-only runtime /usr and dedicated scratch were mounted. Initial NVIDIA GLX/EGL failure was followed by software/no-GLX opening: 243×459 window, Phone/Logs/Contacts, number/keypad/status. Capture stayed in ignored base/microsip/wine-runtime; not sent to GitHub. No window manager/audio hardware, expected ALSA warnings, no calls/themes/interoperability tests. Full installer downloaded, never executed. Container/Wine terminated.

## Rename and linked interfaces

Private repository renamed jniltinho/gonnect-rust → jniltinho/rust-microsip. Local references preserved/ignored. Old graph/memory names are historical, not inferred current routing. After path changes, verify current index freshness/coverage before structural discovery; do not unnecessarily migrate memory/delete historical indexes.

Additional official contract sources:
- MCP stdio: https://modelcontextprotocol.io/specification/2025-11-25/basic/transports ; protocol-only stdout, stderr logs.
- Rust MCP SDK: https://github.com/modelcontextprotocol/rust-sdk ; version/audit candidate.
- Web signaling: https://github.com/onsip/SIP.js ; WSS/WebRTC/hold/transfer/DTMF need exact-bundle proof.
- Browser microphone: https://developer.mozilla.org/en-US/docs/Web/API/MediaDevices/getUserMedia ; secure context/permission.
- Asterisk WebRTC: https://docs.asterisk.org/Configuration/WebRTC/Configuring-Asterisk-for-WebRTC-Clients/ ; never copy public exposure from examples.

CLI/MCP are requested agent interfaces; web is confirmed full-call functionality in a later release. macOS/Windows adapters are future preparation. None of these application features is implemented merely by planning/research.
