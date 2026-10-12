# ADR 0001 — scaffold and G0 decision boundaries

Date: 2026-10-12. Status: accepted for the scaffold; complete-stack selection pending.

## Context

The approved plan requires Docker builds without host Cargo and a vertical proof before the complete UI. Starting with the builder makes `base/` isolation, toolchain, lockfile, and tests verifiable without inventing telephony success.

## Decision for this slice

Own code uses GPL-3.0-or-later, following the approved direction; see `LICENSE`. Initially the workspace contains only `apps/rust-microsip`, a private typed error, and a minimal CLI API. No empty crates or fakes advertise available services. No upstream code/assets are incorporated. No font/icon was selected in this slice; the ELF contains no graphical resources.

The builder pins Ubuntu 24.04 and Rust 1.99.0, compiles offline, and exports only scaffold, licenses, and metadata. Builder tools are not application runtime dependencies. Own code forbids unsafe; lints reject unwrap/expect/todo/unimplemented.

## Current inventory versus pending work

| Item | Current status | License/provenance | Remaining verification |
|---|---|---|---|
| Own binary | incorporated | GPL-3.0-or-later | independent review and G0 expansion |
| Rust std/toolchain 1.99.0 | std linked; tools builder-only | MIT/Apache-2.0 and third-party notices in official package, exported | complete release transitive inventory |
| Linux/glibc/libgcc | ELF dependencies to measure | Ubuntu packages identified in export | SONAME allowlist and clean VMs after complete stack |
| Fonts, icons, tones | none incorporated | no upstream assets copied | select open resources and preserve attribution |
| rsipstack | candidate, not in this Cargo.lock | [upstream manifest](https://github.com/restsend/rsipstack/blob/main/Cargo.toml) declares MIT | pin version/features; real TLS/dialogs/REFER/transitions |
| rustrtc | candidate, not incorporated | [upstream manifest](https://github.com/restsend/rustrtc/blob/main/Cargo.toml) declares MIT | prove SIP SDES, PCMA/PCMU, DTMF, and mandatory SRTP |
| eframe/wgpu, audio, D-Bus, IPC, SQLite | planned, not incorporated | per-version inventory pending | features, native/transitive dependencies, licenses, dlopen |
| Asterisk | separate test component | [ADR 0002](0002-disposable-asterisk-lab.md) | isolated PJSIP lab and full call/media tests |

Candidate links document investigation, not incorporation authorization or pinned versions. Task 1.3 stays open until the complete selected dependency/font/icon and compatibility inventory exists. A root crate license does not prove compatibility of every dependency.

## Consequences

`--doctor` reports `g0_passed=false`; no UI, call, or migration is simulated. G0 remains pending until all matrix receipts exist, including secure media, desktop/hardware, and complete-stack dependencies. This ADR does not authorize replacing ELF with AppImage, adopting PJSIP/Qt, or removing TLS/SRTP without proposal review.
