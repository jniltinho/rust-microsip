# Proposal validation

Date: 2026-10-12. Historical scope: planning documents before application implementation. Counts and absent-file statements below describe that planning snapshot, not current implementation progress. See tasks/implementation receipts for current status. English translation; no new runtime approval is implied.

## Local checks at planning time

- OpenSpec CLI 1.10.0: strict noninteractive validation passed.
- Proposal/specs/design/tasks complete: 4/4 planning artifacts.
- Desktop traceability: seven specs, 46 unique scenarios/test-plan entries, 54 unexecuted tasks. CLI/MCP and web add one spec/six scenarios each and 14/16 tasks: 58 scenarios, 84 pending tasks at planning time.
- Root `/base/` ignore, no tracked files/submodules from references.
- Initially jniltinho/gonnect-rust, renamed jniltinho/rust-microsip, private throughout. Only approved documents/skills sent; no application tag/release.
- Clean index-export snapshot validated without base; relative links, unique IDs, skill symlinks, no submodules verified.
- Staged whitespace checks passed after normalization, including imported skills.
- No personal AppImage execution/modification, personal DB/credential reads, or started PBX during planning.

## Independent Claude CLI review

Installed CLI 2.1.296, print mode, no tools, empty/strict MCP, no hooks/project/user settings/session persistence. Input only sanitized documents, never base/personal configuration. Raw output stays outside Git; sanitized reports are in docs/reviews.

Round 1: **REVISE**, five documentary blockers, [report](../../../docs/reviews/claude-proposal-round-1.md). Assessed fixes:
1. Host override replaces internal network with non-internal loopback-bound bridge.
2. G4 auxiliary Docker PBX/isolated host-only network; clean clients and specified second human endpoint.
3. User-space UDP impairment; manual isolated-VM NAT/STUN, no default-container privileges.
4. G0 ELF includes actual UI/audio/D-Bus/IPC, compatible renderer, keyboard/hotplug before dependency allowlist finalization.
5. Replace “no installed Qt” with “no extra desktop packages” and “Qt-free ELF dependencies,” permitting standard KDE.

Also clarified WAL/SHM/backup permissions, --config replacement, private literal passwords, CSV duplicates, DND response.

Round 2 (earlier desktop scope, not current extensions): **APPROVE**, [report](../../../docs/reviews/claude-proposal-round-2.md). Input SHA-256 `185fc546a12be27d1f23cbec7b9240751ff5118a0973aa6586a0196a9a9dd540`; exit 0/success/no error. Nonblocking G4 command, override merging, Docker forwarding firewall, teardown observations were explicitly added to design. Review does not prove interoperability/lab execution.

## Planning-stage limits

No Cargo, Docker builds, Asterisk tests, voice calls, new-app UI comparison, or ELF validation were run during this snapshot. Own Cargo.toml/Dockerfile/Compose/compiled app did not yet exist. All runtime gates remained pending. Approved implementation starts with G0/Asterisk; later scaffold/PBX receipts do not retroactively change this historical review.

## rust-microsip scope review

User confirmed project name, compact MicroSIP UI with complementary GOnnect references, platform contracts preparing macOS/Windows, agent CLI/MCP, and full-call web in a later release. Local directory/private repository renamed; base remained local.

MicroSIP source/installer/Lite downloads stayed in base. Lite opened in disposable network/account/personal-device-free Wine/Xvfb; visual capture only, no calls/audio proof. See [source hashes/boundaries](../../../docs/reference-sources.md). Official help is a behavior reference, not parity promise.

Three complete planning changes, each strict-validated:
- create-native-rust-linux-release: compact Linux desktop/platform/persistence.
- add-agent-cli-mcp: CLI, optional service owner, authorized stdio MCP without SIP/SQLite duplication.
- add-web-wasm-softphone: later Rust/WASM/browser integration, WSS/WebRTC/PBX-gateway.

Extension gates live in their specs/tasks; older reviews do not automatically approve expanded scope.

Round 3: **REVISE**, [report](../../../docs/reviews/claude-proposal-round-3.md), input SHA-256 `867b53b47f5e7bc07d95df88c10a15987f4c71cb60411c03a5d543ec673e4648`. Six fixes: approval outside protocol/honest same-UID threat model; global pending-intent idempotency; cross-epoch read-only outcomes; no false service/URI/GUI approval channel; ephemeral DTMF HMAC; planning approval before G0 and AG gates before release.

Round 4: **APPROVE**, [report](../../../docs/reviews/claude-proposal-round-4.md), input SHA-256 `27d5aa3522b9055339478c30f0bb207cd22cfd0e56ae34188b6a307513aa933a`. Both processes exit 0/subtype success/is_error false, no tools/MCP/hooks/settings/persistence. Afterwards clarified: previous-epoch awaiting_approval expires; bootstrap_consumed; grant renewal requires new session; human AG-06 closes in G4 with mode/hash receipts; WSS proxy belongs to operator infrastructure.

Final planning audit: nine specs, 58 unique IDs, 84 pending tasks, intact relative links/skill symlinks, no staged base/submodules. Each change validated from clean document snapshot without base; staged whitespace passed. Review/Wine smoke do not prove application telephony; runtime acceptance stayed pending.
