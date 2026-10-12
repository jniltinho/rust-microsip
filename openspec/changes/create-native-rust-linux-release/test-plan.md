# Test plan and acceptance criteria

**Status: all 46 functional scenarios below remain pending.** Implementation started with the G0 scaffold on 2026-10-12, with CLI/build tests but no application telephony/desktop; see the [receipt](../../../docs/receipts/g0-scaffold-2026-10-12.md). Names below are proposed contracts, not implemented scenario tests. Every OpenSpec scenario has a stable ID and traceability row. OpenSpec validation, scaffold tests, and Claude review do not prove a working softphone.

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
| [AUD-01](specs/audio-media/spec.md) — bidirectional voice | `test_aud_01_bidirectional_voice` | Media integration + real VM hardware | pending |
| [AUD-02](specs/audio-media/spec.md) — degraded network and incompatible codec | `test_aud_02_degraded_network_and_incompatible_codec` | Media integration + real VM hardware | pending |
| [AUD-03](specs/audio-media/spec.md) — selection and headset removal | `test_aud_03_selection_and_headset_removal` | Media integration + real VM hardware | pending |
| [AUD-04](specs/audio-media/spec.md) — test and mute | `test_aud_04_test_and_mute` | Media integration + real VM hardware | pending |
| [AUD-05](specs/audio-media/spec.md) — mandatory SRTP | `test_aud_05_mandatory_srtp` | Media integration + real VM hardware | pending |
| [AUD-06](specs/audio-media/spec.md) — supported topology | `test_aud_06_supported_topology` | Media integration + real VM hardware | pending |
| [CFG-01](specs/config-storage/spec.md) — precedence and provenance | `test_cfg_01_precedence_and_provenance` | Unit/property + SQLite/filesystem integration | pending |
| [CFG-02](specs/config-storage/spec.md) — invalid file | `test_cfg_02_invalid_file` | Unit/property + SQLite/filesystem integration | pending |
| [CFG-03](specs/config-storage/spec.md) — durable setting | `test_cfg_03_durable_setting` | Unit/property + SQLite/filesystem integration | pending |
| [CFG-04](specs/config-storage/spec.md) — active external override | `test_cfg_04_active_external_override` | Unit/property + SQLite/filesystem integration | pending |
| [CFG-05](specs/config-storage/spec.md) — credentials and missing keyring | `test_cfg_05_credentials_and_missing_keyring` | Unit/property + SQLite/filesystem integration | pending |
| [CFG-06](specs/config-storage/spec.md) — insecure secret file | `test_cfg_06_insecure_secret_file` | Unit/property + SQLite/filesystem integration | pending |
| [CFG-07](specs/config-storage/spec.md) — interrupted migration | `test_cfg_07_interrupted_migration` | Unit/property + SQLite/filesystem integration | pending |
| [CFG-08](specs/config-storage/spec.md) — unavailable or future database | `test_cfg_08_unavailable_or_future_database` | Unit/property + SQLite/filesystem integration | pending |
| [CFG-09](specs/config-storage/spec.md) — import without touching legacy data | `test_cfg_09_import_without_touching_legacy_data` | Unit/property + SQLite/filesystem integration | pending |
| [CH-01](specs/contacts-history/spec.md) — contact and favorite | `test_ch_01_contact_and_favorite` | Unit + SQLite/CSV integration | pending |
| [CH-02](specs/contacts-history/spec.md) — invalid and duplicate CSV | `test_ch_02_invalid_and_duplicate_csv` | Unit + SQLite/CSV integration | pending |
| [CH-03](specs/contacts-history/spec.md) — outcomes and recovery | `test_ch_03_outcomes_and_recovery` | Unit + SQLite/CSV integration | pending |
| [CH-04](specs/contacts-history/spec.md) — search and retention | `test_ch_04_search_and_retention` | Unit + SQLite/CSV integration | pending |
| [DIST-01](specs/reproducible-distribution/spec.md) — clean machine | `test_dist_01_clean_machine` | Build/isolation/artifact + release checklist | pending |
| [DIST-02](specs/reproducible-distribution/spec.md) — separate ELF and AppImage | `test_dist_02_separate_elf_and_appimage` | Build/isolation/artifact + release checklist | pending |
| [DIST-03](specs/reproducible-distribution/spec.md) — clean-clone build | `test_dist_03_clean_clone_build` | Build/isolation/artifact + release checklist | pending |
| [DIST-04](specs/reproducible-distribution/spec.md) — lab and isolation | `test_dist_04_lab_and_isolation` | Build/isolation/artifact + release checklist | pending |
| [DIST-05](specs/reproducible-distribution/spec.md) — manual desktop profile | `test_dist_05_manual_desktop_profile` | Build/isolation/artifact + release checklist | pending |
| [DIST-06](specs/reproducible-distribution/spec.md) — incomplete release blocked | `test_dist_06_incomplete_release_blocked` | Build/isolation/artifact + release checklist | pending |
| [DIST-07](specs/reproducible-distribution/spec.md) — pre-push audit | `test_dist_07_pre_push_audit` | Build/isolation/artifact + release checklist | pending |
| [LIN-01](specs/linux-integration/spec.md) — desktop without tray | `test_lin_01_desktop_without_tray` | Desktop/IPC integration + URI tests | pending |
| [LIN-02](specs/linux-integration/spec.md) — legitimate and malicious links | `test_lin_02_legitimate_and_malicious_links` | Desktop/IPC integration + URI tests | pending |
| [LIN-03](specs/linux-integration/spec.md) — second instance | `test_lin_03_second_instance` | Desktop/IPC integration + URI tests | pending |
| [LIN-04](specs/linux-integration/spec.md) — exit during a call | `test_lin_04_exit_during_a_call` | Desktop/IPC integration + URI tests | pending |
| [ND-01](specs/native-desktop/spec.md) — comparable layout | `test_nd_01_comparable_layout` | Snapshot/keyboard + VM visual review | pending |
| [ND-02](specs/native-desktop/spec.md) — feature outside the release | `test_nd_02_feature_outside_the_release` | Snapshot/keyboard + VM visual review | pending |
| [ND-03](specs/native-desktop/spec.md) — theme switching | `test_nd_03_theme_switching` | Snapshot/keyboard + VM visual review | pending |
| [ND-04](specs/native-desktop/spec.md) — system theme | `test_nd_04_system_theme` | Snapshot/keyboard + VM visual review | pending |
| [ND-05](specs/native-desktop/spec.md) — keyboard and HiDPI | `test_nd_05_keyboard_and_hidpi` | Snapshot/keyboard + VM visual review | pending |
| [ND-06](specs/native-desktop/spec.md) — registration failure | `test_nd_06_registration_failure` | Snapshot/keyboard + VM visual review | pending |
| [SIP-01](specs/sip-telephony/spec.md) — registration and renewal | `test_sip_01_registration_and_renewal` | Asterisk integration + dialog/state assertions | pending |
| [SIP-02](specs/sip-telephony/spec.md) — invalid configuration and authentication | `test_sip_02_invalid_configuration_and_authentication` | Asterisk integration + dialog/state assertions | pending |
| [SIP-03](specs/sip-telephony/spec.md) — calls in both directions | `test_sip_03_calls_in_both_directions` | Asterisk integration + dialog/state assertions | pending |
| [SIP-04](specs/sip-telephony/spec.md) — cancellation and answer race | `test_sip_04_cancellation_and_answer_race` | Asterisk integration + dialog/state assertions | pending |
| [SIP-05](specs/sip-telephony/spec.md) — rejection and second call | `test_sip_05_rejection_and_second_call` | Asterisk integration + dialog/state assertions | pending |
| [SIP-06](specs/sip-telephony/spec.md) — DTMF and hold | `test_sip_06_dtmf_and_hold` | Asterisk integration + dialog/state assertions | pending |
| [SIP-07](specs/sip-telephony/spec.md) — do not disturb | `test_sip_07_do_not_disturb` | Asterisk integration + dialog/state assertions | pending |
| [SIP-08](specs/sip-telephony/spec.md) — transfer and failure | `test_sip_08_transfer_and_failure` | Asterisk integration + dialog/state assertions | pending |
| [SIP-09](specs/sip-telephony/spec.md) — invalid certificate | `test_sip_09_invalid_certificate` | Asterisk integration + dialog/state assertions | pending |
| [SIP-10](specs/sip-telephony/spec.md) — network and suspend | `test_sip_10_network_and_suspend` | Asterisk integration + dialog/state assertions | pending |

Total: **46 scenarios, all pending**. Split composite cases into explicit success/failure tests without removing traceability.

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
