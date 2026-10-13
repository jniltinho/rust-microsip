# Developer guide

How to build, test and contribute to rust-microsip. Project rules live in [`AGENTS.md`](../AGENTS.md);
the [documentation index](README.md) has the roadmap and the [architecture](architecture.md) the design.

- [Prerequisites](#prerequisites)
- [Repository layout](#repository-layout)
- [Docker workflow](#docker-workflow)
- [Common tasks](#common-tasks)
- [Lab suite](#lab-suite)
- [Running the app locally](#running-the-app-locally)
- [Release process](#release-process)
- [OpenSpec workflow](#openspec-workflow)
- [Coding rules](#coding-rules)
- [Pinned inputs and isolation](#pinned-inputs-and-isolation)
- [Troubleshooting](#troubleshooting)

## Prerequisites

- Docker (with BuildKit and Compose) and git. `make` is optional.
- Nothing else. Do **not** install a Rust toolchain or system libraries on the host: builds, lints
  and tests run in the pinned toolchain image (`Dockerfile`, target `toolchain`).
- `base/` holds local-only references: never add it to Git, a build context or an artifact.

## Repository layout

```
apps/rust-microsip/   single binary: window, control CLI, `mcp --stdio`, `service`
crates/core           domain types, call state machine, Command/Event contract (no I/O)
crates/control        local control contract (JSON v1), pure
crates/telephony      SIP engine (rsipstack), SDP, RTP/SRTP media
crates/media          G.711, RTP, RFC 4733 DTMF, SRTP adapter
crates/audio          cpal/ALSA device bridge
crates/config         flags > env > .env > TOML > SQLite > defaults, with provenance
crates/storage        SQLite: settings, history, contacts, control ledger, grants
crates/platform       single instance, control socket (SO_PEERCRED), links, notifications, keyring
docker/asterisk/      disposable Asterisk lab and the lab suite (suite.sh)
packaging/            AppImage, licence notices, release scripts (scanner, clean-clone build)
scripts/              cargo-docker.sh (toolchain wrapper)
openspec/             proposals, designs, specs and task lists
docs/                 architecture, ADRs, receipts, reviews, screenshots
```

Layering is enforced by `crates/core/tests/layering.rs` (from `cargo metadata`): the domain uses
no infrastructure crate, SQL/SIP/audio/D-Bus each live in one crate, and the UI module never names
them. See [architecture.md](architecture.md).

## Docker workflow

`scripts/cargo-docker.sh` runs cargo (or any command) in the toolchain image, building the image on
first use. The checkout is mounted at `/work`, the container runs as your UID, `HOME` and
`CARGO_HOME` live under `target/`, and `base/` is masked by an empty tmpfs. Without arguments it
runs `cargo build --workspace --locked`.

```sh
scripts/cargo-docker.sh cargo test -p rust-microsip-storage
scripts/cargo-docker.sh bash               # interactive shell
```

| Variable | Default | Effect |
|---|---|---|
| `CARGO_TARGET_DIR` | `target/docker` | Build directory inside the checkout; parallel agents use `target/agent-<name>`. |
| `RMS_DOCKER_NET` | `bridge` | Container network; `none` for offline runs after the first fetch. |
| `RMS_TOOLCHAIN_IMAGE` | `rust-microsip-toolchain:dev` | Image tag to use. |
| `CARGO_INCREMENTAL` | `0` | Off by default to save disk. |

`RUST_LOG` and `RUST_BACKTRACE` are passed through. After changing the `Dockerfile`, rebuild the
image with `make toolchain`.

## Common tasks

The root `Makefile` wraps everything; `make help` lists the targets. Each target calls
`scripts/cargo-docker.sh`, `docker` or a repository script — never a host cargo.

| Make | Equivalent |
|---|---|
| `make build` / `make release` | `cargo build --workspace --locked [--release]` |
| `make test` | `cargo test --workspace --locked` (unit, integration, layering, bounded fuzzing, CLI/MCP process tests) |
| `make fmt` / `make fmt-check` | `cargo fmt --all [--check]` |
| `make clippy` | `cargo clippy --workspace --all-targets --locked -- -D warnings` |
| `make doc` | `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` |
| `make check` | fmt-check, clippy, test, doc |
| `make verify` | the CI `verify` stage: the same checks offline in Docker plus the licence/notice policy |
| `make dist` | `docker buildx build --target artifacts --output dist/` (ELF, AppImage, `SHA256SUMS`, notices, SBOM) |
| `make dist-check` | checksums, AppImage wraps the identical ELF, publication scan of the artifacts |
| `make scan` | `packaging/release/scan-publication.sh` (run before every push) |
| `make selftest` | the scanner's negative controls |
| `make context` | Docker context allowlist audit with a negative control |
| `make clean-clone` | build from a clean clone without `base/` or host Rust |
| `make lab-build` / `lab-init` / `suite` | lab image, fictional fixtures, lab suite |
| `make shell` | interactive shell in the toolchain container |
| `make clean` | deletes `target/` and `dist/` without asking |

New crates and features must pass the licence policy: `packaging/collect-cargo-notices.py` runs in
`verify` and rejects expressions outside the accepted list unless they are reviewed in
`packaging/reviewed-licenses.json` (archives without notices need pinned upstream texts in
`packaging/third-party-notices/`). A new source directory must also be added to `.dockerignore`,
`packaging/check-context.sh` and the `context-audit` list in the `Dockerfile`.

## Lab suite

A disposable Asterisk PBX on an internal Docker network (no published ports) with fictional
accounts; see [docker/asterisk/README.md](../docker/asterisk/README.md). Never point tests at a
real PBX or account.

```sh
make lab-build && make lab-init
make suite                                  # all scenarios
make suite SCENARIOS="gui-control gui-grants"
```

The suite builds a local `suite-runner` image (release app, engine examples, private Xvfb and
AT-SPI), starts a fresh PBX per scenario and writes `artifacts/suite/<time>-<commit>/` with one
`report.json` per scenario. Runs with uncommitted changes fail unless `ALLOW_DIRTY=1`;
`SKIP_BUILD=1` reuses an image built from the same commit. Scenarios: `engine-lab`,
`engine-network`, `gui-link`, `gui-frozen-exit`, `gui-theme`, `gui-a11y`, `gui-a11y-call`,
`gui-control`, `gui-grants`. Receipts in [docs/receipts/](receipts/) record the evidence.

## Running the app locally

Build in Docker, run the binary on the host (it needs your X11/Wayland session, a Vulkan or OpenGL
driver and `libasound.so.2`).

```sh
make dist
./dist/rust-microsip --version
./dist/rust-microsip                        # opens the window; Menu → Settings for the account
./dist/rust-microsip status                 # control CLI against the running window
```

Your personal settings, database and installed AppImage are real data: point `XDG_CONFIG_HOME`,
`XDG_DATA_HOME` and `XDG_RUNTIME_DIR` at scratch directories when experimenting, and never use a
production account for tests. See [agent-cli-mcp.md](agent-cli-mcp.md) for the CLI, MCP and the
headless service.

## Release process

See [release.md](release.md). In short: binaries are built and published only by
`.github/workflows/release.yml` on a `vX.Y.Z` tag, only for a commit whose `ci` run on `main` is
green and whose workspace version equals the tag; `ci.yml` only checks. **v0.1.0 also requires every
release gate in the OpenSpec change and the owner's decision** — tagging is the owner's act.

## OpenSpec workflow

Requirements and decisions live in [`openspec/`](../openspec/): `changes/<name>/` holds one change
each (`proposal.md`, `design.md`, delta `specs/`, `tasks.md`). Review a proposal with the owner
before implementing it. With Claude Code, the `/opsx:*` commands drive the flow (`explore`,
`propose`, `apply`, `sync`, `archive`).

`tasks.md` is the source of truth for progress: tick `- [x]` only with verifiable evidence (a
receipt in [docs/receipts/](receipts/) bound to a commit) and keep human/VM gates open until they are
really done.

## Coding rules

- `unsafe` is forbidden; `unwrap`, `expect`, `todo!`, `unimplemented!` and `dbg!` are denied;
  every public item is documented (`missing_docs = "deny"`).
- English for code, comments, CLI messages, documentation and OpenSpec tasks.
- Typed errors at boundaries; messages never echo passwords, numbers, digits or other personal data.
- Tests use fictional data only; no production calls or credentials.
- Independent reviews use the Claude CLI read-only with sanitized context
  (`claude -p --tools "" --strict-mcp-config --disable-slash-commands --no-session-persistence`);
  a review is not runtime evidence.
- Commit and push each verified milestone; run `make scan` before every push and never publish
  absolute personal paths, secrets or generated artifacts.

## Pinned inputs and isolation

- Ubuntu 24.04 `linux/amd64` and the CA bootstrap image, both pinned by digest in the `Dockerfile`.
- Builder packages from the [Ubuntu snapshot](https://snapshot.ubuntu.com/) `20261001T000000Z`
  over HTTPS with archive signatures.
- Rust 1.99.0 from the official 2026-10-01 manifest; five components with SHA-256 in
  `docker/rust-components.sha256`.
- `Cargo.lock` is versioned; the image fetches with `--locked` and compiles and tests with
  `--frozen` in `RUN --network=none` stages as UID 10001 without host mounts.
- The Docker context is an allowlist (`.dockerignore`); `make context` proves `base/`, `.git`,
  `.env`, databases and artifacts are not sent.
- Reproducibility: a no-cache clean-clone rebuild produced identical `SHA256SUMS` on the same
  machine; that is auditable inputs, not a cross-machine proof.

## Troubleshooting

- **Disk full during builds**: `target/` and Docker's build cache grow with every image rebuild;
  `make clean` removes `target/`; `docker builder prune` frees the cache (other projects rebuild
  slower afterwards).
- **`snapshot.ubuntu.com` 502/503**: the snapshot mirror is sometimes unavailable; rerun the build.
- **`--frozen` fails after a manifest change**: refresh the lock with
  `scripts/cargo-docker.sh cargo update -w` (network on) and commit `Cargo.lock`.
- **GUI tests in containers**: the suite uses Xvfb with Mesa llvmpipe; a black capture usually
  means the window did not render.
