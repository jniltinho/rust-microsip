# Claude CLI review — G0 scaffold, round 1

Date: 2026-10-12. Sanitized input SHA-256 `1b0efcfdc9d2afc502e3741159b65240778900d15d641c8df24e2b7ac96f73b0`. CLI print/safe mode, empty tools, empty/strict MCP, hooks/auto-memory disabled, isolated settings, no session persistence. Exit 0, subtype `success`, is_error `False`. Static review only; no tests run and no G0 approval. **English summary translation of the historical report, not a new review.**

Findings were assessed: B1 fixed with a marker/sentinel identification; B2 with individual copying without recursion/symlinks; B3 with complete generated notices, mandatory checks, and SBOM without an inaccurate single expression. B4 receives files/evidence in round 2. The first input preceded some B1 fixes.

---

# Independent static review: rust-microsip G0 scaffold

**Verdict: REVISE**

This is static review only. I ran no build, tests, Docker, or CLI. Nothing here claims G0 approval or ready SIP/UI/audio, nor does the supplied material. Scope is generally well contained: correct `.dockerignore` allowlist, appropriate offline stages, digest/SHA-256 pins, no argument echo, and honest `g0_passed=false`. Blockers concern the negative control, context script isolation, licenses, and missing evidence.

## Blockers

### B1 — Negative control accepts any failure as success
The initial `check-context.sh` treated any failed `context-audit` build as detection of a permissive filter. An unavailable daemon, pull/snapshot failure, syntax/frontend error, or full disk would pass. It did not distinguish audit failure.

**Fix:** emit an audit-specific marker (for example `CONTEXT_AUDIT_VIOLATION` and a dedicated exit code), capture `--progress=plain 2>&1`, and require that marker. Other failures must fail the test. Also confirm `unexpected` includes expected sentinels such as `base/private.txt` and `.env`.

### B2 — Context script reads/sends arbitrary real-tree content
The original `cp -R apps` and `cp -R packaging` copied every file, including untracked local `.env`/IDE outputs. With an empty `.dockerignore`, negative-control scratch content reached BuildKit; `COPY . /context/` could leave real content in cache before RUN failed. This contradicted the no-personal-config requirement and individually enumerated-copy documentation.

**Fix:** copy only enumerated allowlisted files (`Cargo.toml`, Rust sources/tests, verify/export scripts), rejecting anything outside that selection. Optionally run the negative case without cache in a disposable builder.

### B3 — Unverified toolchain notices and inaccurate SBOM license
A top-level `find ... LICENSE* COPYRIGHT` succeeds even when it finds nothing. The statically linked std requires appropriate notices, but the ADR's “exported” claim was not guaranteed. The preliminary SBOM's std `MIT OR Apache-2.0` expression was inaccurate because std includes third-party code under other licenses, such as Unicode tables and internal crates with Zlib, 0BSD, or Unlicense alternatives.

**Fix:** require appropriate per-component COPYRIGHT/MIT/Apache notices; determine whether COPYRIGHT points to generated third-party material and include it; qualify the SBOM as incomplete with a notice-directory reference instead of an exact std expression.

### B4 — Required evidence missing from this review
Provide:
- `docker/rust-components.sha256`: components/hashes/provenance, whether the official manifest and PGP signature were verified, and how hashes were obtained/checked.
- Complete `LICENSE`/`NOTICE`.
- `.gitignore` proving `dist/`/`artifacts/` exclusion.
- Receipts and implementation approval supporting claimed RED/GREEN execution.

## Nonblocking observations

**Build/provenance**
1. Pin the Dockerfile frontend; heredoc/offline RUN behavior otherwise depends on the daemon frontend.
2. Avoid duplicated literal Rust/snapshot/image values in the manifest; derive from toolchain/arguments.
3. Local exports retain old destination files; checksums validate only listed files. Prefer an empty temporary destination and full replacement.
4. Use `find -print0 | sort -z | xargs -0` for filenames with spaces. Checksums are unsigned local integrity, not provenance.
5. Lockfile regeneration mutates inherited `/work`; comparison makes this safe, but a copy would be cleaner.
6. `x86_64-unknown-linux-gnu` produces a dynamic glibc/libgcc_s ELF. “Static” must mean static diagnostics, not static linking.
7. Separate doctest execution is redundant with workspace tests.

**CLI/tests**
1. Desktop stderr uses ignored write results while argument errors handle them; minor inconsistency and asymmetric exit codes on stderr failure.
2. The initial JSON test omitted storage/ipc/host_probed and did not parse JSON; export indirectly parses it.
3. Environment isolation test should compare stdout byte-for-byte with `env_clear()`, not merely search for a marker.
4. Invalid-argument tests should require nonempty stderr.
5. Signal trap handlers should exit instead of continuing after cleanup.

**Documentation/fidelity**
1. Correct tone: G0 pending, no calls/releases/AppImage, doctor does not probe the host, no ready SIP/UI/audio claims.
2. “Auditable inputs, not proven bit-for-bit identity” is appropriate until SOURCE_DATE_EPOCH/comparative rebuilds exist.
3. Pinned inputs need a security-update process with an owner/frequency.

## Summary

Fix B1–B3 and supply B4 provenance, licenses, and execution receipts for reevaluation of scaffold + Docker builder + CLI + tests. Even approval does not satisfy complete DIST-03/G0.
