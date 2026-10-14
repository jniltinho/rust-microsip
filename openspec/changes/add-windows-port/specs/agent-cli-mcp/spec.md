## ADDED Requirements

### Requirement: Windows control channel
On Windows the agent control channel SHALL be a named pipe whose name includes the user SID, created only if no instance of that name exists, rejecting remote clients, with an access-control list granting access only to the current user. The owner SHALL close without a response connections from processes of another user or with a restricted token, and clients SHALL refuse a pipe whose server process runs as another user. Frame limits, rate limits, human approval, grants, the ledger and "exists only while enabled" SHALL match Linux; the channel SHALL never listen on a network interface.

#### Scenario: WIN-10 other user
- **WHEN** a process of another Windows user connects to the control pipe
- **THEN** it receives no response and no operation is recorded

#### Scenario: WIN-10 approval still required
- **WHEN** an MCP client on Windows requests a dial without a matching grant
- **THEN** the owner returns approval_required and nothing is dialed until a person approves in the window

### Requirement: Windows console executable
On Windows, CLI subcommands, `mcp --stdio`, `service` and `--doctor` SHALL be provided by `rust-microsip-cli.exe` with the same arguments, JSON, exit codes and protocol-only stdout as on Linux; `rust-microsip.exe` SHALL only open the window and SHALL NOT open a console. Secrets SHALL never be accepted as arguments, and grant capabilities SHALL be read only from an owner-only file.

#### Scenario: WIN-10 MCP client on Windows
- **WHEN** an MCP client starts `rust-microsip-cli.exe mcp --stdio` while the window runs
- **THEN** the official SDK client initializes, lists tools and reaches the running owner, and stdout carries only JSON-RPC
