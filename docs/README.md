# Documentation and roadmap

This directory holds the project's design records, evidence and guides. Start with:

- [Architecture](architecture.md): layers, runtime model, security boundaries and porting.
- [Development guide](development.md): pinned toolchain, Docker build and tests.
- [Architecture decision records](adr/): one file per accepted decision.
- [Receipts](receipts/): evidence of each verified milestone (commands, hashes, measurements and limits).
- [Reviews](reviews/): independent static reviews and their verdicts.
- [OpenSpec change](../openspec/changes/create-native-rust-linux-release/): proposal, design, specifications and the task list that is the authoritative progress tracker.

## Status

**There is no release yet.** A first desktop build opens a compact MicroSIP-like window and makes real SIP calls, verified against a disposable Asterisk lab. The remaining work below must pass before v0.1.0 is published. Progress figures come from the OpenSpec task list, which only checks a task when its complete criteria have evidence.

## Done

| Area | What exists | Evidence |
| --- | --- | --- |
| Planning | Approved OpenSpec proposal, design, specs and test plan; license and native-dependency boundaries | [Approval](implementation-approval.md), [ADR 0003](adr/0003-g0-license-and-native-boundaries.md) |
| Build | Pinned Ubuntu/Rust Docker builder, offline verification, reviewed notice collection, ELF export with SBOM and checksums | [ADR 0012](adr/0012-production-dependency-graph.md) |
| Lab | Disposable Asterisk PBX with UDP/TCP/TLS and SDES fixtures, IVR, tone and echo routes; no published ports | [Lab receipt](receipts/asterisk-lab-2026-10-12.md) |
| Domain | Validated settings and dial targets, call/registration state machine, redacted command/event contract | `crates/core` |
| Telephony | Registration with renewal and bounded backoff; outgoing and incoming calls, reject, cancel, DND; PCMU/PCMA; RFC 4733 DTMF; hold/resume; mute; blind transfer with NOTIFY; PBX re-INVITE answers | [Production engine receipt](receipts/production-engine-2026-10-13.md) |
| Secure media | TLS signaling with system or custom CA, SDES/SRTP and SRTCP, replay protection, downgrade rejection, rekey with a fresh SSRC, secure DTMF/hold/transfer | [SRTP/SRTCP](receipts/g0-srtcp-call-2026-10-13.md), [rekey](receipts/g0-rekey-call-2026-10-13.md), [secure controls](receipts/g0-secure-controls-2026-10-13.md) |
| Audio | ALSA capture/playback bridge, resampling to 8 kHz, bounded jitter buffer, silent fallback with notice | `crates/audio`, `crates/telephony` |
| Configuration | CLI/env/.env/TOML/SQLite resolution with provenance, user edits only written back, `config import/validate/export`, `--doctor`; optional keyring storage of the password (Secret Service) | [Config/recovery/release receipt](receipts/config-recovery-release-2026-10-13.md) |
| Storage | SQLite schema and migrations (0600 files, future-schema refusal), call history with 90-day retention, contacts with multiple numbers and favorites, atomic CSV preview/import, online backup; used by the Calls and Contacts tabs | `crates/storage` |
| Desktop | egui window: Phone/Calls/Contacts tabs, keypad, Call/End, DND/MUTE/HOLD/XFER, incoming-call banner, history filters and search, contact editor and CSV import, settings with audio device selection, themes, licences; graceful SIGTERM | [UI notes](ui/g0-native-ui.md), [screenshots](prints/) |
| Linux integration | Single instance over a private Unix socket (a second launch forwards and exits), strict `sip:`/`tel:` link parsing that only fills the number, opt-in link handler entry, incoming-call notifications with Answer/Reject over D-Bus, quit confirmation during calls | `crates/platform` |
| Resilience | Registration renewal, recovery after a network outage, BYE delivered after recovery, crash recovery of interrupted calls in history | [Config/recovery/release receipt](receipts/config-recovery-release-2026-10-13.md) |
| Release engineering | AppImage, CI workflow passing on GitHub, a RustSec/source check job, publication scanner (index, history and artifacts), reproducible clean-clone build, release-notes draft | [Release draft](release/v0.1.0-draft.md) |
| Hardening | Automated layering rule, bounded deterministic fuzzing of the dial-string, link, CSV, `.env`, argument, SDP, RTP and DTMF parsers, engine supervision, final events kept on shutdown, WCAG contrast test, theme switching during a call, exit with a frozen PBX, 100/150/200% scaling | [Hardening receipt](receipts/hardening-2026-10-13.md) |

## Remaining for v0.1.0

| Area | Remaining work | OpenSpec tasks |
| --- | --- | --- |
| G0 gate | Same-stack spike with D-Bus/notifications and local IPC; clean GNOME/Wayland and KDE/X11 VMs; native dependency audit; recorded G0 decision | 1.5, 1.8, 1.9 |
| Media | Device hotplug with real headsets, local test on real devices, NAT/STUN topology | 5.2, 5.3, 5.5 |
| Desktop UI | Visual matrix against MicroSIP, remaining screens, screen-reader labels | 7.1–7.3, 7.5 |
| Linux integration | Notification and tray checks on real desktops | 8.1 |
| Release | Real-desktop checks of keyring prompts/KWallet and suspend/resume (part of the VM checklist), AppImage licence review, the first `cargo deny` run, full Asterisk suite, VM and human audio checklist, performance budgets, final review, SBOM and notes, publication | 9.2, 9.3, 9.5–9.10 |
| Agent CLI/MCP | Local control CLI and MCP server over authenticated IPC | [add-agent-cli-mcp](../openspec/changes/add-agent-cli-mcp/tasks.md) |

## Later releases

- macOS and Windows builds (the engine and media layers are already portable).
- A refined visual design following the MicroSIP layout.
- A web/WASM softphone with WebRTC ([proposal](../openspec/changes/add-web-wasm-softphone/proposal.md)).
