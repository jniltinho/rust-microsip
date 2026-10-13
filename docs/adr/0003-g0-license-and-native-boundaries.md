# ADR 0003 — G0 licenses, assets and native dependency boundaries

Date: 2026-10-12. Status: accepted engineering boundary for pre-adoption evaluation; not a release/legal certification or G0 approval. Baseline: scaffold `1bfbe7d`, isolated PBX `7104510`.

This is the pre-adoption snapshot at the baseline above. Subsequent experimental SIP dependency incorporation is documented in [ADR 0004](0004-experimental-sip-adapter.md) and its selected registry/notice inventory; the workspace is no longer dependency-free. The ordinary CLI scaffold still has no external crates or account access.

## Decision

Keep own application code **GPL-3.0-or-later**, as approved. Evaluate only dependencies with a documented compatible licensing path and retained notices. Do not copy code/assets from the local reference trees. Do not substitute the separate PBX's GPL-2.0 licensing for the client's license. No app dependency is approved for production merely because its crate declares a familiar license. An adoption-time block applies **anywhere in the selected build tree**, including transitive dependencies; a parent that cannot disable a blocked component inherits its block.

This is an engineering compliance inventory, not legal advice. [ASF's GPL compatibility guidance](https://apache.org/licenses/GPL-compatibility.html) supports the Apache-2.0 → GPLv3 direction, not the reverse; retain applicable Apache NOTICE/patent terms and permissive copyrights. Dual-license declarations allow selection only where the actual files provide that option. Unknown, nonstandard, GPL-2.0-only without an applicable permission, or conflicting bundled-component terms block incorporation until reviewed. GPL-2.0-or-later reference code would still need an exact per-file audit and attribution; no copying is planned.

## Existing incorporated material

- Own Rust scaffold, tests, packaging/lab wrappers and docs: project GPL-3.0-or-later, root LICENSE/NOTICE. No external Cargo crate is in the scaffold lockfile at this baseline.
- Rust 1.99.0 standard library/compiler distribution: exported component licenses, `COPYRIGHT.html`, `COPYRIGHT-library.html` and the generated license directory are preserved. Rust's overall MIT/Apache direction is **not** asserted as the license of every bundled subcomponent. Preliminary inventory remains explicitly incomplete for a final release; future license collection must cover actual linked/embedded parts and required notices.
- Current ELF dynamically needs glibc/libgcc_s only. System runtime licenses are separate from the application license; this is neither a fully static ELF nor a full future-stack audit.
- Docker builder packages/tooling are not included in the exported app. Exported package version inventory identifies build inputs; a version list alone is not a license SBOM.
- Development skills under `.agents/skills/` have their own upstream licenses/provenance in [development-skills](../development-skills.md); they are development tools, not embedded application resources.
- Asterisk/PJProject/SRTP/OpenSSL/Debian and inherited optional image assets are **separate PBX infrastructure**, under [ADR 0002](0002-disposable-asterisk-lab.md). The image is not part of client ELF/AppImage and is not published by this project. No inference that the complete image is MIT licensed.

## Direct candidate inventory

The [machine-readable inventory](../inventory/g0-candidate-crates.json) records exact published versions, archive SHA-256, Cargo VCS revisions and inspected notice hashes/origin paths. Sixteen direct archives were retrieved from the official registry over HTTPS, bounded/path-checked and checksum-verified in ignored research storage. No downloaded source/font is copied into the application or repository. Crates missing packaged workspace notices were checked against their exact VCS revision where available. This catalog is not Cargo.lock and does not freeze the final choices.

| Candidate/version | Observed declared terms | Evaluation boundary |
| --- | --- | --- |
| rsipstack 0.7.3 | MIT; packaged license inspected | Disable defaults; evaluate `platform-tokio,rustls`, no WebSocket/embedded/benchmark features. This version uses its own SIP types; do not add old `rsip` with unresolved nonstandard metadata. TLS custom verifier must remain absent, explicit CA/hostname validation required. |
| rustrtc 0.4.7 | MIT; packaged license inspected | Data-plane-only `default-features=false` evaluation first. No inferred SIP SDES support from WebRTC claims; interoperable SRTP/replay/authentication tests still mandatory. Do not enable its DTLS/UPnP/ICE defaults for the initial SIP profile. |
| audio-codec 0.4.9 | MIT declaration/README, **complete license notice unavailable** | **Do not adopt yet.** Exact-revision LICENSE lookup also failed. Default Opus/G.729 features must not enter a G.711-only build. Choose a fully licensed alternative or resolve the missing notice before linking; no copied implementation to bypass the check. |
| eframe 0.36.2 | MIT OR Apache-2.0; exact-workspace notices inspected | Disable default fonts, persistence, links and unneeded backends. Evaluate wgpu + Wayland/X11 + accessibility. The inspected eframe 0.36.2 normalized manifest requests wgpu 30.0; resolve/record the exact transitive pin at adoption, not an independently guessed ABI. Reject `default_fonts` if feature unification re-enables it anywhere in eframe/egui/epaint, including `epaint_default_fonts`. |
| cpal 0.18.2 | Apache-2.0; packaged license inspected | ALSA candidate first. No JACK/ASIO/PipeWire/PulseAudio/realtime feature without updating the native/permission inventory. No host package installation to make client acceptance pass. |
| rusqlite 0.40.2 | MIT; packaged license inspected | Evaluate bundled SQLite + backup; no SQLCipher, extension loading or external server. [SQLite deliverable is public-domain](https://sqlite.org/copyright.html); wrapper/build tooling has separate terms. |
| zbus 5.19.0 | MIT; packaged license inspected | Tokio backend intended, no libdbus FFI assumed; D-Bus services may be absent. Reject an unreviewed async-io + Tokio union from accessibility/tray feature unification. Secret Service/notifications must degrade explicitly. |
| ksni 0.3.6 | Unlicense; packaged text inspected | Optional tray only, with Tokio; not mandatory for receiving calls. Retain the upstream text. |
| rmcp 3.5.1 | Apache-2.0; exact-workspace license inspected | Server/stdio only; no HTTP/auth/child-process transport required. Local authorization remains our responsibility. |
| tokio 1.53.2 | MIT; packaged license inspected | Own requested feature set stays narrow; the inspected rsipstack 0.7.3 normalized manifest requests Tokio `full`, so feature union must be reported honestly. |
| rustls 0.23.45 | Apache-2.0 OR ISC OR MIT; packaged notices inspected | Set `default-features=false` and explicitly select ring. Pin ring 0.17.14, whose packaged LICENSE, LICENSE-BoringSSL and LICENSE-other-bits were inspected. Retain mixed Apache/ISC and nested once_cell notices. Feature unification must not re-enable aws-lc; dynamic-link inspection alone cannot catch static vendoring. |
| serde 1.0.229 / serde_json 1.0.154 | MIT OR Apache-2.0; packaged notices inspected | Typed, bounded input; a compatible license does not ensure safe secret serialization. |
| thiserror 2.0.21 | MIT OR Apache-2.0; packaged notices inspected | Redacted typed adapter errors, no network/auth payload in Display/Debug. |
| secrecy 0.10.3 / zeroize 1.9.1 | Apache-2.0 OR MIT; packaged notices inspected | Redacted secret ownership; avoid accidental Debug/Serialize. Zeroization does not promise removal of every allocator/library copy. |

A disposable signaling/media manifest was resolved in Docker, never using host Cargo or personal HOME. Its filtered metadata was used to inspect transitive declarations and feature-union risks, **not** to approve every transitive source/notice or to claim the final runtime graph. Cargo metadata/lock resolution may retain optional packages that the selected `cargo tree` does not compile. In particular optional Opus/G.729 packages appeared in metadata while absent from the selected feature tree: use actual build/feature evidence, not a package-name grep, for final inclusion decisions. This exploratory manifest is ignored and is not the application's lockfile. Before each actual adoption, capture the exact lock/features, inspect native build scripts/bundles, retain complete notices and rerun security/license checks. Full release SBOM and vulnerability policy remain task 9.3/9.9 gates.

## Crypto provider and feature-union gate

For TLS native crypto, **ring 0.17.14 only** is within the current evaluation boundary. Use rustls 0.23.45 with `default-features=false` and `ring` explicitly selected. `aws-lc-rs`, `aws-lc-sys`, `openssl-sys`, `openssl-src`, BoringSSL FFI and any other unreviewed C crypto provider are outside the boundary, including statically vendored code. Their absence from DT_NEEDED would prove nothing about static inclusion. This policy does not assert the licensing of every present/future AWS-LC version; it requires exact-version source/notice review before a change.

For SRTP, rustrtc 0.4.7's inspected no-default-feature manifest uses **RustCrypto** AES/CTR/HMAC/SHA-1/SHA-2 and AES-GCM, not ring or libSRTP. Exploratory selected versions include aes 0.9.3 and 0.8.4, ctr 0.10.1 and 0.9.2, hmac 0.13.0, sha1 0.11.0, sha2 0.11.1 and aes-gcm 0.10.3, each declaring an MIT/Apache-2.0 choice. Final exact transitive lock/notices, cryptographic correctness and replay/authentication interoperability are still adoption/G0 gates; this is not a cryptographic audit. Any nonempty rustrtc feature set requires another inventory review; the reviewed allowlist is empty, not merely a denylist of known default features.

`packaging/check-g0-dependencies.py` checks **actual selected `cargo tree` output** for direct/transitive blocked components, the exact ring/rustls boundary, forbidden default-font re-enablement, unreviewed rustrtc features and mixed zbus runtimes. Docker verification runs nine positive/adversarial tests and this check with the actual target. It is a narrow deny boundary, not a complete allowlist, SBOM, source-license or vulnerability audit. Future dependencies with other native backends still require human/source inventory review. No complete notice obligation is satisfied by the check alone. Before adoption, also match registry source/checksums in Cargo.lock and reject unreviewed path/git patches: the checker itself does not authenticate source suffixes. Test-only dependencies need the same notice/provenance review before compilation, although they are not shipping-tree edges.

The ring 0.17.14 archive's named license notices and nested once_cell terms were inspected; a bounded source scan found no `Original SSLeay License`, `advertising materials` or `OpenSSL License` marker. This is evidence for this candidate version, not proof of absence of every possible conflicting term. Source/notice review is still mandatory at final incorporation.

## SRTP pre-adoption follow-up

The [isolated SRTP candidate inventory and diagnostic](../receipts/g0-srtp-candidate-2026-10-13.md)
now records the exact no-default-feature candidate graph and retained notices.
The collector additionally pins `rustrtc 0.4.7` and `version_check 0.9.5` archive
identities; the latter's legacy license spelling is restricted to its inspected
version and both complete notices. This is evaluation evidence, not application
adoption: the actual duplicate-authenticated-packet test fails. See
[ADR 0009](0009-srtp-candidate-evaluation-boundary.md) before using the candidate.

## Fonts, icons, recordings and tones

No upstream application font/icon/recording is currently embedded. Do not redistribute Microsoft Sans Serif, MicroSIP/GOnnect brands, screenshots, Windows binaries, or local reference assets. The same UI flow is not permission to copy artwork. The project name/trademark/disclaimer is a separate open naming review before public release or visibility changes; copyright compatibility is not trademark clearance.

Initial font direction: unmodified Noto Sans under **SIL OFL-1.1**, not egui's entire mixed-license default-font bundle. The [official Noto license](https://github.com/notofonts/noto-fonts/blob/main/LICENSE) and [OFL FAQ](https://openfontlicense.org/ofl-faq/) support bundling under the font's own terms. Before embedding, record the exact font revision/file hash, copyright/reserved-name terms and complete OFL notice in the repository/export; no runtime font download. Font remains OFL, not relicensed GPL. No font binary has been copied yet. Keep the first font unmodified; subsetting is an OFL Modified Version and requires separate reserved-name/attribution review, not an assumed size optimization.

Icons: own simple vector primitives initially, no upstream logo/icon pack. Ringtone/test tones: own mathematical synthesis, no recordings or downloaded samples. Accessibility labels remain English. Additional resources require an inventory update **before** copying them.

## Candidate native/runtime inventory

These are expected interfaces to audit, not proven DT_NEEDED/dlopen output of a complete-stack ELF. Exact versions/SONAMEs/presence must be measured in the supported clean VMs in task 1.8; the allowlist cannot be expanded silently.

| Area | Expected boundary / terms | Acceptance obligation |
| --- | --- | --- |
| Loader/glibc | Host Linux ABI; glibc LGPL-2.1-or-later family | Build on supported Ubuntu 24.04 baseline, audit symbol versions and indirect dependencies. No bundled replacement loader promised. |
| libgcc_s | Host GCC runtime, GPL with GCC Runtime Library Exception | Preserve the exception/license attribution if redistribution occurs; current scaffold records dependency only. |
| SQLite | Bundled C implementation, public-domain deliverable; libsqlite3-sys wrapper/build terms separately | Static into ELF; no libsqlite3.so dependency, plugins, external DB or first-use download. |
| TLS crypto | ring compiled C/assembly and Rust; Apache-2.0 AND ISC plus bundled notice details | Compiler/assembler only in builder; inspect complete ring notices and actual native output. No hidden libssl/libcrypto requirement. |
| Audio | ALSA `libasound.so.2`, LGPL-2.1-or-later family; host desktop audio services/plugins | Only builder gets development headers. Enumerate dlopen/plugin needs and actual device behavior; no root/realtime capability or extra client package requirement. Optional backends stay off pending review. |
| Window/input | Host X11/Wayland/xkbcommon client interfaces, predominantly MIT/permissive with per-package notices | Enumerate actual direct/dlopen SONAMEs and clean GNOME/Wayland and KDE/X11 availability. No toolkit sidecars. |
| Graphics | Host Vulkan/GL/EGL loader and driver selected by wgpu; driver-specific terms | Compatible software/integrated renderer required; audit driver/loader use, not just ldd. No dedicated-Vulkan assumption or bundled driver. |
| D-Bus/IPC | zbus's Rust protocol and host session bus; Unix socket/SO_PEERCRED via audited platform adapter | No libdbus/Qt assumed; missing services explicit. Do not leak Linux types into domain. |

System-family license labels are inventory pointers, not a claim that every driver's/plugin's code has the same license. Refer to the exact supported-distribution package copyright files before bundling any library in AppImage. Dynamic host use and redistribution have different notice/source obligations. Native/unsafe third-party code is expected and must be isolated/audited; own `unsafe_code=forbid` remains in force unless a separately reviewed narrow adapter is necessary.

## Consequences and stop criteria

The inventory/compatibility boundary precedes the next G0 dependency/resource incorporation. It approves evaluation of evidenced candidates, **not production acceptance**. Missing codec notice blocks `audio-codec` at this or any unreviewed version **wherever it is selected, directly or transitively**; any parent requiring it is blocked too. This is not permission to skip G.711. Missing transitive notices or incompatible terms block **adoption**, not only release. Out-of-allowlist runtime dependencies or extra client packages block release until resolved/reviewed. Do not silently switch to PJSIP/Qt, remove SRTP or call AppImage success an ELF pass. PBX review, this ADR, downloads and metadata resolution are not call tests. Complete G0 and all release gates remain open.
