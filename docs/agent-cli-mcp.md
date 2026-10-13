# Controlling rust-microsip from a terminal or an AI agent

The same `rust-microsip` executable is also a small CLI and a stdio MCP server.
Both talk to the **running** window over a private local socket; they never start
the softphone, open its database or register SIP themselves. Status: preview of the
[agent CLI/MCP change](../openspec/changes/add-agent-cli-mcp/tasks.md) — grants and
the headless service are not available yet.

## Rules that always apply

- **Every change needs a person.** Dialing, answering, rejecting, hanging up, DTMF,
  mute, hold, transfer and DND wait up to 60 seconds for **Allow** or **Deny** in a
  prompt over the window, which shows one request at a time, which local process
  asked and what would happen; its buttons work only after one second. There is no
  `approve` command, no `--yes`, and no MCP tool that can approve anything.
- **Personal data stays private.** `status` shows registration, DND and the current
  call's id, version, direction and phase, never names or numbers. Contacts,
  history, devices and configuration need a read grant (not offered yet) and answer
  `grant_required`.
- **Retries are safe only with the same key.** Each change carries an idempotency
  key. Repeating a request with the same key reports the earlier result and never
  acts twice; the same key with a different request is a `conflict`. After the
  window restarts, old keys answer `stale_epoch` and are never re-run.
- **Same-user programs.** The socket accepts only processes of your own user
  (checked by the kernel). Malware already running as your user can do anything
  you can, including clicking in the window or pressing **Allow** through the
  accessibility bus; this is a consent boundary for agents limited to the MCP
  tools, not a sandbox. Never put personal data into idempotency keys: they are
  kept in the local ledger for 24 hours.

## CLI

```sh
rust-microsip status                       # add --json for machine output
rust-microsip call dial 1002               # prints the operation id and key
rust-microsip operation get OPERATION_ID   # pending, succeeded, denied, expired, ...
rust-microsip call hangup CALL_ID VERSION  # CALL_ID and VERSION from `status`
rust-microsip call dtmf CALL_ID VERSION -  # digits from standard input
echo 1002 | rust-microsip call dial -      # number kept out of the process list
rust-microsip dnd on --key my-key-1        # explicit key for a safe retry
```

Exit codes: 0 done or accepted, 2 invalid input or conflict, 3 waiting for approval
or not authorized, 4 not running or unavailable, 5 timeout, 1 internal error.
`rust-microsip --help` lists every command.

## MCP (agents)

Configure your MCP client to start a local stdio server:

```json
{ "command": "/absolute/path/to/rust-microsip", "args": ["mcp", "--stdio"] }
```

Use the absolute path of your ELF or AppImage; nothing is installed or changed in
any agent's configuration by rust-microsip. Tools: `status_get`, `operation_get`,
`devices_list`, `contacts_search`, `history_list`, `config_show`, `call_dial`,
`call_answer`, `call_reject`, `call_hangup`, `call_dtmf`, `call_mute`, `call_hold`,
`call_transfer`, `dnd_set`. Changes return `approval_required` with an
`operation_id`; poll `operation_get` until the person decides. stdout carries only
MCP messages; input lines are limited to 1 MiB and calls to 10 per second.

## What is recorded

The local database keeps each change for 24 hours (operation, state, sanitized
outcome and a keyed hash of the request — never digits or full numbers) and a
sanitized audit trail for 90 days. See [ADR 0013](adr/0013-agent-cli-mcp.md).
