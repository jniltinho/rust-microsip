# G0 scaffold development

## Status and commands

See [implementation approval](implementation-approval.md). The workspace contains a CLI scaffold, an experimental telephony library and an own media-primitives library, not the design's final workspace. The shipping scaffold has no SIP/account access; opt-in examples prove registration and short synthetic plain-RTP calls through the disposable PBX. GUI, human/device audio, production calls, SQLite, IPC, control CLI, MCP and AppImage do not exist yet. Starting without options returns exit code 1 and reports unavailability; invalid arguments return 2 without echoing possible passwords. Help, version, and static diagnostics return 0.

```sh
docker buildx build --platform linux/amd64 --target artifacts --output type=local,dest=dist .
./dist/rust-microsip --help
./dist/rust-microsip --version
./dist/rust-microsip --doctor --json
(cd dist && sha256sum --check SHA256SUMS)
```

At this stage `--doctor` describes compiled capabilities only, without reading HOME/XDG, configuration, SIP environment variables, databases, networks, or devices. It is not yet task 3.6's planned runtime diagnostic. Do not use the scaffold with a real account.

The local exporter writes files owned by its invoking user. It neither installs the executable nor changes the existing AppImage. `dist/` and logs in `artifacts/` are ignored by Git. Do not create tags/releases from this scaffold.

The [isolated SRTP pre-adoption diagnostic](receipts/g0-srtp-candidate-2026-10-13.md)
is separate from the application workspace and its lockfile. Its inventory and
four local positive tests do **not** approve SRTP: the duplicate-packet rejection
test fails for unadapted rustrtc 0.4.7. The probe is outside normal Cargo targets
and the Docker context; the application still has no secure call/profile. An
[isolated per-SSRC packet adapter](receipts/g0-srtp-adapter-2026-10-13.md) now has
29 passing tests and separate SRTP/SRTCP replay windows. It is not the complete
secure-call owner. A separate [initial TLS/SDES experiment](receipts/g0-secure-call-2026-10-13.md)
now verifies four encrypted G.711 PBX calls and negatives, without adopting the
SDK/secure profile in the application. See [ADR 0011](adr/0011-isolated-initial-tls-sdes-calls.md)
for strict scope, key-erasure limits and remaining secure-control gates.

## Pinned inputs and isolation

- Ubuntu 24.04 `linux/amd64`, digest in the Dockerfile.
- Bootstrap CAs from `curlimages/curl:8.17.0`, also digest-pinned. Only the CA bundle is copied; TLS verification is not disabled.
- Builder packages from [Ubuntu snapshot](https://snapshot.ubuntu.com/) `20261001T000000Z`, exclusively over HTTPS with Ubuntu archive signatures. `Check-Valid-Until: no` allows reuse of this historical snapshot; it does not disable signatures or package checksums.
- Rust 1.99.0, official [2026-10-01 metadata](https://static.rust-lang.org/dist/channel-rust-1.99.0.toml), five components with SHA-256 in `docker/rust-components.sha256`. Standalone installation occurs only in the image; `rust-toolchain.toml` enables the same version in opt-in rustup development environments.
- Hashes were obtained over HTTPS from that versioned official manifest and compared with Docker downloads. The `.asc` PGP signature was not independently verified; initial trust is distributor HTTPS/CA plus the repository pin, not independent attestation.
- Versioned `Cargo.lock`, `--locked` fetching, `--frozen` compilation/tests in `RUN --network=none` stages, executed as UID 10001 without host mounts.
- Allowlisted Docker context in `.dockerignore`: `base/`, `.git`, `.env` files, databases, AppImages, artifacts, and personal settings are not sent.

The CLI scaffold itself has no external crates; the experimental adapter does. Docker verification checks the selected dependency deny boundary, registry source/lock checksums, reviewed provider pins, known permissive license expressions and preserved archive notices. It does not replace vulnerability, complete license/source or native audits. `cargo-licenses/inventory.json` includes the selected Linux workspace graph (including G0 dev/build dependencies), with exact archive/notice hashes and features; it is not an ELF link map. The historical [registration inventory](inventory/g0-registration-cargo.json) and current [media inventory](inventory/g0-media-cargo.json) record their respective selected graphs. `std` is linked into the ELF, which is **dynamic** with respect to glibc/libgcc_s. Complete native/dlopen auditing remains part of G0. Rust component notices accompany the export, including generated `COPYRIGHT.html`, `COPYRIGHT-library.html`, and `licenses/`; missing required notices fail the builder. `scaffold-sbom.cdx.json` is a preliminary workspace/build inventory, not the future release SBOM. `elf-dynamic.txt` and `elf-versions.txt` describe the CLI ELF only.

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

Rust tests verify real process contracts, exit codes, conflicting arguments, non-UTF-8 input, no secret-marker echo, and environment/configuration-independent diagnostics. Adapter tests cover redaction, validation, cancellation and a finite unreachable-registrar deadline. See the [registration receipt](receipts/g0-rust-registration-2026-10-12.md) and [lab guide](../docker/asterisk/README.md) for actual PBX execution. The [media receipt](receipts/g0-rust-media-2026-10-12.md) records 12 synthetic five-second calls, exhaustive functional G.711 comparisons and bounded RTP tests. Silence, local echo and wrong-tone controls must fail. The [DTMF receipt](receipts/g0-rust-dtmf-2026-10-12.md) adds allocation-free RFC 4733 primitives, negotiated event-mask/sender tests and 12 actual positive/negative IVR cases. The [hold receipt](receipts/g0-rust-hold-2026-10-12.md) adds twelve synthetic hold/resume/rejected-offer calls and current-source pair-call/IVR regressions. The [transfer receipt](receipts/g0-rust-transfer-2026-10-12.md) adds 24 real synthetic REFER/NOTIFY cases, destination-tone validation and preserved original-call audio after a 404. These separate experiments do not establish production SIP-06/SIP-08, human voice, production SRTP, resampling or jitter-buffer readiness.

## Next steps

The [Asterisk infrastructure](../docker/asterisk/README.md) now satisfies task 1.4's readiness/isolation scope. Task 1.3 now has a [pre-adoption inventory and dependency boundary](adr/0003-g0-license-and-native-boundaries.md); task 1.5's host/VM acceptance and candidate interoperability still need execution before approving G0; neither registration fixtures nor configuration checks prove media. Task 1.8 requires the spike ELF to contain the real complete stack; the headless scaffold ELF is no substitute. Claude code review neither approves G0 nor replaces telephony or VM acceptance tests.
