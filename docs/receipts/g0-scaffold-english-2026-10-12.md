# English scaffold rebuild — 2026-10-12

Status: **scaffold only; G0 and release remain pending**. This records the rebuild after the user's English-language requirement, not a replacement for the [historical initial receipt](g0-scaffold-2026-10-12.md). Work is based on `14b46b9`; source hashes identify the tested working tree before its milestone commit.

## Identity

- Version: `0.1.0-dev.0`; target: `x86_64-unknown-linux-gnu`.
- ELF SHA-256: `d24ff6ebc1496bbddb8fd1004c2840b6d22a97056432701bbb3865a25ddc8ea0`; 325576 bytes.
- Source-manifest SHA-256: `243a3192fcb7d821ff23c7fb2bf07be3455349a7fb2e720ae3503bcf6c5d3317`. Canonical representation: sorted JSON source-file/hash map, compact separators, as implemented in `packaging/export-scaffold.sh`.
- Context-test script SHA-256: `b72cf3634ee439d6a7200ae66a67052efe3e6091dca86fabb148d8d94abe91b9`.
- Rust 1.99.0; Ubuntu 24.04 snapshot `20261001T000000Z`; all image/component pins remain in the Dockerfile and `docker/rust-components.sha256`.

## Executed checks

1. Language-contract RED: three process tests failed and seven passed before translating implementation messages. Failures were help, text diagnostics, and honest desktop-unavailability output.
2. GREEN: ten process tests and one doctest passed; fmt, clippy with warnings denied, rustdoc with warnings denied, frozen/offline verification, and lockfile regeneration/comparison passed in Docker.
3. `sh packaging/check-context.sh` returned 0 after the final Cargo description change: allowlisted positive context; negative control identified synthetic `base/private.txt` and `.env` using `CONTEXT_REJECTED`; clean-copy artifact build passed. No arbitrary Docker failure counts as negative-test success.
4. Exported SHA256SUMS passed verification. Help/version/doctor smoke passed; JSON reports schema 1, `g0_passed=false`, `host_probed=false`, and unimplemented SIP/media/desktop/storage/IPC.
5. Scaffold `DT_NEEDED` is limited to `libgcc_s.so.1` and `libc.so.6`. This is not an audit of the future GUI/audio/SIP stack or its dlopen dependencies.

Raw logs remain ignored: `artifacts/english-cli-red.log` and `artifacts/english-scaffold-final.log`. The binary and build manifest remain in ignored `dist/`. No host Rust installation, personal configuration, installed AppImage, or local-only references were used.

The [scaffold's second Claude review](../reviews/claude-g0-scaffold-round-2.md) approved its infrastructure scope. Tests above, not that review, establish the language rebuild result.

No desktop, calls, media, persistence, IPC, MCP, or AppImage exists in this scaffold. No usable release is claimed. Native/transitive license inventory, full G0, and exact-artifact desktop/audio acceptance remain mandatory.
