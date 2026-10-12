# G0 scaffold development

## Status and commands

See [implementation approval](implementation-approval.md). The first slice is a single-binary workspace, not the design's final workspace. GUI, SIP, audio, SQLite, IPC, control CLI, MCP, and AppImage do not exist yet. Starting without options returns exit code 1 and reports unavailability; invalid arguments return 2 without echoing possible passwords. Help, version, and static diagnostics return 0.

```sh
docker buildx build --platform linux/amd64 --target artifacts --output type=local,dest=dist .
./dist/rust-microsip --help
./dist/rust-microsip --version
./dist/rust-microsip --doctor --json
(cd dist && sha256sum --check SHA256SUMS)
```

At this stage `--doctor` describes compiled capabilities only, without reading HOME/XDG, configuration, SIP environment variables, databases, networks, or devices. It is not yet task 3.6's planned runtime diagnostic. Do not use the scaffold with a real account.

The local exporter writes files owned by its invoking user. It neither installs the executable nor changes the existing AppImage. `dist/` and logs in `artifacts/` are ignored by Git. Do not create tags/releases from this scaffold.

## Pinned inputs and isolation

- Ubuntu 24.04 `linux/amd64`, digest in the Dockerfile.
- Bootstrap CAs from `curlimages/curl:8.17.0`, also digest-pinned. Only the CA bundle is copied; TLS verification is not disabled.
- Builder packages from [Ubuntu snapshot](https://snapshot.ubuntu.com/) `20261001T000000Z`, exclusively over HTTPS with Ubuntu archive signatures. `Check-Valid-Until: no` allows reuse of this historical snapshot; it does not disable signatures or package checksums.
- Rust 1.99.0, official [2026-10-01 metadata](https://static.rust-lang.org/dist/channel-rust-1.99.0.toml), five components with SHA-256 in `docker/rust-components.sha256`. Standalone installation occurs only in the image; `rust-toolchain.toml` enables the same version in opt-in rustup development environments.
- Hashes were obtained over HTTPS from that versioned official manifest and compared with Docker downloads. The `.asc` PGP signature was not independently verified; initial trust is distributor HTTPS/CA plus the repository pin, not independent attestation.
- Versioned `Cargo.lock`, `--locked` fetching, `--frozen` compilation/tests in `RUN --network=none` stages, executed as UID 10001 without host mounts.
- Allowlisted Docker context in `.dockerignore`: `base/`, `.git`, `.env` files, databases, AppImages, artifacts, and personal settings are not sent.

The scaffold has no external crates. `std` is linked into the ELF, which is **dynamic** with respect to glibc/libgcc_s. Candidate dependencies and complete native/dlopen auditing remain part of G0. Standalone Rust component notices accompany the export, including the distribution-generated `COPYRIGHT.html`, `COPYRIGHT-library.html`, and `licenses/` tree; missing required notices fail the builder. `scaffold-sbom.cdx.json` is a preliminary inventory without a single std license expression, not the future release SBOM. `elf-dynamic.txt` and `elf-versions.txt` describe this ELF only.

Pinned versions need periodic security review. Reproducibility here means auditable inputs, not proven bit-for-bit identity. `-C target-cpu=native` is not used.

## Tests

```sh
# fmt/clippy/test/rustdoc, offline after fetching:
docker buildx build --platform linux/amd64 --target verify .

# Positive context, negative sentinel control, and clean-snapshot build:
sh packaging/check-context.sh
```

The second command makes a temporary copy of individually enumerated own files only, refusing symlinks and never recursively copying directories or `base/`. It injects **synthetic** private files, confirms exclusion, makes the filter permissive, and requires the audit to identify the `base/private.txt` and `.env` sentinels. A network/build failure does not count as privacy rejection. It then removes the sentinels and builds without `base/`. Besides Docker, this test uses common POSIX utilities (`cp`, `mktemp`, `sha256sum`); the main build still requires no host Cargo.

The local exporter does not remove leftovers from previous runs; reserve `dist/` for builds and use an empty destination when comparing or distributing artifacts. Checksums provide local integrity, not authorship/provenance signatures.

Rust tests verify real process contracts, exit codes, conflicting arguments, non-UTF-8 input, no secret-marker echo, and environment/configuration-independent diagnostics. RED/GREEN evidence is in the [initial receipt](receipts/g0-scaffold-2026-10-12.md) and [English rebuild receipt](receipts/g0-scaffold-english-2026-10-12.md). No call tests were run in this slice.

## Next steps

Complete task 1.3's inventory/ADR and the disposable Asterisk PBX (1.4/1.5), before adopting the SIP/media stack. Task 1.8 requires the spike ELF to contain the real complete stack; the headless scaffold ELF is no substitute. Claude code review neither approves G0 nor replaces telephony or VM acceptance tests.
