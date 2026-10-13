# Agent control receipt: control socket, CLI and MCP (2026-10-13)

**Result: the window serves a local, kernel-authenticated control socket; the CLI and a stdio MCP server built on the official Rust SDK drive it; every change waits for a person in the window and runs at most once. The full lab suite passed 8/8 on commit `f5b4a5e`. Grants, the headless service and human audio acceptance are not done; this is not the extension's acceptance.**

Design: [ADR 0013](../adr/0013-agent-cli-mcp.md). Usage: [Controlling rust-microsip](../agent-cli-mcp.md).

## What exists

- `rust-microsip-control`: JSON v1 contract with strict schemas, bounded fields and 1 MiB lines, idempotency keys required for mutations, stable error codes and CLI exit codes.
- `control.sock` (0600) in the private runtime directory, started only by the single-instance owner; `SO_PEERCRED` checked on both sides; 16 connections, 10 requests per second per connection, 50 in total; mutations the client already gave up on are dropped unrecorded.
- Owner handling in the window's reducer:
  - status without names or numbers;
  - contacts, history, devices and configuration answer `grant_required`;
  - each mutation is recorded in the SQLite ledger (schema v4: epochs, operations as HMAC under a per-start key, audit), then shown in a modal prompt;
  - **Allow** arms after one second in the focused window; **Deny** and **Deny all and pause** work at once;
  - the check that the call is still the one the client saw is repeated at approval;
  - each operation executes once; a replay reports the earlier result, the same key with another payload is a `conflict`, an earlier session's key is `stale_epoch`, and a restart leaves `expired` or `outcome_unknown`;
  - DTMF is paced 200 ms apart and abandoned if stalled.
- CLI: `status`, `operation get`, `call dial|answer|reject|hangup|dtmf|mute|unmute|hold|resume|transfer`, `dnd on|off`, the grant-gated reads, `--json`, `--key`, and `-` for digits or targets from standard input. There is no approve, `--yes` or `--confirm`.
- `rust-microsip mcp --stdio` with `rmcp` 3.5.1: 15 strict tools with honest annotations, a connection to the owner per call, never an owner start, protocol-only stdout, 1 MiB input lines, 10 calls per second.

## Tests

- Unit and integration (fmt, clippy `-D warnings`, all workspace tests in the pinned container):
  - contract: 5 tests;
  - control socket: 5 tests (two clients, absent owner, invalid input never forwarded, owner timeout, rate limit, foreign socket file, bad handshake, stale margin);
  - ledger: 2 tests;
  - owner flow: 8 tests (masked status, grant-gated reads, approve once and replays, conflict, denial, expiry, call/version mismatch, service mode, non-durable store, stale epoch, paced and abandoned DTMF, deny-all pause, orphaned rows);
  - CLI: 3 unit and 1 process test (no owner gives exit 4 and creates nothing; approve and grant commands, `--yes` and `--confirm` are rejected);
  - MCP: 4 unit tests and 2 process tests. The SDK's own client lists the 15 tools and gets structured errors. A raw session with a garbage line keeps answering, and every stdout line is JSON-RPC.
- Layering rule extended to the new crate and dependencies.

## Lab: `gui-control` against Asterisk (run `20261013T193824Z-f5b4a5e`)

Release ELF `e87a061784f17d65c49fc29878cd0156af4ae655f8b7bd1b740f4a9da3eee76a`, image `sha256:43a6bda4641f1d7b03a9a0e09b73effe616324e3f2c59c0e45ec058c934ab6b8`, `summary.json` SHA-256 `6827107432b32fa2ca817f296c41d0819f523a8022a0f973856b7f7a7bd83c20`; all 8 scenarios passed on a clean tree with no lab objects left. The approvals were given through AT-SPI, as a person would click.

| Step | PBX / result |
| --- | --- |
| Two CLI clients read status at once | both `registered`, no names or numbers |
| `call dial 600` | exit 3 `approval_required`; **0 channels** while waiting; prompt focus on Deny |
| Allow | 1 channel; `operation get` → `succeeded` |
| Same key again / same key to 602 | exit 0 with the earlier result, still 1 channel / `conflict` |
| Hang-up with an old version | `not_found`, nothing recorded |
| Hang-up with the current version | 1 channel until Allow, then 0 |
| `dnd on`, Deny | `denied`, DND still off |
| `call dial 600`, Deny | **0 channels** |
| `history list` | exit 3 `grant_required` |
| MCP (official SDK client): tools, `call_dial` | 15 tools; `approval_required`; **0 channels** while waiting |
| Allow, replay with the same key, disconnect | `succeeded`; replay `ok` with the same operation id; after the client left the call stayed with the owner (1 channel) |
| CLI hang-up after approval | 0 channels |
| Window closed, `status` | exit 4 `engine_unavailable`, no process started |

## Licences

`rmcp` 3.5.1 was reviewed (Apache-2.0, upstream LICENSE pinned by commit, also keeping MIT terms for legacy contributions). The other new packages use accepted expressions with their own notices. The image build ran the notice collection with these packages.

## Independent review

Three Claude CLI rounds ([record](../reviews/claude-agent-control-2026-10-13.md)); round 3 approved after fixes for approval clickjacking, prompt flooding, late retries, stalled DTMF and lab gates that could pass falsely.

## Not established

- Read and control grants, the single-use headless bootstrap and the headless service (tasks 1.3, 1.4, 2.2).
- Answer, reject, DTMF, mute, hold and transfer driven through the CLI/MCP in the lab: only covered by unit tests so far (task 3.3).
- Audit deletion from the window (task 2.5).
- Human audio with headsets in the G4 topology (task 3.4).
- Task 1.1 waits for the desktop G0 decision.
