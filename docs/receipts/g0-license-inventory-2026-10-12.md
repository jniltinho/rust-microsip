# G0 pre-adoption inventory receipt — 2026-10-12

Status: **task 1.3 inventory/ADR verified; G0 remains pending**. Baseline commit `7104510`; exact tested working-tree sources are identified below, before their milestone commit. No candidate application dependency or font was incorporated by this milestone. This is engineering evidence, not legal certification or a full release SBOM/security audit.

## Identity

- [ADR 0003](../adr/0003-g0-license-and-native-boundaries.md) inventories current incorporation, direct candidates, blocked components, fonts/icons/tones and expected native interfaces before adoption.
- Candidate-catalog SHA-256: `9d68a2397e6e2168c7a3b2f210eedf235693698bd8efb88345420763a800a576`; 16 direct checksum-verified published archives and one separately recorded ring transitive candidate. VCS-origin and archive-origin notice paths/hashes are distinct.
- Dependency checker SHA-256: `ed5abfd311af9268a9297e7760db54ecc0bbae9fb5f2ba7510ba13b10ae16c60`.
- Checker-tests SHA-256: `a2be5afbf362919194036154f02eab34d780726307929808eff3056aac10aa0d`.
- Clean-copy context script SHA-256: `8f833d8eae3947d3673f84fdf759526ce3519ff48b34db2ecfc1bb4bb2cd800f`.
- Rebuilt source-manifest SHA-256: `05ce028e4f088fdd744619ecb5a4f17bba5ba6c000c9e3a4579c24983f4dcf94`; canonical sorted compact JSON source path/hash map in `packaging/export-scaffold.sh`, now including both allowlisted Python sources.
- Rebuilt scaffold ELF SHA-256 remains `d24ff6ebc1496bbddb8fd1004c2840b6d22a97056432701bbb3865a25ddc8ea0`, 325576 bytes. It is still help/version/static diagnostics only, not a softphone.

## Executed evidence

1. Archived candidate bytes matched the official registry SHA-256. Bounded extraction refused absolute/parent-traversal and non-file/directory entries. Exact Cargo manifests, VCS identity and available notices were inspected in ignored research storage, never `base/` or personal profiles.
2. `audio-codec` 0.4.9 had an MIT declaration but no complete license notice in the archive or attempted exact-revision LICENSE lookup. It is blocked directly **and transitively**, at this or any unreviewed version; requiring parents inherit the block. G.711 remains mandatory and needs a resolved candidate, not a waived gate.
3. TDD RED: checker implementation absent, test import failed. GREEN: **9 unit tests** passed. Cases cover zero-dependency scaffold, reviewed ring/RustCrypto data-plane shape, static/dynamic blocked providers, transitive codec, rustls provider/default feature union, version changes, implicit fonts, malformed/oversized input and mixed runtimes/media features.
4. Review R1 reproduced RED: arbitrary `rustrtc|upnp` feature was erroneously accepted. Requiring **an empty selected rustrtc feature set** fixed it; all nine tests passed again, including under `python3 -O`. Checks do not depend on assertions.
5. CLI negative control rejected a blocked provider with a synthetic private source marker, exit 1, without echoing the marker. An **actual external cargo-tree** negative control rejected the exploratory manifest containing blocked `audio-codec`; metadata alone was not treated as proof of selected compilation.
6. Final `sh packaging/check-context.sh`, exit 0: positive context, explicit synthetic `.env`/`base/private.txt` privacy rejection, clean-copy offline Docker verification and export. Nine Python tests, actual selected scaffold dependency-tree check, fmt/clippy, ten Rust process tests, one doctest, rustdoc and lock regeneration/comparison passed. Export SHA256SUMS passed. No host Cargo was needed.
7. Builder/context allowlists and copied-source manifests include exactly the checker and its test file; unlisted Python files are not admitted. Export source hashes cover them. NOTICE points to preserved Rust/toolchain notices without relicensing std.
8. Read-only Claude [round 1](../reviews/claude-license-round-1.md) found B1/B2; [round 2](../reviews/claude-license-round-2.md) confirmed those fixes and found R1; [round 3](../reviews/claude-license-round-3.md) returned **APPROVE** for the revised narrow scope. Static review is not runtime, license certification or G0 acceptance.
9. Strict validation passed for all three OpenSpec changes. Link/privacy checks found no personal home paths, credentials, local artifacts or copied reference sources in publishable content.

Raw RED/GREEN/review/build logs remain in ignored `artifacts/`, notably `license-boundary-red.log`, `license-r1-red.log`, `license-r1-green.log`, `license-optimized-green.log` and `license-r1-context-final.log`. Research metadata/source/lockfiles also remain ignored; they are not application dependencies or release artifacts.

## Limits that remain enforced

The selected-tree checker is a **narrow deny boundary**, not a complete license allowlist, crypto audit, SBOM or provenance verifier. Its current source-suffix parsing does not itself authenticate registry origin; at adoption, verify Cargo.lock source/checksum and reject unreviewed path/git patches of reviewed crate names/versions. Run it for each shipping feature/target combination; current workspace-default invocation matches the current scaffold build only. Dev-only dependencies also require notice/source review before their compilation even though this shipping-tree check excludes dev edges.

Missing Apache workspace NOTICE checks for cpal/rmcp, font-file revision/hash/OFL attribution, full transitive notices/native build scripts, vulnerabilities and complete-stack DT_NEEDED/dlopen/clean-VM evidence remain adoption/release gates. No third-party resource may be copied as a workaround. Expected SONAMEs are an inventory, not a measured complete-stack manifest. No Rust calls/media, desktop, AppImage, CLI/MCP control or usable release is established.
