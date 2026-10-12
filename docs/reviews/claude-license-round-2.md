# Claude license/native inventory review — round 2

Date: 2026-10-12. Read-only sanitized Claude CLI with tools, hooks, MCP and persistence disabled. Input SHA-256: `b220573b4e534fb4fed0438c5123d1d3840f2ef7283b12b989d92d62a9a2b2f9`. CLI exit 0, subtype success, is_error false. R1 was subsequently reproduced RED with an arbitrary `upnp` feature and fixed by requiring an empty rustrtc feature set. The ring transitive entry/notice hashes were also added. Follow-up and executed evidence are recorded separately; no legal/runtime certification is implied.

---

# Verdict: REVISE

One concrete blocker. Everything else holds up for the narrow pre-adoption scope stated.

## Blocker

**R1. The rustrtc feature gate is a denylist, but the ADR describes it as rejecting any unreviewed feature.**

- **What the docs claim:**
  - The ADR says the checker rejects "unreviewed rustrtc features".
  - The reviewed configuration is the no-default-feature manifest, so the reviewed feature set is empty.
  - The JSON `media_provider` describes it as "no-default-features; RustCrypto data plane only".
  - The rustrtc row says not to enable "its DTLS/UPnP/ICE defaults".
- **What the code does:** `check-g0-dependencies.py` rejects only `{"default", "std", "dtls", "crypto-p256"}`.
  - Any other rustrtc feature passes, including any separately named UPnP, ICE, SCTP or alternative-crypto feature in 0.4.7, or one added by a parent's feature unification.
  - Such a feature could pull a non-RustCrypto or unreviewed native provider into the selected tree. It would only be caught if that provider's crate name happens to be in `BLOCKED`.
- **Why it blocks:** This is the same kind of gap B1 asked to close: an unreviewed provider can enter through feature unification.
- **Fix (either one):**
  - **Preferred:** reject any non-empty feature set, i.e. `if name == "rustrtc" and (version != "0.4.7" or features):`. Then add a test that some arbitrary other feature (e.g. `rustrtc v0.4.7|upnp`) is rejected. Keep `test_accepts_reviewed_ring_and_rustcrypto_data_plane`, which already uses an empty feature set.
  - **Alternative:** if specific non-default features were actually reviewed, list them as an explicit allowlist in the checker and the JSON, and reject everything else.
  - Narrowing the ADR wording to "four named features" is not enough on its own, because it would contradict the "no-default-features only" boundary.

## Checked, no blocker

- **Transitive codec block.** `audio-codec` is rejected wherever it appears in the selected tree, at any version, so any parent requiring it is effectively blocked. The test covers a transitive case and a version bump. B2 is satisfied.
- **TLS boundary.**
  - rustls must be exactly 0.23.45 with `ring`, and is rejected with `default`, `aws_lc_rs`, `aws-lc-rs` or `fips`.
  - ring must be exactly 0.17.14, and that version must be present whenever rustls is.
  - AWS-LC, OpenSSL and BoringSSL crate names are blocked anywhere, which covers static vendoring independent of DT_NEEDED.
  - Feature unification is caught because `{f}` reports the unified enabled set and features are merged per (name, version).
  - B1 is satisfied.
- **Fonts.** `default_fonts` is rejected on eframe, egui and epaint, and `epaint_default_fonts` is blocked by name.
- **Parsing.**
  - Malformed, empty and oversized input fails closed, and the input is never echoed (privacy is fine).
  - `(proc-macro)`, path, git and `(*)` suffixes are handled.
  - Build-edge section headers are not emitted with `--prefix none`. Note that the zero-dependency scaffold never exercises this path, so re-confirm on the first real tree.
- **Selected-tree scope.** For a virtual workspace, `cargo tree` covers all members with default features, which matches `cargo build --workspace`. Using the target and `-e normal,build` rather than metadata is the correct choice, and the ADR correctly explains why metadata over-reports optional crates.
- **Packaging consistency.** The new checker and tests are included consistently in `.dockerignore`, the context-audit allowlist, the `check-context.sh` copy list and the export source hashes.
- **Honest scope statements.** The docs consistently describe a deny boundary rather than an allowlist, SBOM or audit. audio-codec is called out as blocked with a missing notice. The cpal and rmcp Apache NOTICE checks are recorded as pending. AWS-LC licensing is not asserted. Test results are presented as supplied facts.

## Non-blocking notes

- ring 0.17.14 is now a pinned part of the boundary, and the ADR says its LICENSE, LICENSE-BoringSSL and LICENSE-other-bits were inspected. However, the JSON has no ring entry with archive and notice SHA-256, unlike the sixteen direct candidates. Adding one would make the B1 evidence traceable.
- The RustCrypto SRTP crates are listed by version only. That is acceptable, since the ADR defers exact pins and notices to adoption.
- The rustrtc rerun mentioned in your summary has not been verified here. Make sure the R1 test is part of that rerun.
