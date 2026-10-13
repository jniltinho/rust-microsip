# rust-microsip

A native Rust desktop softphone under development, with a planned compact interface inspired by [MicroSIP](https://www.microsip.org/) and complementary references from [GOnnect](https://github.com/gonicus/gonnect). Linux is the first release target.

**Status: a first desktop build exists, but there is no release yet.** The binary opens a compact MicroSIP-like window, registers one SIP account over UDP, TCP or TLS, and places and receives calls with G.711 audio through ALSA, DTMF, hold, mute, blind transfer, DND and optional or mandatory SRTP/SDES. These functions passed end-to-end runs against a disposable Asterisk lab ([receipt](docs/receipts/production-engine-2026-10-13.md)). Clean desktop VMs, real headsets, persistence, desktop integration, AppImage and the other [release gates](openspec/changes/create-native-rust-linux-release/tasks.md) are still open. This is an independent project, not an official MicroSIP or GONICUS product.

See the [roadmap](docs/README.md) for what is done and what remains, and the [architecture overview](docs/architecture.md) for the crate layers.

## Build

Only Docker Engine with BuildKit/buildx is required on the host. Rust is installed exclusively inside the Ubuntu 24.04 builder, pinned by version and SHA-256.

```sh
docker buildx build --platform linux/amd64 --target artifacts --output type=local,dest=dist .
./dist/rust-microsip --help
```

After fetching, the build runs fmt, clippy, tests, rustdoc and the dependency/notice policy without network access, then exports the ELF, `SHA256SUMS`, licenses, a build manifest and an SBOM. The ELF needs the host's glibc and `libasound.so.2` (ALSA, present on standard desktops). See the [development guide](docs/DEVELOPMENT.md) and [ADR 0012](docs/adr/0012-production-dependency-graph.md).

## Run

Start `./dist/rust-microsip` and open **Menu → Settings** to enter the account; the password is kept in memory for the session only. Alternatively, create `$XDG_CONFIG_HOME/rust-microsip/config.toml` (mode 0600):

```toml
schema = 1

[account]
username = "1001"
domain = "sip.example.invalid"
server = "sip.example.invalid"
transport = "tls"        # udp | tcp | tls
media = "srtp"           # rtp | srtp (srtp requires tls)
password_env = "RUST_MICROSIP_SIP_PASSWORD"
```

and export the password in that environment variable before starting. Passwords are never accepted as command-line arguments. `rust-microsip --doctor` prints a sanitized report of the effective configuration and where each value came from; `rust-microsip config validate` and `config export` check and print it without secrets.

While the window runs, `rust-microsip status`, `rust-microsip call dial 1002` and `rust-microsip mcp --stdio` (for AI agents) control it locally; every change waits for **Allow** in the window unless a person issued a scoped grant in Settings → Agent access. `rust-microsip service` runs without a window from a single-use bootstrap exported there. See [docs/agent-cli-mcp.md](docs/agent-cli-mcp.md) (preview).

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

[Implementation approval](docs/implementation-approval.md) was given on 2026-10-12. In Codex, use `$openspec-apply-change`; in Claude, use the installed OpenSpec workflow or request change application. Check tasks only after obtaining evidence. G0 remains pending until the complete vertical proof passes. A [pre-adoption license/asset/native inventory](docs/adr/0003-g0-license-and-native-boundaries.md) and tested dependency boundary now precede candidate incorporation.

Earlier isolated G0 experiments and their receipts are listed in the [development guide](docs/DEVELOPMENT.md) and [`docs/receipts/`](docs/receipts/).

## Local references and skills

Additional Rust telephony references are [rvoip](https://github.com/eisenzopf/rvoip) for client/session/media design, [RustPBX](https://github.com/restsend/rustpbx) for server-side integration, and [rsipstack](https://github.com/restsend/rsipstack) for SIP transport/transactions/dialogs. Their [pinned research snapshots and adoption boundaries](docs/reference-sources.md#rust-telephony-reference-projects) distinguish reference material from dependencies; only rsipstack is currently incorporated, as an experimental, version-pinned dependency. These references do not replace the andrius/asterisk lab or establish feature parity.

`base/` is local-only material, ignored by Git and unnecessary to read the proposal in a fresh clone. Reproducible references are documented by URL/revision without including upstream sources, AppImages, personal logs, or databases.

Project-installed Rust skills: Apollo `rust-best-practices`, `rust-async-patterns`, and `rust-testing`, shared by Codex and Claude. These support development; they are not application dependencies. See [provenance](docs/development-skills.md).

## Interfaces and roadmap

- Platform library: portable contracts and a Linux adapter; macOS/Windows in later releases.
- [Local agent CLI and MCP](openspec/changes/add-agent-cli-mcp/proposal.md): modes of the same binary without duplicated SIP/SQLite; explicitly authorized control.
- [Web/WASM](openspec/changes/add-web-wasm-softphone/proposal.md): full WebRTC calls in a later release, without client installation, using a compatible PBX or gateway. Not part of Linux v0.1.
- [Official MicroSIP help](https://www.microsip.org/help): neither a manual for the future rust-microsip nor a promise of parity.

Private repository: [jniltinho/rust-microsip](https://github.com/jniltinho/rust-microsip).
