## 1. Contracts and single ownership

- [ ] 1.1 Approve this extension's planning alongside desktop before G0, start its implementation only after technical G0 approval, and pin the MCP SDK/MSRV/licenses; verify the ADR and ELF/Docker build compatibility.
- [ ] 1.2 Extract application/control with JSON v1 DTOs and schemas; verify core independence from UI/SQL/MCP and test errors/limits (AG-02).
- [ ] 1.3 Implement GUI/service modes and protected lock/IPC through platform; verify two clients, competing owners, malicious sockets/symlinks, and engine_unavailable without spawning (AG-01).
- [ ] 1.4 Implement service mode without DISPLAY, with explicit devices/secrets; verify startup/shutdown and no capture before an authorized operation (AG-01, AG-04).

## 2. CLI, authorization, and ledger

- [ ] 2.1 Implement CLI queries, pagination, and JSON/exit codes; verify contract snapshots, denied sensitive data, and secret-free config/doctor (AG-02, AG-04).
- [ ] 2.2 Implement owner-side human approvals and GUI grants with single-use headless bootstrap outside CLI/MCP; verify scopes, destinations, lifetime/quota, revocation, restart, rejection of self-approval through confirm/yes/TTY/pty, and the documented same-UID boundary (AG-04).
- [ ] 2.3 Implement the durable ledger and session versioning; verify replay, conflicting payloads, expiration/epoch, expired awaiting_approval with no effect, and crash outcome_unknown queryable across epochs with a new grant, ephemeral HMAC, and no automatic retry (AG-05).
- [ ] 2.4 Implement CLI controls and operation_id queries; verify real state, queues, deadlines, missing sessions, and denied operations with no effect (AG-02, AG-05).
- [ ] 2.5 Implement sanitized, retained auditing; verify no secrets, DTMF/PINs, reversible plain hashes, or full names/numbers in errors/logs without scope, and confirmed deletion (AG-04, AG-05).

## 3. MCP and real acceptance

- [ ] 3.1 Implement stdio MCP with a pinned official Rust SDK and strict tools; verify initialize/tools/list/call, schemas, and protocol-only stdout with a real SDK client (AG-03).
- [ ] 3.2 Implement limits/cancellation and reuse use cases/policies; verify malformed/excessive input, rate limits, disconnection, and no agent-issued authorization (AG-03, AG-04).
- [ ] 3.3 Run Asterisk tests against the exact ELF through CLI and MCP; verify dial/answer/reject/hangup/DTMF/mute/hold/transfer/DND and no duplication through retransmission (AG-05, AG-06).
- [ ] 3.4 Validate GUI and service without DISPLAY using headsets/a human peer in the G4 auxiliary-VM/host-only topology; verify two-way audio, consented capture, client disconnection, and owner shutdown, with a separate sanitized receipt per mode/hash, completing human AG-06 acceptance in G4 (AG-06).
- [ ] 3.5 Document commands, JSON/errors, threat model, grants, and generic MCP configuration; review with Claude CLI and verify a clone without base, fmt/clippy/test/doc/audit, and AG gates in G3b plus desktop G4/G5 before publication.
