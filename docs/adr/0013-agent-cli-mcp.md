# ADR 0013: Local control socket, CLI and stdio MCP server

Date: 2026-10-13. Status: **accepted for the `add-agent-cli-mcp` change; the human
G4 acceptance (AG-06 with headsets) is still open.**

## Context

The [CLI/MCP extension](../../openspec/changes/add-agent-cli-mcp/design.md) lets a
terminal or an AI agent drive the softphone locally without a second telephony owner
and without any way for the agent to authorize itself.

## Decision

### One owner, one contract

- The single-instance owner (the window today; the headless service later) binds
  `control.sock` (0600) in the private `$XDG_RUNTIME_DIR/rust-microsip` directory
  next to the instance lock. Only a process holding the lock can start the server.
- Each connection is checked with the kernel's peer credentials (`SO_PEERCRED`)
  through `rustix` 1.1.5 (already in the reviewed graph; `std`'s `peer_cred` is
  still unstable and the workspace forbids `unsafe`). Clients check the owner the
  same way and refuse a foreign directory, socket or peer.
- `rust-microsip-control` (new crate, pure) defines the JSON v1 contract: hello and
  welcome, requests with an idempotency key for every mutation, strict schemas
  (`deny_unknown_fields`), bounded fields, 1 MiB lines, stable error codes and the
  CLI exit codes 0/2/3/4/5/1.
- The owner handles requests in the window's reducer, so the CLI, MCP and the
  window share one call state.

### Authorization

Personal reads (contacts, history, devices, configuration) need read grants and
otherwise answer `grant_required`. Grants are issued only in the window (Settings →
Agent access): scopes, exact destinations and incoming callers, a call quota and at
most one hour of validity, bound to the owner epoch, revocable. The client presents
the secret capability from a private file; the database keeps its SHA-256 only
(schema v5). A mutation the grant covers runs without a prompt; anything else falls
back to the prompt in the window. The headless service (`rust-microsip service
--grant-file FILE`) consumes a single-use bootstrap exported from the window into its
own epoch, never prompts and answers `grant_required` for everything else; a window
or link launched meanwhile is answered `service` over the instance socket and exits
(`engine_owned_by_service`, `uri_confirmation_unavailable`). `status_get` hides names and numbers.
Every mutation is first written to the durable ledger and then waits up to 60 s for
a person to press **Allow** or **Deny** in a modal prompt that blocks the rest of
the window, shows one request at a time (who asked, by process id, what would
happen and the countdown) and arms its buttons only one second after that request
appears, so a click aimed at the call controls cannot approve it. A mutation the
client already timed out on is dropped unrecorded, so a retry with a new key cannot
produce a second identical prompt. There is no `approve` command,
no `--yes`/`--confirm`, and no MCP tool that changes authorization; tool
annotations are hints only. 

### Ledger

SQLite schema v4 adds `control_epochs`, `control_operations` and `control_audit`.
A request is identified by owner epoch and idempotency key; its payload is stored
only as an HMAC-SHA-256 under a key generated per owner start and kept in memory
(`ring`, already in the graph). A retransmission returns the stored state; the same
key with another payload is `conflict`; a key from an earlier epoch is
`stale_epoch` and never re-run. At each start, waiting operations become
`expired` and operations that may have been sent become `outcome_unknown`.
Operations are kept 24 hours, audit rows 90 days; neither holds digits or full
numbers. DTMF digits stay in memory and are paced 200 ms apart, because the media
task replaces an event in progress.

### MCP

`rust-microsip mcp --stdio` uses the official Rust SDK **rmcp 3.5.1**
(`server`, `transport-io`; no `macros`): 15 tools, one per operation, with
hand-written strict JSON schemas (`additionalProperties: false`, the key required
for mutations). It connects to the owner per call and never starts one, so stdout
carries only protocol messages. Input lines are bounded to 1 MiB without buffering
the excess, and calls to 10 per second. Tests use the SDK's client.

### Dependencies

New registry packages: `rmcp` 3.5.1 (Apache-2.0; the upstream LICENSE, pinned by
commit `79437f291b2c44053d00dcd5db969fd0cca7c887`, also keeps MIT terms for legacy
contributions — both compatible with GPL-3.0-or-later, text retained), `schemars`
1.2.3 and `schemars_derive` (MIT), `chrono` 0.4.45, `dyn-clone`, `futures-executor`,
`iana-time-zone` (+`-haiku`, not built on Linux), `pastey`, `ref-cast`
(+`-impl`), `serde_derive_internals` (MIT OR Apache-2.0); `tokio-stream` 0.1.19
(MIT) only for the SDK client in tests. MSRV 1.88 ≤ the pinned 1.99 toolchain.
`rmcp` is recorded in `packaging/reviewed-licenses.json` with its upstream LICENSE in
`packaging/third-party-notices/rmcp-3.5.1/`; the others use accepted expressions and
ship their own notices. `cargo deny` checks advisories in CI.

## Consequences

- The same ELF provides window, CLI and MCP; no extra executable or daemon.
- Same-UID malware can still read files, start processes or automate the window —
  including pressing **Allow** through the accessibility bus, as the lab suite
  itself does. The guarantee holds for agents limited to the MCP tools (no shell,
  no accessibility access); it is consent and least privilege, not isolation from
  the user. `sip:`/`tel:` links forwarded through the instance socket only fill
  the number and never dial.
- Idempotency keys are stored in the local ledger for 24 hours; clients must not
  put personal data into them (the CLI generates random keys).
- Grants, the headless service and real-audio acceptance (G4) remain to be built
  and verified before the extension's gates pass.
