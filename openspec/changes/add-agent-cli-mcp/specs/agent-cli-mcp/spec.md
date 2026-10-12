## Purpose

Enable verifiable local terminal/agent CLI/MCP operation with one telephony/storage owner and explicit external-effect authorization.

## ADDED Requirements

### Requirement: Single owner and local modes
The executable SHALL offer GUI, explicit headless service, CLI client, and stdio MCP server. CLI/MCP SHALL connect to the profile owner through OS-user-authenticated local IPC, never independently opening SQLite/registering SIP/capturing audio. Without an owner they SHALL return engine_unavailable, never silently start one.

#### Scenario: AG-01 multiple clients
- **WHEN** GUI/service owns the profile and two CLI/MCP clients query it, or no owner exists
- **THEN** at most one SIP registration/SQLite worker exists; coherent snapshots or engine_unavailable are returned without hidden spawning; GUI against service returns engine_owned_by_service, external URI uri_confirmation_unavailable without dialing, and service without grant grant_required

### Requirement: Structured CLI and real commands
CLI SHALL provide status, devices list, contacts search, history list, sanitized config show, call dial/answer/reject/hangup/dtmf/mute/hold/transfer, and dnd. It SHALL support human/JSON-v1 output, bounded filtering/pagination, session IDs, documented exit codes. Desktop config validate/import/export/doctor SHALL remain compatible/secret-free; MCP configuration mutation is outside this release.

#### Scenario: AG-02 CLI contract
- **WHEN** an operator queries state and runs authorized controls against a valid/missing session
- **THEN** coherent human/JSON-v1 output, traceable asynchronous outcomes, and distinct success/validation/authorization/unavailable/timeout exits are returned

### Requirement: Interoperable stdio MCP
Server SHALL negotiate compatible MCP, advertise strictly schematized tools, and provide CLI-equivalent capabilities with separate reads/controls. stdout MUST contain only MCP messages; sanitized logs SHALL use stderr. HTTP/SSE, arbitrary shell, and arbitrary file opening MUST NOT be exposed.

#### Scenario: AG-03 protocol and limits
- **WHEN** a client initializes/lists tools/makes a valid read and sends unknown arguments, excessive messages, or invalid requests
- **THEN** correct protocol responses/bounded errors occur without stdout logs, panic, file access, or shell execution

### Requirement: Local authorization and data protection
Owner SHALL deny mutations by default. Default reads SHALL expose only summarized status/nonpersonal diagnostics; contacts/history/identifiable devices/config need local read grants. Each mutation SHALL require owner-side human approval or a GUI-issued pre-headless-session grant outside agent CLI/MCP, bounded by scope, permitted destinations/sessions, expiry, limits, revocation. Tool annotations/agent confirm/yes MUST NOT authorize. UI-less service SHALL require prior single-use human bootstrap. TTY/pty MUST NOT create approval; unrestricted same-UID shell/GUI automation SHALL be documented outside the boundary, never falsely promised prevention. SIP credentials, audio, and secret references MUST NOT be returned.

#### Scenario: AG-04 deny and grant
- **WHEN** an agent requests dial/history without permission, attempts confirm/yes or cli approve in pty, then uses valid/expired/revoked grants
- **THEN** self-approval is rejected (no cli approve exists); pending-intent GUI approval plus operation_get or active prior grants allow only authorized operations; denial causes no call/capture/data disclosure/change

### Requirement: Idempotency, concurrency, audit
Unavailable storage/ledger MUST block effects before mutation. Mutations SHALL require idempotency keys and target session/version where applicable; retransmission MUST NOT duplicate calls/DTMF/transfers. Namespace SHALL be global profile/epoch/key, not grant/client. Different payload/scope reuse SHALL fail. Old-epoch mutation SHALL return stale_epoch without effects; operation_get SHALL return persisted outcome/outcome_unknown for 24 hours with current post-restart read authorization, then result_expired. Operations SHALL be bounded with timeouts/result IDs; client timeout MUST NOT automatically retry. Local audit SHALL retain sanitized decisions/outcomes without digits/secrets/full numbers by default.

#### Scenario: AG-05 replay and race
- **WHEN** CLI/MCP repeat dial/dtmf/transfer with one key, reuse it for different payloads, race an ended session, or query after crash/restart
- **THEN** at most one effect per accepted intent occurs, explicit conflict/stable terminal result, no second dialog; current read grants access old outcomes without reexecution, and DTMF/PIN cannot be recovered through plain DB hashes

### Requirement: Proven telephony and independent client shutdown
Acceptance SHALL test CLI/MCP controls on exact Linux artifacts against Asterisk and real-device bidirectional voice in GUI/service ownership. Closing CLI/MCP SHALL release its connection without terminating owner/accepted calls; owner exit SHALL follow safe desktop shutdown.

#### Scenario: AG-06 real operation and disconnection
- **WHEN** an authorized client establishes/controls a lab call and disconnects
- **THEN** PBX confirms effects, call stays with owner until explicit termination, and receipt distinguishes protocol/synthetic media/real human audio
