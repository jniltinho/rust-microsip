# rust-microsip

A native Rust desktop softphone under development, with a planned compact interface inspired by [MicroSIP](https://www.microsip.org/) and complementary references from [GOnnect](https://github.com/gonicus/gonnect). Linux is the first release target.

**Status: implementation has started with the G0 scaffold; no usable softphone or release yet.** The experimental binary provides help, version, and static diagnostics only; it does not open a desktop window, register, or dial. This is an independent project, not an official MicroSIP or GONICUS product.

## Build the scaffold

Only Docker Engine with BuildKit/buildx is required on the host. Rust is installed exclusively inside the Ubuntu 24.04 builder, pinned by version and SHA-256.

```sh
docker buildx build --platform linux/amd64 --target artifacts --output type=local,dest=dist .
./dist/rust-microsip --help
./dist/rust-microsip --doctor --json
```

After fetching, the build runs fmt, clippy, tests, and rustdoc without network access. It exports a **scaffold ELF**, checksums, licenses, preliminary inventory, and metadata. **It does not produce an AppImage or independently satisfy full DIST-03/G0.** See the [development guide and limitations](docs/development.md).

## Proposal

[OpenSpec proposal](openspec/changes/create-native-rust-linux-release/proposal.md) · [Architecture](openspec/changes/create-native-rust-linux-release/design.md) · [Tasks](openspec/changes/create-native-rust-linux-release/tasks.md) · [Test plan](openspec/changes/create-native-rust-linux-release/test-plan.md) · [Sources and evidence](docs/reference-sources.md)

The proposed first release includes:

- A compact MicroSIP-inspired interface with Dialpad, Calls, and Contacts tabs and light, dark, and system themes.
- A local CLI and stdio MCP client for agents, with explicit authorization and a single owner.
- Real SIP registration and voice calls, audio devices, DTMF, hold, and blind transfer.
- TOML or explicit `.env` configuration, UI settings, and embedded SQLite without a database server.
- Local/CSV contacts, favorites, history, and basic Linux desktop integration.
- One executable with embedded application resources and explicitly audited Linux system dependencies. An additional AppImage does not silently replace this requirement.
- Docker builds without a host Rust toolchain and a disposable Docker Asterisk lab.

## OpenSpec workflow

Planning commands require OpenSpec CLI 1.10.0:

```sh
openspec status --change create-native-rust-linux-release
openspec validate create-native-rust-linux-release --strict --no-interactive
```

[Implementation approval](docs/implementation-approval.md) was given on 2026-10-12. In Codex, use `$openspec-apply-change`; in Claude, use the installed OpenSpec workflow or request change application. Check tasks only after obtaining evidence. G0 remains pending until the complete vertical proof passes.

The scaffold builder exists. The disposable PBX lab is being implemented using a [pinned andrius/asterisk image](docs/adr/0002-disposable-asterisk-lab.md). Application telephony, media, UI, persistence, and CLI/MCP controls remain planned contracts, not implemented features.

## Local references and skills

`base/` is local-only material, ignored by Git and unnecessary to read the proposal in a fresh clone. Reproducible references are documented by URL/revision without including upstream sources, AppImages, personal logs, or databases.

Project-installed Rust skills: Apollo `rust-best-practices`, `rust-async-patterns`, and `rust-testing`, shared by Codex and Claude. These support development; they are not application dependencies. See [provenance](docs/development-skills.md).

## Interfaces and roadmap

- Platform library: portable contracts and a Linux adapter; macOS/Windows in later releases.
- [Local agent CLI and MCP](openspec/changes/add-agent-cli-mcp/proposal.md): modes of the same binary without duplicated SIP/SQLite; explicitly authorized control.
- [Web/WASM](openspec/changes/add-web-wasm-softphone/proposal.md): full WebRTC calls in a later release, without client installation, using a compatible PBX or gateway. Not part of Linux v0.1.
- [Official MicroSIP help](https://www.microsip.org/help): neither a manual for the future rust-microsip nor a promise of parity.

Private repository: [jniltinho/rust-microsip](https://github.com/jniltinho/rust-microsip).
