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
| Configuration | CLI/env/.env/TOML resolution with provenance, private secret files, atomic saves, `--doctor`, `config validate/export` | `crates/config` |
| Storage | SQLite schema and migrations (0600 files, future-schema refusal), call history with 90-day retention, contacts with multiple numbers and favorites, atomic CSV preview/import, online backup; used by the Calls and Contacts tabs | `crates/storage` |
| Desktop | egui window: Phone/Calls/Contacts tabs, keypad, Call/End, DND/MUTE/HOLD/XFER, incoming-call banner, history filters and search, contact editor and CSV import, settings with audio device selection, themes, licences; graceful SIGTERM | [UI notes](ui/g0-native-ui.md), [screenshots](prints/) |
| Linux integration | Single instance over a private Unix socket (a second launch forwards and exits), strict `sip:`/`tel:` link parsing that only fills the number, incoming-call notifications with Answer/Reject over D-Bus | `crates/platform` |

## Remaining for v0.1.0

| Area | Remaining work | OpenSpec tasks |
| --- | --- | --- |
| G0 gate | Same-stack spike with D-Bus/notifications and local IPC; clean GNOME/Wayland and KDE/X11 VMs; native dependency audit; recorded G0 decision | 1.5, 1.8, 1.9 |
| Architecture | Platform contracts and fakes, supervision and saturation tests, fake clock and an automated layering rule | 2.1, 2.3, 2.5 |
| Configuration | SQLite settings layer, Secret Service with fallback, migrations/backups under failure, TOML import | 3.1–3.6 |
| Telephony | Registration renewal and duplicate-account checks, network/suspend recovery | 4.1, 4.6 |
| Media | Loss/jitter proxy tests, device selection and hotplug, local echo test, NAT/STUN | 5.1–5.5 |
| Contacts and history | Crash recovery of interrupted calls | 6.3 |
| Desktop UI | Visual matrix against MicroSIP, remaining screens, validated settings persistence, accessibility, keyboard and scaling | 7.1–7.5 |
| Linux integration | Notification and tray checks on real desktops, opt-in `sip:` handler registration, close/quit confirmation during calls | 8.1, 8.2, 8.4 |
| Release | AppImage, CI with fuzzing, publication scanning, full Asterisk suite, VM and human audio checklist, performance budgets, final review, SBOM and notes, publication | 9.1–9.10 |
| Agent CLI/MCP | Local control CLI and MCP server over authenticated IPC | [add-agent-cli-mcp](../openspec/changes/add-agent-cli-mcp/tasks.md) |

## Later releases

- macOS and Windows builds (the engine and media layers are already portable).
- A refined visual design following the MicroSIP layout.
- A web/WASM softphone with WebRTC ([proposal](../openspec/changes/add-web-wasm-softphone/proposal.md)).
