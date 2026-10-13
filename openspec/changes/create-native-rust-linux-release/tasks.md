## 1. Approval and G0 vertical proof

- [x] 1.1 Obtain approval of this proposal and its linked CLI/MCP extension, recording web as a later release and v0.1.0 limits; explicitly verify one simultaneous account/call, deferred integrations, and the ELF versus AppImage contract. Evidence: [implementation approval](../../../docs/implementation-approval.md).
- [x] 1.2 Create a minimal Rust scaffold for the vertical proof with a pinned toolchain and lockfile; verify a clean Docker builder build without `base/` or host Cargo (DIST-03). Evidence: [scaffold receipt](../../../docs/receipts/g0-scaffold-2026-10-12.md); this is not full DIST-03 or G0 approval.
- [x] 1.3 Inventory licenses, fonts/icons, and native dependencies; deliver a compatibility/license ADR before copying any upstream resource. Evidence: [ADR 0003](../../../docs/adr/0003-g0-license-and-native-boundaries.md) and [inventory receipt](../../../docs/receipts/g0-license-inventory-2026-10-12.md); candidate adoption and complete release audits remain gated.
- [x] 1.4 Create a pinned Asterisk Docker PBX with extensions 1001/1002, echo, IVR, and tones; verify PJSIP readiness and no published ports in the default profile (DIST-04). Evidence: [PBX infrastructure receipt](../../../docs/receipts/asterisk-lab-2026-10-12.md); full DIST-04 call scenarios remain pending.
- [ ] 1.5 Create a fictional secrets/CA generator, a host profile with a non-internal bridge/loopback, and a G4 host-only profile in an auxiliary VM; verify `docker compose config`, binds/ACL, SDP, anchored media, and teardown without accessing personal data (DIST-05).
- [ ] 1.6 Prove UDP/TCP/TLS registration and two-way G.711 PCMA/PCMU with the candidate Rust stack; attach PBX receipts and synthetic media measurements (SIP-01, SIP-03, AUD-01). Partial evidence: [Rust registration receipt](../../../docs/receipts/g0-rust-registration-2026-10-12.md) and [synthetic media receipt](../../../docs/receipts/g0-rust-media-2026-10-12.md). Short isolated calls do not complete AUD-01 human/exact-artifact acceptance; this task remains open without narrowing its criteria.
- [ ] 1.7 Prove DTMF, hold, REFER/NOTIFY, and TLS+SRTP/SDES with negative cases; approve the adapter/versions only with green tests (SIP-06, SIP-08, SIP-09, AUD-05). Partial evidence: [negotiated RFC 4733 IVR receipt](../../../docs/receipts/g0-rust-dtmf-2026-10-12.md), [synthetic hold/resume/rejected-offer receipt](../../../docs/receipts/g0-rust-hold-2026-10-12.md), and [synthetic REFER/NOTIFY transfer receipt](../../../docs/receipts/g0-rust-transfer-2026-10-12.md); mandatory SRTP/SDES and production hardening remain pending. The [isolated SRTP pre-adoption diagnostic](../../../docs/receipts/g0-srtp-candidate-2026-10-13.md) records a real replay-rejection failure, not secure-media acceptance. The [isolated per-SSRC packet adapter](../../../docs/receipts/g0-srtp-adapter-2026-10-13.md) now has green SRTP/SRTCP replay tests, but no secure call/signaling or bounded call source/key lifecycle proof. This task stays open with its full criteria.
- [ ] 1.8 Produce a spike ELF with the same eframe/wgpu, audio, D-Bus/notifications, and IPC v1/SO_PEERCRED stack as the production CLI/MCP extension; verify window, focus/keyboard, hotplug, and compatible renderer in GNOME/Wayland and KDE/X11, audit DT_NEEDED/dlopen, and block G0 if extra packages are required (DIST-01, ND-05, AUD-03).
- [ ] 1.9 Record the G0 decision and read-only Claude CLI review; verify a receipt linked to commits/results, preventing full UI work if a blocker exists.

## 2. Workspace and documented contracts

- [ ] 2.1 Structure crates and binary according to the design; verify an acyclic dependency graph and a domain free of UI/SQL/network adapters; include platform contracts, Linux adapter, capabilities, and fakes without leaking OS types, preparing macOS/Windows without promising support.
- [ ] 2.2 Implement types/IDs, the state machine, and typed errors; verify transition properties and no panic on invalid input.
- [ ] 2.3 Implement commands/events with bounded queues, deduplication, and supervision; verify saturation, cancellation, and final-event retention without blocking UI/audio.
- [ ] 2.4 Add crate/API rustdoc, examples, and an architecture README; verify `cargo test --doc` and rustdoc with warnings as errors.
- [ ] 2.5 Create fixtures, a fake clock, and a layering rule; verify an automated test rejects infrastructure dependencies in domain/UI.

## 3. Configuration, database, and secrets

- [ ] 3.1 Implement schema/validation and CLI/env/dotenv/TOML/SQLite/default resolution; verify all combinations, provenance, and failure before SIP (CFG-01, CFG-02).
- [ ] 3.2 Implement SQLite repositories, XDG permissions, and durable editing without persisting effective configuration; verify restart, overrides, and enabled-account constraints (CFG-03, CFG-04).
- [ ] 3.3 Integrate Secret Service and ephemeral fallback; verify DB/log/export scanning, 0600 secret files, and missing-keyring behavior (CFG-05, CFG-06).
- [ ] 3.4 Implement transactional migrations and consistent SQLite backups; verify interruption, locks, full disk, corruption, and future schemas without file loss (CFG-07, CFG-08).
- [ ] 3.5 Implement TOML import/export with sanitized preview; verify atomicity and untouched legacy data (CFG-09).
- [ ] 3.6 Implement CLI `--doctor`, validation, import/export, and messages; verify secret-free JSON and diagnostics without registration/calls.

## 4. Complete release telephony

- [ ] 4.1 Consolidate registration, renewal, and per-transport backoff in the selected adapter; verify 401/407, wrong passwords, duplicate accounts, and no infinite retries (SIP-01, SIP-02).
- [ ] 4.2 Implement incoming/outgoing calls, acceptance/rejection, BYE/CANCEL; verify both directions and the CANCEL/200 OK race without ghost sessions (SIP-03, SIP-04).
- [ ] 4.3 Implement busy and DND with coherent state; verify a second incoming call preserves the active call and DND does not interrupt it (SIP-05, SIP-07).
- [ ] 4.4 Consolidate DTMF and hold/resume re-INVITE; verify the IVR recognizes the sequence and audio is restored (SIP-06).
- [ ] 4.5 Consolidate blind transfer with NOTIFY; verify final success and an invalid destination without losing the original call when its dialog is preserved (SIP-08).
- [ ] 4.6 Implement TLS/custom-CA validation and network/suspend recovery; verify negative certificates, bounded retries, and no automatic redial (SIP-09, SIP-10).

## 5. Production audio and media

- [ ] 5.1 Consolidate RTP, G.711, resampling, and a bounded jitter buffer; verify loss/jitter/reordering through a seeded user-space UDP proxy, incompatible codecs, and bounded buffers without runner network privileges (AUD-01, AUD-02).
- [ ] 5.2 Implement capture/playback/ringing device selection and persistence by ID; verify headsets, missing devices, permissions, and confirmation of a new microphone on hotplug (AUD-03).
- [ ] 5.3 Implement local testing and mute; verify voice is blocked at the peer without interrupting reception, and no blocking/allocation in the callback (AUD-04).
- [ ] 5.4 Consolidate mandatory SRTP/SDES in the TLS profile; verify downgrade rejection and no key leakage (AUD-05).
- [ ] 5.5 Implement STUN/rport/keepalive/symmetric RTP and a manual procedure with a NAT router VM and STUN server on an isolated network; verify two-way audio, SDP, and teardown without changing the personal host, documenting no TURN support (AUD-06).

## 6. Contacts and history

- [ ] 6.1 Implement SQLite contacts, multiple numbers, and favorites; verify persistence, search, and selection before dialing (CH-01).
- [ ] 6.2 Implement bounded, transactional CSV preview/import; verify duplicates, UTF-8, size, and invalid records without partial writes (CH-02).
- [ ] 6.3 Persist session outcomes, monotonic duration, and crash recovery; verify session uniqueness and real outcomes (CH-03).
- [ ] 6.4 Implement filters, local search, 90-day retention, and confirmed cleanup; verify zero search traffic and removal of active entries (CH-04).

## 7. Compact MicroSIP UI and themes

- [ ] 7.1 Complete MicroSIP Lite/Wine references and official-help research in an isolated fixture-based environment without personal settings; deliver a per-screen/theme matrix and record scope exceptions (ND-01, ND-02).
- [ ] 7.2 Implement the compact window, Dialpad/Calls/Contacts tabs, 3×4 keypad, status/menu, list search, and tokens; verify snapshots against the approved Rust mock and MicroSIP flow similarity with explained differences (ND-01).
- [ ] 7.3 Implement contact, dialpad, incoming-call, active/hold/transfer, and error screens connected to real commands; verify backend-derived state and unavailable deferred features (ND-02, ND-06).
- [ ] 7.4 Implement validated settings and light/dark/system themes; verify persistence, switching during a call, and missing theme service (ND-03, ND-04).
- [ ] 7.5 Implement focus, labels, keyboard navigation, contrast, and scalable layout; verify 1024x768 and 100/150/200% scaling with essential actions accessible (ND-05).

## 8. Linux integration and shutdown

- [ ] 8.1 Implement optional notifications and tray; verify desktops with and without support and always-accessible incoming calls (LIN-01).
- [ ] 8.2 Implement URI parsing and opt-in integration; verify confirmation, CR/LF, long input, and rejected shell commands (LIN-02).
- [ ] 8.3 Implement single instance and protected local IPC; verify a second instance causes neither new registration nor destructive database contention (LIN-03).
- [ ] 8.4 Implement close/hide/quit, confirmation, and bounded shutdown; verify exit during a call, SIGTERM, an unresponsive PBX, and released audio (LIN-04).

## 9. Build, security, and release candidate

- [ ] 9.1 Complete the pinned Docker builder, offline fetch/build, and export; verify a clean-clone build without base or a host toolchain (DIST-03).
- [ ] 9.2 Implement ELF/AppImage packaging, licenses, and runtime auditing; verify both artifacts separately, including AppImage without FUSE (DIST-01, DIST-02).
- [ ] 9.3 Implement CI for fmt/clippy/test/doc, layering, audit/deny, and bounded fuzzing; verify injected failures block the pipeline and every public API is documented.
- [ ] 9.4 Implement publication/artifact scanning and the `base/` protection rule; verify negative tests with a forbidden path or fictional secret (DIST-07).
- [ ] 9.5 Run the complete Asterisk suite with per-scenario reports and build metadata; verify all relevant SIP/AUD/CFG test-plan rows without inherited approvals.
- [ ] 9.6 Run the manual checklist on the exact ELF and AppImage in clean GNOME/Wayland and KDE/X11 VMs, connected to the auxiliary PBX and a second human endpoint through the isolated G4 network; verify headsets, five-minute two-way audio per codec, receipts per hash/theme/transport, and no extra packages in client VMs (AUD-01, DIST-01, DIST-02, DIST-06).
- [ ] 9.7 Measure startup p95/RSS/stability and review the visual matrix with an operator; verify design budgets and no critical usability/audio defects.
- [ ] 9.8 Run final Claude CLI review with sanitized context; verify the blocker report and resolution, distinguishing review from runtime evidence (DIST-06).
- [ ] 9.9 Prepare SBOM, checksums, notes, limitations, Docker guide, and rollback; verify each artifact matches the candidate commit/tag and contains no personal data (DIST-07).
- [ ] 9.10 Publish v0.1.0 only after all gates in this change and AG-01..06 in the CLI/MCP extension's G3b, plus G4/G5 and the owner's visibility decision; verify the full checklist, download/checksums, and no claim of full parity.
