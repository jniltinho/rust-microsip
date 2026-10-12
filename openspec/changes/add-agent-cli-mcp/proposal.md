## Why

Operators and agents need to query/control rust-microsip without pixel automation. Local CLI/MCP clients must share desktop services/limits, avoiding duplicate SIP registration, direct database writes, and unexpected calls.

## What Changes

- Add same-executable modes: `rust-microsip cli ...`, `rust-microsip mcp --stdio`, and explicitly started optional headless `rust-microsip service`.
- One owner per user/profile handles SIP, audio, SQLite, authorization; CLI/MCP connect through protected IPC.
- Query status/devices/contacts/history/sanitized configuration; control dial/answer/reject/hangup/DTMF/mute/hold/blind transfer/DND.
- Read-only MCP defaults; controls require per-operation human approval or explicit bounded/revocable local grants.
- Versioned JSON, typed errors, mutation idempotency, sanitized auditing.
- Real CLI/MCP Asterisk tests and agent-connection documentation, no automatic client installation/configuration.

## Capabilities

### New Capabilities

- `agent-cli-mcp`: CLI/stdio MCP, single owner, authorization, privacy, testable contracts.

### Modified Capabilities

No existing canonical spec. Complements the unimplemented desktop change.

## Impact

Depends on [create-native-rust-linux-release](../create-native-rust-linux-release/proposal.md), especially G0/domain/IPC/platform/configuration. Plan application/control/cli/mcp crates and same-binary modes, without mandatory daemon, public HTTP, Node, or embedded LLM calls. This requested extension joins the Linux candidate after G0; combined publication requires both changes' gates. No recording/transcription/automatic PSTN/multiple calls/secret queries. Planning artifacts only, no implemented CLI/MCP.
