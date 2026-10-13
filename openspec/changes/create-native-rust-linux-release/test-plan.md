# Test plan and acceptance criteria

**Status (2026-10-13):** each row below names its current evidence. Lab rows come from the versioned suite `docker/asterisk/suite.sh` (per-scenario reports bound to the commit and binary hashes; run `20261013T171518Z-907d96b`, see the [lab suite receipt](../../../docs/receipts/lab-suite-2026-10-13.md)); rows marked pending need VMs, real devices or human operators. The proposed test names are traceability labels, not literal function names. Every OpenSpec scenario has a stable ID and traceability row. OpenSpec validation, scaffold tests, and Claude review do not prove a working softphone.

## Gates and order

1. G0: vertical SIP + G.711 + TLS/SRTP + Docker/ELF proof; blocks complete UI until passed.
2. G1: domain/configuration/persistence, RED → GREEN → refactor, doctests.
3. G2: isolated Asterisk suite, real SIP flows, two-direction synthetic media.
4. G3: compact MicroSIP flow, approved Rust-mock snapshots, keyboard/themes, GNOME/Wayland and KDE/X11 integration.
5. G3b: AG-01..05 and automated AG-06 CLI/MCP (owner/IPC, schemas, GUI/grant authorization, ledger/restart, concurrency, Asterisk controls); mandatory Linux-agent candidate gates.
6. G4: human AG-06 in GUI/service on the same host-only topology, separate mode/hash receipts; exact ELF/AppImage in clean VMs, headsets, five-minute real calls per codec, hash-linked receipts.
7. G5: independent Claude review, licenses, security, SBOM, Git audit, release decision.

## Execution matrix

| Environment | Purpose | Required evidence |
|---|---|---|
| Docker, no ports, internal network | Automated PBX/peer/runner | Readiness, SIP states, DTMF/transfer outcomes, tone metrics |
| Opt-in Docker loopback, non-internal bridge | Host-application PBX | Local binds, correct SDP, anchored audio |
| Auxiliary-VM Docker PBX, opt-in G4 host-only | Clean-VM/human-peer calls | Explicit-IP bind, lab ACL, no LAN/Internet |
| Ubuntu 24.04 x86_64 GNOME/Wayland | Primary desktop support | Keyboard/themes/scaling, ELF/AppImage calls |
| Ubuntu 24.04 x86_64 KDE/X11 | Primary desktop support | Same evidence, with/without tray |
| VM without packages beyond standard desktop | Distribution contract | Qt-free ELF dependencies, audited libraries, no first-run resource downloads, real calls |

Test explicitly insecure UDP/TCP RTP and mandatory-SRTP TLS using separate endpoint fixtures. Test PCMA/PCMU, valid/expired/untrusted-CA/wrong-hostname certificates, and missing Secret Service, including KDE/KWallet without Secrets service. In G4 operator A uses clean VM/app/headset, operator B a reference softphone/headset on the isolated network; client VMs receive no Docker/toolchain. Signaling runners do not prove physical microphone capture/playback. Never record conversations; fixtures/tones are synthetic.

AUD-02 uses an unprivileged user-space UDP impairment proxy. AUD-06 needs dedicated manual topology: disposable NAT-router VM, client behind it, PBX/STUN across the isolated network; default Compose does not cover it. Record addressing/SDP without changing personal-host routes/firewall.

## Complete scenario traceability

| Scenario | Proposed test | Method | Status |
|---|---|---|---|
| [AUD-01](specs/audio-media/spec.md) — bidirectional voice | `test_aud_01_bidirectional_voice` | Media integration + real VM hardware | lab: two-way synthetic tones over RTP and SRTP (engine-lab, suite `20261013T171518Z-907d96b`); real headsets and human five-minute calls pending (9.6) |
| [AUD-02](specs/audio-media/spec.md) — degraded network and incompatible codec | `test_aud_02_degraded_network_and_incompatible_codec` | Media integration + real VM hardware | integration: seeded impairment relay (1% loss, 30 ms jitter, reordering) with bounded buffers; incompatible codec rejected (488 / `NotAcceptable`) in unit tests |
| [AUD-03](specs/audio-media/spec.md) — selection and headset removal | `test_aud_03_selection_and_headset_removal` | Media integration + real VM hardware | pending: real devices and hotplug (5.2) |
| [AUD-04](specs/audio-media/spec.md) — test and mute | `test_aud_04_test_and_mute` | Media integration + real VM hardware | lab: muted peer hears silence while the muted side keeps hearing (engine-lab, suite `20261013T171518Z-907d96b`); local test on real devices pending (5.3) |
| [AUD-05](specs/audio-media/spec.md) — mandatory SRTP | `test_aud_05_mandatory_srtp` | Media integration + real VM hardware | lab: TLS+SRTP calls, plaintext offers rejected, downgrade rejection (engine-lab, suite `20261013T171518Z-907d96b`) |
| [AUD-06](specs/audio-media/spec.md) — supported topology | `test_aud_06_supported_topology` | Media integration + real VM hardware | pending: NAT/STUN VM topology (5.5) |
| [CFG-01](specs/config-storage/spec.md) — precedence and provenance | `test_cfg_01_precedence_and_provenance` | Unit/property + SQLite/filesystem integration | unit: precedence combinations with provenance (`crates/config`) |
| [CFG-02](specs/config-storage/spec.md) — invalid file | `test_cfg_02_invalid_file` | Unit/property + SQLite/filesystem integration | unit: invalid files rejected without values in errors (`crates/config`) |
| [CFG-03](specs/config-storage/spec.md) — durable setting | `test_cfg_03_durable_setting` | Unit/property + SQLite/filesystem integration | unit/integration: only user edits persisted in SQLite (`crates/config`, `crates/storage`) |
| [CFG-04](specs/config-storage/spec.md) — active external override | `test_cfg_04_active_external_override` | Unit/property + SQLite/filesystem integration | unit: a field set by a higher layer is refused with its source named |
| [CFG-05](specs/config-storage/spec.md) — credentials and missing keyring | `test_cfg_05_credentials_and_missing_keyring` | Unit/property + SQLite/filesystem integration | integration: Secret Service round trip in a disposable gnome-keyring session and session-only fallback; real desktops and KWallet pending (9.6) |
| [CFG-06](specs/config-storage/spec.md) — insecure secret file | `test_cfg_06_insecure_secret_file` | Unit/property + SQLite/filesystem integration | unit/process: insecure secret files refused |
| [CFG-07](specs/config-storage/spec.md) — interrupted migration | `test_cfg_07_interrupted_migration` | Unit/property + SQLite/filesystem integration | integration: injected mid-migration failure leaves the old version and a backup |
| [CFG-08](specs/config-storage/spec.md) — unavailable or future database | `test_cfg_08_unavailable_or_future_database` | Unit/property + SQLite/filesystem integration | integration: locked, full, corrupted and future-version databases |
| [CFG-09](specs/config-storage/spec.md) — import without touching legacy data | `test_cfg_09_import_without_touching_legacy_data` | Unit/property + SQLite/filesystem integration | process: `config import` preview and atomic apply; the source is never modified |
| [CH-01](specs/contacts-history/spec.md) — contact and favorite | `test_ch_01_contact_and_favorite` | Unit + SQLite/CSV integration | integration: contacts with several numbers and favorites (`crates/storage`, UI tests) |
| [CH-02](specs/contacts-history/spec.md) — invalid and duplicate CSV | `test_ch_02_invalid_and_duplicate_csv` | Unit + SQLite/CSV integration | integration: bounded CSV preview with per-line issues, all-or-nothing import |
| [CH-03](specs/contacts-history/spec.md) — outcomes and recovery | `test_ch_03_outcomes_and_recovery` | Unit + SQLite/CSV integration | integration: outcomes from end reasons; simulated kill marks open calls `Interrupted` |
| [CH-04](specs/contacts-history/spec.md) — search and retention | `test_ch_04_search_and_retention` | Unit + SQLite/CSV integration | integration: literal search and 90-day retention |
| [DIST-01](specs/reproducible-distribution/spec.md) — clean machine | `test_dist_01_clean_machine` | Build/isolation/artifact + release checklist | pending: clean GNOME/Wayland and KDE/X11 VMs (9.6) |
| [DIST-02](specs/reproducible-distribution/spec.md) — separate ELF and AppImage | `test_dist_02_separate_elf_and_appimage` | Build/isolation/artifact + release checklist | build: ELF and AppImage verified separately, AppImage without FUSE; clean desktops pending (9.2) |
| [DIST-03](specs/reproducible-distribution/spec.md) — clean-clone build | `test_dist_03_clean_clone_build` | Build/isolation/artifact + release checklist | build: clean-clone build with identical `SHA256SUMS` on rebuild (`packaging/release/clean-clone-build.sh`) |
| [DIST-04](specs/reproducible-distribution/spec.md) — lab and isolation | `test_dist_04_lab_and_isolation` | Build/isolation/artifact + release checklist | lab: no published ports, internal network, per-scenario teardown with no leftover containers (suite `20261013T171518Z-907d96b`) |
| [DIST-05](specs/reproducible-distribution/spec.md) — manual desktop profile | `test_dist_05_manual_desktop_profile` | Build/isolation/artifact + release checklist | pending: host profile and G4 host-only VM profile (1.5) |
| [DIST-06](specs/reproducible-distribution/spec.md) — incomplete release blocked | `test_dist_06_incomplete_release_blocked` | Build/isolation/artifact + release checklist | process: release draft keeps every open gate unchecked; no tag or download exists |
| [DIST-07](specs/reproducible-distribution/spec.md) — pre-push audit | `test_dist_07_pre_push_audit` | Build/isolation/artifact + release checklist | CI: publication scan with negative controls on every push |
| [LIN-01](specs/linux-integration/spec.md) — desktop without tray | `test_lin_01_desktop_without_tray` | Desktop/IPC integration + URI tests | partial: notifications over D-Bus implemented; real desktops with and without tray pending (8.1) |
| [LIN-02](specs/linux-integration/spec.md) — legitimate and malicious links | `test_lin_02_legitimate_and_malicious_links` | Desktop/IPC integration + URI tests | unit + GUI: strict link parsing; a link only fills the number (gui-link, suite `20261013T171518Z-907d96b`) |
| [LIN-03](specs/linux-integration/spec.md) — second instance | `test_lin_03_second_instance` | Desktop/IPC integration + URI tests | GUI: second launch forwards and exits 0, one call only after Enter (gui-link, suite `20261013T171518Z-907d96b`) |
| [LIN-04](specs/linux-integration/spec.md) — exit during a call | `test_lin_04_exit_during_a_call` | Desktop/IPC integration + URI tests | GUI: SIGTERM during a call with the PBX paused exits in 2.5 s (gui-frozen-exit, suite `20261013T171518Z-907d96b`); audio release on real devices pending (8.4) |
| [ND-01](specs/native-desktop/spec.md) — comparable layout | `test_nd_01_comparable_layout` | Snapshot/keyboard + VM visual review | pending: visual matrix against MicroSIP (7.1, 7.2) |
| [ND-02](specs/native-desktop/spec.md) — feature outside the release | `test_nd_02_feature_outside_the_release` | Snapshot/keyboard + VM visual review | pending: deferred-feature screens (7.3) |
| [ND-03](specs/native-desktop/spec.md) — theme switching | `test_nd_03_theme_switching` | Snapshot/keyboard + VM visual review | GUI: dark theme from Settings during a call, call kept, persisted after relaunch (gui-theme, suite `20261013T171518Z-907d96b`) |
| [ND-04](specs/native-desktop/spec.md) — system theme | `test_nd_04_system_theme` | Snapshot/keyboard + VM visual review | GUI: "System" without a theme service falls back to light (gui-theme, suite `20261013T171518Z-907d96b`); real desktops pending (9.6) |
| [ND-05](specs/native-desktop/spec.md) — keyboard and HiDPI | `test_nd_05_keyboard_and_hidpi` | Snapshot/keyboard + VM visual review | GUI: AT-SPI sweep of seven screens and a call with 0 unnamed controls, Tab cycle, WCAG contrast test, 100/150/200% at 1024x768 (gui-a11y, gui-a11y-call, suite `20261013T171518Z-907d96b`); Orca on real desktops pending (9.6) |
| [ND-06](specs/native-desktop/spec.md) — registration failure | `test_nd_06_registration_failure` | Snapshot/keyboard + VM visual review | unit + GUI: registration failures shown in the status bar (`registration_text` tests, captures) |
| [SIP-01](specs/sip-telephony/spec.md) — registration and renewal | `test_sip_01_registration_and_renewal` | Asterisk integration + dialog/state assertions | lab: registration and renewal past a 60 s binding (engine-network, suite `20261013T171518Z-907d96b`) |
| [SIP-02](specs/sip-telephony/spec.md) — invalid configuration and authentication | `test_sip_02_invalid_configuration_and_authentication` | Asterisk integration + dialog/state assertions | lab: wrong password fails without retries (engine-lab, suite `20261013T171518Z-907d96b`); invalid settings rejected (unit) |
| [SIP-03](specs/sip-telephony/spec.md) — calls in both directions | `test_sip_03_calls_in_both_directions` | Asterisk integration + dialog/state assertions | lab: calls in both directions (engine-lab, suite `20261013T171518Z-907d96b`) |
| [SIP-04](specs/sip-telephony/spec.md) — cancellation and answer race | `test_sip_04_cancellation_and_answer_race` | Asterisk integration + dialog/state assertions | lab: CANCEL racing 200 OK leaves no call on either side (engine-lab, suite `20261013T171518Z-907d96b`) |
| [SIP-05](specs/sip-telephony/spec.md) — rejection and second call | `test_sip_05_rejection_and_second_call` | Asterisk integration + dialog/state assertions | lab: reject and 486 for a second call while the first keeps audio (engine-lab, suite `20261013T171518Z-907d96b`) |
| [SIP-06](specs/sip-telephony/spec.md) — DTMF and hold | `test_sip_06_dtmf_and_hold` | Asterisk integration + dialog/state assertions | lab: RFC 4733 DTMF and hold/resume (engine-lab, suite `20261013T171518Z-907d96b`) |
| [SIP-07](specs/sip-telephony/spec.md) — do not disturb | `test_sip_07_do_not_disturb` | Asterisk integration + dialog/state assertions | lab: DND refuses calls (engine-lab, suite `20261013T171518Z-907d96b`) |
| [SIP-08](specs/sip-telephony/spec.md) — transfer and failure | `test_sip_08_transfer_and_failure` | Asterisk integration + dialog/state assertions | lab: blind transfer with NOTIFY and a failed transfer keeping the call (engine-lab, suite `20261013T171518Z-907d96b`) |
| [SIP-09](specs/sip-telephony/spec.md) — invalid certificate | `test_sip_09_invalid_certificate` | Asterisk integration + dialog/state assertions | lab: untrusted CA, wrong hostname and unusable bundles never register (engine-lab, suite `20261013T171518Z-907d96b`) |
| [SIP-10](specs/sip-telephony/spec.md) — network and suspend | `test_sip_10_network_and_suspend` | Asterisk integration + dialog/state assertions | lab: 40 s network loss ends the call without redial and registration recovers (engine-network, suite `20261013T171518Z-907d96b`); suspend/resume on real desktops pending (9.6) |

Total: **46 scenarios**; current evidence per row above. Split composite cases into explicit success/failure tests without removing traceability.

## Quality and additional cases

- Properties: transitions never create two calls; duplicate events never duplicate history; invalid values never trigger SIP; malformed URI/CSV/SDP never panic.
- Cancellation/saturation: full queues, shutdown without PBX, network loss, suspend, worker failure; no unbounded wait or UI/audio locks.
- SQLite: consistent WAL backup, interrupted migration, constraints/FKs, future schema, full disk, permissions, preserved data.
- Audio: correct muted silence, RTP sequence/timestamps, bounded buffers, no alternative-mic hotplug capture, jitter/loss, incompatible codecs.
- Performance: p95 over at least 20 warm-cache starts, idle RSS after 60 seconds, interaction under network failure; report hardware/drivers/design budgets.
- Security: scan dotenv, DB/WAL, exports, Debug, logs, artifacts for fictional secret markers; no lab emergency/PSTN calls.
- Build: ldd/readelf and observed dynamic loads including audio/graphics; no loose own libraries/external assets.
- Publication: clean clone without base; audit tracked/staged files/artifacts; no local-reference submodules or personal home paths.

Planned implementation quality commands; scaffold receipts identify the subset already executed:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --doc --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo deny check
cargo audit
```

Target coverage: 80% domain/config/storage lines. Coverage/mocks replace no SIP/media/distribution scenario. Added features need an explicit CI matrix.

## Per-execution receipt

Retain sanitized reports only: commit, ELF/AppImage hash, OS/desktop/driver, codec/transport, devices without personal identifiers, scenario, command, exit code, UTC time, metrics, outcome, operator. Manual evidence identifies both voice directions, not just echo/open window. Critical failures keep gates pending. Raw outputs/pcaps/keys/DBs/recordings stay out of Git/releases.

## Proposal validation

See [validation.md](validation.md) for structural validation/independent analysis/publication audit. Claude receives sanitized documents/contracts with tools disabled, never personal configuration/AppImage access.

## Linked extensions

[CLI/MCP](../add-agent-cli-mcp/tasks.md) adds mandatory scenarios/tasks for the requested Linux-agent candidate. These 46 scenarios cover desktop only. [Web/WASM](../add-web-wasm-softphone/proposal.md) is a later release with independent browser/WebRTC criteria. Platform-contract tests use fake and real Linux adapters; macOS/Windows are outside the current matrix.
