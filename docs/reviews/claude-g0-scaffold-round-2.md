# Claude CLI review — G0 scaffold, round 2

Date: 2026-10-12. Sanitized input SHA-256 `57484f9d13a3809893bca7a398c5dd2d362b217e2c86660e26df3cec07c64ce4`. CLI print/safe mode, empty tools, empty/strict MCP, hooks/auto-memory disabled, isolated settings, no session persistence. Exit 0, subtype `success`, is_error `False`. Static review only; **APPROVE applies only to the scaffold**, never G0/release. **English summary translation of the historical report, not a new review.**

---

# Static reevaluation: rust-microsip G0 scaffold, round 2

**Verdict: APPROVE**, limited to Docker scaffold + static diagnostic CLI + tests + export.

I ran no build, tests, Docker, or CLI; neither independently compared Rust-manifest hashes nor compared LICENSE byte-for-byte with FSF canonical text. This does not approve G0, complete DIST-03, calls, SIP, media, desktop, or release. Missing functions are honestly unavailable and do not count against the scaffold scope.

## Round 1 blockers

### B1 — Negative control resolved
- Output is captured using `--progress plain 2>&1`. Required `CONTEXT_REJECTED:` must have a BuildKit runtime log prefix (`^#N time`), so echoed heredoc/Dockerfile text cannot satisfy it.
- Quoted `'base/private.txt'` and `'.env'` must appear on the rejection line; `.env.local` cannot falsely match.
- Synthetic-content-only rejection does not contain sentinels, so cannot incorrectly pass.
- A changed BuildKit output format fails closed.

### B2 — Recursive copying resolved
- Fourteen own files are individually enumerated without recursive copying; symlinks/nonregular files are refused.
- Negative context contains only own code and fictional sentinels, leaving no real private data in residual cache.
- Documentation correctly describes current behavior.

### B3 — Licenses/SBOM resolved
- Builder fails without MIT/Apache notices for each component, COPYRIGHT for rustc/std, and Cargo third-party notices.
- Generated COPYRIGHT.html, COPYRIGHT-library.html, and licenses/ are required/copied into generated-notices/.
- `set -eu` applies in loop/case bodies and stops the build on failure.
- SBOM no longer claims one std license expression; it states incompleteness and links notices, appropriate for preliminary inventory.

### B4 — Evidence sufficient
- Five component pins are provided; the 2026-10-01 Rust 1.99.0 distribution URL fits the release cadence. Official HTTPS-manifest provenance and missing PGP verification are honestly documented.
- LICENSE contains GPL3 sections 0–17 and appendix. NOTICE is coherent.
- Ignore rules cover dist/artifacts/base, env, databases, AppImages, keys, and captures.
- Receipt's ten tests match cli.rs, its source-manifest hash matches exported JSON, and it identifies uncommitted source by hashes without pretending G0 passed.

## Remaining observations (nonblocking for this scope)

**Before G0/release**
1. Independently verify the Rust manifest PGP signature and record the release-key fingerprint; current trust is HTTPS/CA plus pin.
2. Binary distribution must satisfy GPL Corresponding Source delivery/offer obligations; the undistributed scaffold did not yet trigger this distribution step.
3. Link receipt to the eventual commit; consider adding check-context.sh to source/test manifests.

**Hygiene**
1. Existing dist exports can retain unrelated old files despite passing checksums. Documented limitation; empty temporary export would strengthen testing.
2. Leaf-only symlink checks do not reject symlinked apps/packaging ancestors; realpath containment could close this gap.
3. LICENSE appears reformatted relative to FSF canonical text; content seems intact, but unmodified canonical text simplifies automated recognition/verbatim compliance.
4. Desktop stderr still ignores write errors unlike argument-error handling.
5. Pinned-version security review lacks an assigned owner/frequency.

## Summary

B1–B3 are corrected without accidental approval of unrelated failures; B4 is sufficient for static review. Scaffold work may proceed. PGP verification, complete transitive inventory (1.3), Asterisk (1.4/1.5), interoperability (1.6/1.7), complete-stack ELF/VMs (1.8), and G0 decision (1.9) remain pending.
