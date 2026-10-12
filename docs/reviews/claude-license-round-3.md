# Claude license/native inventory review — round 3

Date: 2026-10-12. Read-only sanitized Claude CLI; tools, MCP, hooks and persistence disabled. Input SHA-256: `fd9f3b0d4db308c125d61b9d4838edc1c3bd9545eb332f5b105ba98b779a651d`. CLI exit 0, subtype success, is_error false. **APPROVE for pre-adoption inventory/deny-boundary scope only.** The final clean-context rerun subsequently completed exit 0; executed evidence is in the [receipt](../receipts/g0-license-inventory-2026-10-12.md). Nonblocking adoption-time suggestions remain open; this is not G0, legal or runtime certification.

---

## Verdict: APPROVE

I found no concrete blockers within the stated scope: a narrow pre-adoption deny boundary, with no app dependencies adopted yet.

The round-2 R1 fix is correct. `rustrtc` now requires exactly `0.4.7` and an empty selected feature set (`features` non-empty means reject), so the reviewed set really is empty. The `upnp`, `std` and `dtls` subtests, plus the version-mismatch subtest, cover it. The checker has no `assert`s, so behaviour under `-O` is unchanged. The test count (9), the ADR text, the inventory JSON (16 direct items plus reviewed `ring`) and `verify.sh` agree with each other.

## Nonblocking suggestions (for adoption time)

1. **Source provenance is discarded.** The parenthetical source in `{p}` is matched but not used. So a `[patch]`-redirected or git/path `ring v0.17.14`, `rustls v0.23.45` or `rustrtc v0.4.7` would pass, even though the review covered only the registry archive SHA-256. A cheap fix: for these three crates, require no source suffix, and key `found` by `(name, version, source)`.
2. **Selected feature set vs. release build.** The docstring says "exact target and feature set being built," but `verify.sh` runs `cargo tree` with workspace defaults. Once members or features exist, run the check per shipping package with the same `-p`/`--features`/`--no-default-features` used for the release build. Also pass `--workspace` explicitly if the root is not a virtual manifest.
3. **Deny-list breadth.** Consider adding obvious siblings now, since they are cheap and match the ADR's "any other unreviewed C crypto provider":
   - `aws-lc-fips-sys`
   - `libsrtp`/`libsrtp-sys`
   - `mbedtls-sys`
   - `wolfssl-sys`
   - `openssl-macros` is optional

   Normalizing `-`/`_` on both sides would also cost nothing for non-registry sources.
4. **Dev-dependencies.** `-e normal,build` correctly excludes dev-deps from the shipped boundary. Note in the ADR that test-only deps still need license and provenance review, because Docker builds them.
5. **Test coverage.** Two small additions would help:
   - a positive test that a proc-macro or git-sourced line (`name v1.0.0 (proc-macro)`, or `name v1.0.0 (https://…#rev)`) parses;
   - a negative test for a `rustls` tree that lacks `ring 0.17.14` (the "missing reviewed TLS provider" path).

Pending: confirm that the clean-context Docker rerun finishes GREEN before you treat this as final.
