## Context

See [proposal.md](proposal.md) and [contract](specs/agent-cli-mcp/spec.md). Desktop is unimplemented. This change depends on G0 and complements planned IPC without removing external-URI confirmation or one-account/call limits.

## Goals / Non-Goals

**Goals:** shared application service, testable commands, least privilege, deliberate headless operation.
**Non-Goals:** mandatory daemon, remote API, OS commands, audio recording/transcription, agent credential access, LLM client.

## Decisions

### Composition and ownership
Add planned application (use cases), control (DTO/IPC/policies), cli, mcp crates; core stays independent. One ELF selects GUI/service/cli/mcp, no other executable dependency. Manually started service requires no graphical session but does require voice devices/permissions. Only owner holds SIP/audio/storage. Explicit agent-launched stdio subprocess is not a mandatory GUI SIP subprocess and does not violate single-binary packaging.

Profile lock/socket in private XDG_RUNTIME_DIR, verified owner/permissions, Linux SO_PEERCRED; no arbitrary URI-supplied endpoint. Prevent foreign-owner symlinks/sockets; IPC-v1 handshake before commands. Platform exposes protected connection/identity without Unix APIs. GUI/service share lock; loser never registers. No active-call GUI/headless handoff in v0.1. GUI started against service reports engine_owned_by_service and exits without registration/capture; operator deliberately stops service before GUI. External URI forwarded to service returns visible uri_confirmation_unavailable, never dials; agent grants do not approve external links. Service returns grant_required without grants, not impossible interactive approval. CLI/MCP never spawn an absent owner. Offline config validate/doctor neither fetch passwords nor register/open SQLite. Data import/export requires owner and explicit GUI human approval through its single worker; otherwise engine_unavailable. No direct/offline CLI SQLite writes; no service-side client config import in this release.

### CLI/MCP contracts
Candidate CLI parser clap; official Rust SDK rmcp subject to version/MSRV/license/test. JSON v1: schema_version, request_id, accepted operation_id, status, data or error code/message/retryable. Human output default, --json opt-in. Exits: 0 success/accepted, 2 validation, 3 authorization, 4 unavailable engine, 5 timeout, 1 internal error. Acceptance is not answered-call success; operation_id/call_id status proves completion. Default five-second acceptance/query deadline, no automatic mutation retries.

Planned tools: status_get, devices_list, contacts_search, history_list, config_show, operation_get, call_dial, call_answer, call_reject, call_hangup, call_dtmf, call_mute, call_hold, call_transfer, dnd_set. Strict no-extra-field JSON schemas, bounds, honest annotations. Contacts/history ≤100/page, opaque cursor. IPC/MCP messages ≤1 MiB, queue 64/connection, 10 requests/s explicit errors; owner global 50 requests/s and 16 connections. Default status masks destination/name/account/devices; operation_get requires appropriate result-read scope. Pin negotiated MCP version; official SDK-client tests, no homemade JSON-RPC support claim. Sanitized structured stderr, protocol/JSON-only stdout by mode.

### Authorization the agent cannot self-issue
Owner validates every request, including same-UID clients. Same UID does not protect against malware already running as the user: consent/least-privilege boundary, not strong sandboxing. Default only nonpersonal status/basic doctor. First sensitive request returns approval_required plus ID; owner GUI shows origin, operation/destination/effect for human confirmation. Only owner GUI approves operations/issues grants; no cli approve/grant or TTY/pty fallback because TTY does not prove human presence. Show scope/destination/origin/capture effect/lifetime/quota before decisions.

Headless bootstrap is previously human-exported from GUI to a 0600 file in a 0700 directory. GUI stores random capability hash, single-use bootstrap ID, expiry/policy in DB. Operator quits GUI, explicitly starts service --grant-file; owner atomically consumes ID once and binds grant to new epoch. Reuse returns bootstrap_consumed; never delete/rewrite the file without instruction. No automatic restart reimport; new service session needs new GUI bootstrap. Capability never in argv/logs; client obtains it through FD/explicit private file, owner stores hash only. Separate read scopes contacts/history/devices/config/operations and control scopes dial/answer/reject/hangup/dtmf/mute/hold/transfer/dnd. Exact destination allowlists for dial/transfer; explicitly permitted incoming origins/sessions for answer/reject, never infer incoming consent from outbound allowlists. Max validity one hour, call quota/rate limits. Revoke administratively in local GUI or by stopping service, never MCP tools. Per-operation approval expires after 60 seconds, single-use, bound to profile/epoch/idempotency_key/payload HMAC, not request_id.

Approval flow: client sends keyed intent; owner persists operation_id/awaiting_approval and returns approval_required within five seconds. GUI decides within 60 seconds; approval executes the pending intent exactly once/publishes result. Client queries operation_get with permission rather than resending under a new key. Denial/expiry is terminal without effect. Request deadline is not human deadline. Previous-epoch awaiting_approval becomes expired without effects or re-presentation after restart. Service requires prior grants and never offers awaiting_approval.

Security boundary prevents protocol self-authorization. Unrestricted same-UID shell/GUI automation is equivalent to the user and outside the boundary: it can read files/start service/modify binary or DB/automate GUI. Use distinct sandbox/user and bounded grants when that isolation is needed; GUI alone does not verify human presence. No TTY/pty/confirm/yes approval. Expiry returns grant_expired; renewal in v0.1 requires deliberate service stop, GUI/new bootstrap, new service session. Grant expiry never automatically ends/redials calls; operator can still safely stop service through OS. Bootstrap issue/import requires operational consent external to the agent, not protection against a compromised local user.

Answering starts capture and requires consent. GUI or sanitized service indicator/log reports microphone activity. Without read permission, errors reveal no names/numbers. IPC/grant/SIP credentials never return through config_show.

### Idempotency and outcomes
Durable ledger key is global profile/epoch/idempotency_key, independent of CLI/MCP client. Store operation, authorized policy/identity, outcome, and payload HMAC-SHA-256 using random per-epoch memory-only key. Never persist plain hashes of low-entropy DTMF/PIN/password/destination. Same key with different payload/scope returns conflict, even across grants; never another effect. No plaintext DTMF persistence/logging; pending digits exist only in memory. Crash marks outcome_unknown/cancelled before any resend, never restores digits. HMAC comparison only in live epoch; never persist its key in DB/keyring/file.

Persist sanitized acceptance/outcome before effects. Unavailable DB/ledger blocks all mutations before SIP/audio/DTMF per CFG-08. Crash between send/confirmation yields outcome_unknown, never resend; human/PBX reconciliation. Ledger retention 24 hours; each owner gets new epoch, invalidating prior active grants without reviving consumed bootstraps. New effects need current epoch/new key; old-epoch mutations return stale_epoch, no effect. Old operation_get allowed for 24 hours with a current read grant, returning persisted outcome/outcome_unknown without old HMAC key. Afterwards result_expired, never reinterpret as new intent. Reads cannot reuse revoked grants or cross scopes.

Session operations require call_id/expected_version, preventing actions on a replacement call. Request cancellation is not hangup; post-acceptance queries never reexecute. Minimal SQLite audit, masked destination, 90-day retention/confirmed deletion.

No-DISPLAY service still needs functional user session/environment: private valid XDG_RUNTIME_DIR, G0-selected audio backend, and D-Bus/Secret Service if used. Without keyring, explicit private secret file/environment. Never automatically enable linger/logind/services. Guide explains operator preparation and runtime_unavailable/audio_unavailable/secrets_unavailable, never claims audio-less server voice support.

### Validation
AG-01..06 are mandatory additional Linux-agent gates. G3b verifies contracts/automation and technical AG-06. Human AG-06 completes in G4 auxiliary-VM/host-only topology with separate exact-artifact hash/GUI-service receipts; may share a G4 report, never inherit another binary/mode's results. Evidence: schema/policy/ledger units, multi-client IPC integration, real MCP SDK client, clean stdout, negative cases. Asterisk proves exactly-once intentions; synthetic tones/headsets verify GUI and no-DISPLAY service audio. No real accounts. Document absolute ELF path/args ["mcp","--stdio"] without automatically installing/modifying Grok/Claude/Codex.

## Risks / Trade-offs

- [Automation can call paid destinations] → deny by default, allowlists, quota, short grants, no-PSTN test PBX.
- [Timeout/crash duplication] → durable ledger, no outcome_unknown retry, reconciliation.
- [MCP increases surface] → minimal features, bounded input, audit, stdio only.
- [Headless lacks audio/keyring] → real no-DISPLAY tests, prior consent/grant, clear errors, never fake voice.
- [Same-UID processes can read grants] → permissions/bounded capabilities; no compromised-user isolation promise.

## Migration Plan

Approve with desktop; complete G0; application/control/IPC, CLI, policies, MCP, Asterisk/real-media integration, Claude review. Publish only after both checklists. Rollback disables modes/grants, preserves compatible DB, never automatically migrates legacy secrets/settings.
