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

[Implementation approval](docs/implementation-approval.md) was given on 2026-10-12. In Codex, use `$openspec-apply-change`; in Claude, use the installed OpenSpec workflow or request change application. Check tasks only after obtaining evidence. G0 remains pending until the complete vertical proof passes. A [pre-adoption license/asset/native inventory](docs/adr/0003-g0-license-and-native-boundaries.md) and tested dependency boundary now precede candidate incorporation.

The scaffold builder exists. The [disposable PBX lab](docker/asterisk/README.md) uses a [pinned andrius/asterisk image](docs/adr/0002-disposable-asterisk-lab.md). An experimental Rust adapter now passes real UDP/TCP/TLS registration, explicit refresh/unregistration, wrong-password and CA/hostname-negative tests ([receipt](docs/receipts/g0-rust-registration-2026-10-12.md)). This is an isolated signaling proof, not a usable softphone or completed task 1.6. An opt-in lab now also passes 12 five-second synthetic PCMA/PCMU calls over UDP/TCP/TLS in both directions ([media receipt](docs/receipts/g0-rust-media-2026-10-12.md)). TLS calls use explicitly plaintext-RTP fixtures, not SRTP. The opt-in RFC 4733 runner also verifies correct/incorrect three-digit sequences against actual Asterisk IVR tones ([DTMF receipt](docs/receipts/g0-rust-dtmf-2026-10-12.md)). An opt-in hold probe now checks sendonly/recvonly negotiation, local suppression, resumed two-way synthetic audio and preserved audio after a 488 re-INVITE rejection ([hold receipt](docs/receipts/g0-rust-hold-2026-10-12.md)). An opt-in REFER/NOTIFY probe now checks final success with measured destination media and preserved two-way audio after an invalid destination ([transfer receipt](docs/receipts/g0-rust-transfer-2026-10-12.md)). Mandatory SRTP/SDES remains unimplemented; an [isolated pre-adoption SRTP diagnostic](docs/receipts/g0-srtp-candidate-2026-10-13.md) preserves a failing duplicate-packet rejection test, so the candidate is not approved for direct secure-media use. These probes are not production call controls. Human voice, production audio, UI, persistence and CLI/MCP controls remain release requirements; G0 and task 1.6 are still open.

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
