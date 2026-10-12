## Context

See [proposal.md](proposal.md) and [reference-sources](../../../docs/reference-sources.md). The initial directory contained references, not an own Rust workspace. MicroSIP guides compact UI; examined GOnnect surfaces use Qt/PJSIP and guide telephony/desktop cases; PhotoCraft demonstrates Rust/egui separation, not a reusable SIP engine.

This document proposes architecture. Candidate libraries have not yet been proven to meet telephony, secure-media, and packaging requirements. The first milestone is a vertical proof with stop criteria, before investing in complete UI.

## Goals / Non-Goals

**Goals:** separate domain from I/O; preserve responsiveness; test telephony without GUI; maintain one application process/executable; audit actual Linux dependencies; demonstrate calls on the distributed artifact.

**Non-Goals:** port PhotoCraft's engine, translate Qt/QML line-by-line, embed Asterisk, implement a PBX/generic framework, or claim pure-Rust SQLite/Linux drivers. Own application code is Rust; system libraries/native embedded SQLite are audited dependencies. Functional scope/deferrals remain in `proposal.md`.

## Decisions

### 1. Native UI and command architecture

Use egui/eframe as the initial direction, wgpu rendering and gate-validated graphics fallback. PhotoCraft guides workspace/engine-UI boundaries; do not copy its image crates or private `craft-*` dependencies. Rejected alternatives: Tauri/Electron for web runtime; primary GTK/Qt for increased toolkit dependencies and divergence from the requested Rust foundation. If keyboard, basic accessibility, or essential fidelity fails, review this decision before final screens.

**Planned** structure, not proof of implementation:

```text
apps/rust-microsip/    composition, diagnostic CLI, desktop entry
crates/core/          accounts, contacts, states, commands, events, interfaces
crates/telephony/     SIP adaptation, registration, dialogs, transactions
crates/media/         RTP/SRTP, codecs, jitter, resampling, devices
crates/config/        sources, validation, precedence, sanitized views
crates/storage/       SQLite, migrations, repositories
crates/platform/      directory, secret, theme, notification, tray, IPC contracts
crates/platform-linux/ Linux contracts without leaking D-Bus into core
crates/ui/            widgets, screens, tokens; no SQL/direct SIP operations
crates/test-support/  fictional fixtures and controllable clock/transport
tests/                vertical integration/interoperability
packaging/            Linux build, ELF audit, AppImage
docker/               builder and Asterisk lab
docs/                 architecture, development, security, release
```

`core` has no UI/SQLite/adapter dependency. Adapters depend on domain; `ui` receives snapshots/dispatches commands; `apps/rust-microsip` composes implementations. Automated dependency tests block cycles/infrastructure imports into UI/domain.

`platform` exposes narrow directory/permission, secret storage, theme, notification, tray, and IPC transport interfaces. Public types expose no D-Bus, Linux descriptors, or Windows APIs. Explicit capabilities represent absent services; tests use fake adapters. `platform-linux` owns XDG/Secret Service/D-Bus/Unix IPC, target-selected in composition. macOS/Windows get appropriate adapters/storage/IPC in future releases, without support-advertising stubs. Telephony/media use their own portable adapters, not one OS superinterface.

CLI/MCP share services through the [linked change](../add-agent-cli-mcp/design.md). The owner has no SIP subprocesses; opt-in CLI/MCP invocations are same-binary IPC clients. The [future web version](../add-web-wasm-softphone/design.md) shares compatible domain only, never assumes Linux adapters compile to WASM.

### 2. Concurrency and states

GUI loop on the main thread. Tokio runtime in a dedicated worker, fixed-capacity command/event channels with queue-full feedback; `watch` snapshots coalesce state updates. Never drop final call events. A telephony actor owns dialogs; one exclusive worker owns SQLite. No disk I/O in audio callbacks or UI frames.

Registration states: disabled → registering → registered → recovering/failed. Call states: idle → dialing/ringing → established ↔ held → terminating → ended/failed. Transfer is a pending dialog-bound operation confirmed by NOTIFY. Events have session IDs/versions for deduplication; timers use monotonic time. History timestamps are UTC, rendered locally; duration is independent of wall-clock changes.

Audio callbacks use preallocated ring buffers without blocking mutexes, per-packet logging, or per-frame allocation. `CancellationToken` and task supervision control shutdown; bounded joins replace arbitrary sleeps. Normal exit/interruption must be distinguishable on next startup.

### 3. Telephony and mandatory technical gate

`rsipstack` is the first signaling candidate behind a narrow adapter. Standalone `rsip` parsing is not a full stack; PJSIP FFI is not the default because it retains C/C++ for the principal function. `rustrtc`/Rust RTP/SRTP components are media candidates; explicitly validate SIP SDES, not inferred from WebRTC support. Do not invent cryptography. G.711 suffices initially; Opus/advanced echo processing may follow. Initial quality profile is a headset, without promising acoustic-echo-canceled speakerphone use.

**Blocking G0:** register against Asterisk over UDP/TCP/TLS, complete G.711 calls in both directions, and test PCMA/PCMU, DTMF, hold, REFER/NOTIFY, TLS+SRTP/SDES. The spike ELF includes the same production eframe/wgpu, audio, D-Bus/notifications, and IPC stack, not merely headless telephony. Open a minimal window in GNOME/Wayland and KDE/X11, exercise a compatible renderer without dedicated Vulkan, focus/keyboard, audio enumeration/hotplug, and audit the dependency allowlist below. Keyboard/renderer failure blocks egui before complete UI. Compare candidates; pin versions/features in Cargo.lock/ADR. On failure, record the blocker/review this change, never silently substitute Qt/PJSIP, remove security, or replace the single ELF with AppImage.

Secure default: validated TLS hostname/CA, mandatory SRTP, SDES only over TLS. Corporate CA by explicit path. Insecure lab UDP/TCP+RTP requires explicit choice/warning. Never log keyed SDP, Authorization, or audio content.

STUN/rport/symmetric RTP cover restricted topologies; media-anchoring PBX/VPN is recommended. ICE/TURN, DTLS-SRTP, and arbitrary symmetric NAT are outside the initial contract.

### 4. Configuration, secrets, storage

Six-layer field resolution per `config-storage`: flags > environment > explicit `.env` > TOML > SQLite > defaults. Explicit `--config` replaces XDG TOML, not merges two files. Defaults: `$XDG_CONFIG_HOME/rust-microsip/config.toml`, `$XDG_DATA_HOME/rust-microsip/rust-microsip.sqlite3`, state/logs `$XDG_STATE_HOME/rust-microsip/`; use XDG defaults when unset. No implicit cwd dotenv. Versioned schema/keys, environment namespace `RUST_MICROSIP_`. Invalid configuration stops SIP startup but leaves diagnostics/settings accessible.

Persist `StoredConfig` in SQLite; compose immutable `EffectiveConfig`. Never persist the whole effective configuration containing overrides/secrets. Account/transport changes during calls require confirmed hangup or stay pending, never silently drop audio. Future same-executable CLI: `--help`, `--version`, `--config`, `--env-file`, `--doctor`, `config validate`, `config import`, `config export`; always-sanitized diagnostic JSON. Doctor never registers/dials.

Secret Service stores passwords; SQLite stores `secret_ref`. Without keyring: per-session prompt or explicit private file/environment, never homemade encryption with an adjacent key. Explicit private TOML literal passwords are accepted in memory only; recommend `password_env`, never export literals. No password CLI flags (history/process lists). 0700/0600 permissions include DB/WAL/SHM/backups; reject other-readable secret files. Sensitive Debug hides values; exports never contain secrets. Example TOML can use `password_env = "RUST_MICROSIP_SIP_PASSWORD"`, a `.invalid` hostname, no real values.

SQLite through bundled `rusqlite`, foreign keys, bounded busy timeout, parameterized queries, WAL. Exclusive worker serializes writes. Planned tables: `schema_migrations`, `accounts`, `preferences`, `contacts`, `contact_numbers`, `favorites`, `calls`. Stable IDs/FKs; one enabled account. Backups use SQLite's consistent API, not copying an open file without WAL. Transactional migration/interruption tests; downgrade requires compatible backup, never automatic destructive restoration.

Default history retention: 90 days. Deletion removes active records, not guaranteed forensic erasure of SQLite/WAL/backups. CSV preview/limits/transaction: each row is name+number; same-name rows aggregate distinct numbers into the imported contact, exact duplicates are skipped/count-reported. Existing-contact conflicts require preview decisions. No external address-book sync. DND uses 486 Busy Here in this release.

### 5. Measurable visual fidelity

Primary reference: MicroSIP Lite 3.22.16, resources/source and [official help](https://www.microsip.org/help), especially Single Call Mode. It opened in Wine 9/Xvfb in a container without network/accounts/personal devices/HOME, proving opening/layout only. GOnnect complements call lifecycle/failure/device/Linux integration references, not mandatory side dashboard/address book. PhotoCraft remains architectural.

One compact window, Dialpad/Calls/Contacts tabs, number field, 3×4 keypad, call/hangup, account status, settings menu, DND. Mute/hold/blind-transfer controls in call context; favorites in Contacts, no permanent right panel. Candidate initial size 360×560 logical points, resizable; approve own mock before fixing dimensions. No copied chat/video/conference/recording/auto-answer/auto-forwarding; omit or clearly disable out-of-scope controls. Do not import MicroSIP dial plans/arbitrary headers/automatic dialing.

Required matrix: three tabs, search, incoming/active/hold/transfer calls, settings, registration errors × light/dark, plus system/keyboard/100–200%. Similarity means hierarchy/flow, not Win32 pixel identity or unrestricted MicroSIP compatibility. Human approval records intentional differences; synthetic snapshots regress against the approved Rust mock, not Windows decoration. Scroll at 1024×768/HiDPI when needed.

Tokens centralize colors, typography, spacing, radii, states. Examined Win32 resources name Microsoft Sans Serif but no font files were found; never redistribute proprietary Windows fonts. Select similarly proportioned open fonts/licensed icons/tones and embed with attribution. No copied assets/C++ without auditing. Deterministic fixtures/fixed clock enable snapshots; contrast/interaction are separate tests.

### 6. Single-binary contract and Docker build

Primary artifact: `rust-microsip` ELF containing application code/resources. Static SQLite/embedded assets; no sidecars, SIP subprocesses, application plugins, font downloads, or loose own libraries. **Not a promise of 100% static ELF or universal Linux.** Host provides kernel, loader/glibc, graphics/audio services/drivers. Initial allowlist includes only supported clean-desktop libraries: loader/glibc, X11/Wayland/xkbcommon interfaces, graphics loader/GPU driver, and ALSA/PulseAudio per G0-selected backend. Final manifest pins SONAMEs/proves their presence in reference desktops. Out-of-allowlist dependencies require embedding or review, never hidden apt-install instructions.

AppImage is additional convenience with the same core/documented FUSE-free fallback, not ELF replacement. Audit indirect/dlopen libraries, not just ldd/DT_NEEDED. Test real drivers and no dedicated Vulkan; no specific GPU requirement without support review.

Multi-stage digest-pinned Ubuntu 24.04 builder, pinned stable toolchain in rust-toolchain.toml, locked Cargo.lock; version/SHA-256 tool downloads. Fetch followed by offline/frozen build. BuildKit scratch artifact export, no container UI/audio/host-desktop access. Docker context excludes base/git/private dotenv/databases/artifacts. Host needs Docker Engine/BuildKit; compiling with no tools whatsoever is impossible.

Contract command (the scaffold implements only its current subset):

```sh
docker buildx build --target artifacts --output type=local,dest=dist .
```

Full output: ELF, AppImage, SHA256SUMS, SBOM, licenses/attributions, ELF/runtime manifest, commit/toolchain/digests, test receipt. Initial reproducibility means pinned auditable inputs, not unmeasured bitwise identity. Add comparative rebuilds/document divergence. CI uses builder paths and promotes only after gates.

### 7. Docker Asterisk lab

Implement `docker/asterisk/` with Dockerfile, fictional configs, Compose, runner, README. Asterisk 22 LTS, supported patch/source fixed by SHA-256 or pinned snapshot package. At the user's 2026-10-12 request, use the exact version/digest-pinned [andrius/asterisk image](../../../docs/adr/0002-disposable-asterisk-lab.md); this pins the test runtime without claiming independently reproduced upstream package inputs. Never community `latest`. Asterisk is a separate test component, never embedded in the client.

Default topology: Compose `internal: true`, `pbx`, `peer`, `test-runner`, no ports/host networking/Docker socket/HOME/DBus/devices/privileges. Runner tests real headless adapters using synthetic sources, never substitutes desktop/hardware acceptance. PJSIP extensions 1001/1002, 600 echo, 601 IVR/DTMF, 602 reference tone. No outbound/PSTN/anonymous SIP/remote HTTP or AMI administration. Transfer to valid extension/route and nonexistent destination proves success/failure.

Local generator creates random fictional passwords and ephemeral CA/certificates under ignored `artifacts/asterisk-lab/`. PBX/runner receive only selected lab subdirectories. Healthcheck proves PJSIP readiness; runner verifies registration, not merely open ports. Internal TLS 5061, UDP/TCP 5060, RTP UDP 10000–10019 including RTCP. No default recording; debug captures contain fictional traffic only and are excluded from publication.

Opt-in `compose.host.yaml` removes PBX from the internal network onto a separate **non-internal** bridge, never host networking. Publish only `127.0.0.1:15060` → 5060 UDP/TCP, `127.0.0.1:15061` → 5061 TCP, `127.0.0.1:10000–10019` → same UDP range. Advertise 127.0.0.1 externally for media/signaling, with local_net/external_media_address/external_signaling_address/external port and direct_media=no/rtp_symmetric=yes/force_rport=yes/rewrite_contact=yes adjusted to the bridge. Validate SDP both ways: port mapping alone does not solve audio. Automation uses internal addresses/ports without loopback advertisement. Separate transport/encryption and RTP-only negative fixtures prevent implicit endpoint mutation.

**G4 clean-VM topology:** Docker PBX on an auxiliary lab VM, not acceptance desktops. Opt-in `compose.vm-lab.yaml` replaces loopback bind/advertisement with `$LAB_BIND_IP`, the auxiliary VM's isolated host-only interface, no production-LAN bridge/external route, firewall allowing client/peer test IPs only. Never bind 0.0.0.0. This G4-only exception leaves internal/loopback defaults unchanged. Desktop VMs receive artifacts/fictional data only, no Docker/Rust/extra packages. Operator A uses the app/headset on a clean VM; operator B a reference softphone/own headset on another isolated station. Sanitized receipt identifies both endpoints/two-way voice. Synthetic peers do not replace human acceptance.

**Impairment/NAT:** AUD-02 uses seeded user-space UDP proxy for loss/jitter/reordering, no tc/CAP_NET_ADMIN in default containers. AUD-06 is a reproducible manual isolated topology: client VM behind disposable Linux NAT-router VM, PBX/STUN on the other side. Provision NAT only there, never change personal-host routes/firewall. Document addresses/SDP/local STUN bind, hash-linked receipt, teardown. Default Compose does not demonstrate NAT/STUN. No default privileged/network-administration capability.

Full contract commands; check implementation receipts for which portions exist:

```sh
docker compose -f docker/asterisk/compose.yaml up --build --abort-on-container-exit --exit-code-from test-runner
docker compose -f docker/asterisk/compose.yaml -f docker/asterisk/compose.host.yaml up --build pbx
LAB_BIND_IP=<auxiliary-vm-host-only-ip> docker compose -f docker/asterisk/compose.yaml -f docker/asterisk/compose.vm-lab.yaml up --build pbx
docker compose -f docker/asterisk/compose.yaml down --volumes --remove-orphans
```

Host/VM profiles are alternatives; never stack their overrides. Pin a Compose version supporting explicit list replacement (`!override`/`!reset`) to prevent merged default networks/ports. Teardown repeats the same -f files and removes that profile's network. Auxiliary-VM firewall must filter Docker-forwarded traffic (DOCKER-USER for iptables/equivalent nftables), not just INPUT/UFW; prove unauthorized-IP denial. AUD-06 ACL includes NAT-router external IP. Verify bridge-gateway source classification does not bypass host SDP rewriting through local_net.

Tests: three-transport registration/renewal, both call directions, 401/407, busy, CANCEL/200 race, DTMF, hold, REFER/NOTIFY, invalid TLS, mandatory SRTP, network recovery. Synthetic tests measure tone/RMS/frequency both ways; echo alone does not prove independent endpoints. Later manual tests confirm real microphone/playback on exact ELF/AppImage.

### 8. Quality, license, publication

`//!` explains crates/modules; `///` documents public APIs/errors/compilable examples. thiserror in libraries, binary context, no production unwrap/expect/todo. Own unsafe_code=forbid except isolated justified/audited FFI adapter; explicitly inventory third-party native/unsafe dependencies. Minimal Tokio features, limits/cancellation; no async runtime in audio callbacks.

Gates: fmt, clippy warnings-as-errors, tests/doctests, rustdoc warnings-as-errors, evaluated cargo-deny/audit vulnerabilities, state/config/URI/CSV properties, parser/adapter fuzzing, Asterisk integration, snapshots/keyboard, VM smoke. See [test-plan.md](test-plan.md). Initial 2-vCPU/4-GiB VM budget: warm-cache startup p95 ≤3 seconds, idle RSS ≤200 MiB, interactive UI during network failure; measure baseline before enforcing regressions. Target 80% domain/config/storage line coverage, never replacing critical I/O scenarios.

Propose GPL-3.0-or-later for own code, not assumed MIT when using GPL references. Audit any MicroSIP incorporation (site GPLv2; examined headers GPL-2.0-or-later, confirm each file), GOnnect, and dependency license choices; preserve notices, never copy brands/logos as an official product. No upstream source was copied into application implementation at planning time. G0 licensing decisions precede code/asset incorporation.

Initially private, owner jniltinho; no visibility change without instructions. User now requests commits/pushes at verified implementation milestones. Audit staged content before every push: no base paths, submodules, secrets, or personal artifacts. CI repeats this once implemented. Tags/releases only after gates; compilation alone cannot justify “usable release” in GitHub Actions.

## Risks / Trade-offs

- [Rust stack unproven across SIP/SDES] → vertical G0, ADR, scope review on failure; no production mocks filling gaps.
- [Hidden audio/graphics libraries break one-binary contract] → explicit allowlist, dlopen audit, clean-VM calls; separate AppImage tests.
- [egui does not automatically match Win32/Qt accessibility/visuals] → keyboard/contrast/scaling/essential matrix before expansion.
- [NAT/Docker establishes calls without audio] → anchored media, separate internal/host profiles, SDP/two-direction tones.
- [No AEC can cause speaker echo] → initial headset quality profile, published limitation, no professional speakerphone promise.
- [Tray/portals vary] → accessible-window fallback without mandatory extensions.
- [SQLite/logs contain personal metadata] → permissions, retention, sanitization, no real data in tests/GitHub.
- [Full parity overwhelms first release] → same essential-screen experience with recorded exceptions/separate deferred integrations.

## Migration Plan

1. Approve scope and complete G0 receipts before full implementation.
2. Create workspace, fictional data, isolated labs; build tested vertical slices.
3. Independent XDG identifier rust-microsip, never read/change legacy gonnect/MicroSIP data.
4. Private release candidate; exact-artifact GNOME/Wayland/KDE/X11 tests.
5. v0.1.0 only with complete checklist/licenses/tests; user decides eventual public visibility.
6. Rollback: prior binary/compatible Rust-profile backup; reject future schema and preserve current database. Original GOnnect installation remains available/untouched.
