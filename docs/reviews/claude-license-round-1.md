# Claude license/native inventory review — round 1

Date: 2026-10-12. Read-only sanitized Claude CLI; tools, hooks, MCP, persistence disabled. Input SHA-256: `0ee1f01f175b0e323f60e4e0e774534864e50f20664bbf4319a443a0f1ed8e81`. CLI exit 0, subtype success, is_error false. Review only, not legal certification or runtime evidence. B1/B2 were addressed in the revised ADR and automated selected-tree boundary; follow-up review is recorded separately.

---

# Verdict: **REVISE**

The ADR is mostly sound. It is properly non-certifying, scopes itself to evaluation, keeps the PBX outside the client's license, and blocks audio-codec explicitly. Two gaps in the licensing and native boundary should be fixed before it is used to gate adoption. Both are wording or inventory additions, not a redesign.

This review covered only the supplied text. I did not verify hashes, upstream license files, crate feature graphs or registry data. Statements below about upstream defaults should be confirmed against the exact resolved metadata.

## Blockers

**B1 — The crypto provider boundary is incomplete; statically vendored crypto can bypass it.**
- The TLS row covers only ring, and the only stated check is "no hidden libssl/libcrypto requirement", which is a dynamic-linking test.
- rustls 0.23's default features select the aws-lc-rs provider. rsipstack's `rustls` feature, tokio-rustls or another crate could enable it through feature unification, even if your own manifest selects ring.
- aws-lc-sys, and vendored OpenSSL via openssl-src, build statically. They would never show up as a libssl/libcrypto DT_NEEDED entry.
- aws-lc-sys has historically declared OpenSSL/SSLeay-derived terms. That license family is GPL-incompatible unless the exact version shows otherwise.
- Separately, rustrtc's SRTP needs AES-CM/HMAC-SHA1 (and possibly AES-GCM) from some crypto backend. Neither the candidate table nor the native table identifies it.
- **Required fix:**
  - State a crypto allowlist: ring only.
  - List aws-lc-rs, aws-lc-sys, openssl-sys, openssl-src and any other C crypto backend as out of boundary pending review.
  - Require rustls `default-features = false`.
  - Make the per-adoption check inspect the resolved feature tree for these crates, not only `ldd`/DT_NEEDED output.
  - Record rustrtc's SRTP crypto provider as an open inventory item.

**B2 — The audio-codec block is scoped to the candidate, not the dependency graph.**
- "Missing codec notice blocks that candidate only" does not say what happens if rsipstack, rustrtc or another crate pulls audio-codec in transitively, by default or through a feature.
- The stop criteria handle missing transitive notices only as a *release* blocker. That leaves an adoption-time path for the blocked crate to enter through another candidate. The same publisher maintains audio-codec, rsipstack and rustrtc, which makes this realistic.
- **Required fix:** state that audio-codec, at this version or any unreviewed one, is blocked wherever it appears in the selected build's resolved feature tree, direct or transitive. Any candidate that cannot be built without it inherits the block.

## Nonblocking observations

1. **Notice provenance in the JSON is ambiguous.**
   - Keys like `UPSTREAM-LICENSE-APACHE` and `UPSTREAM-LICENSE` (eframe, rmcp) are synthetic names. They don't say whether a file came from the crate archive or from the VCS at `source_revision`, or at which path.
   - Add per-file `origin` (archive/VCS) and `path` fields.
   - For the Apache-2.0-only crates (cpal, rmcp), add an explicit field saying whether an Apache `NOTICE` file exists, since §4(d) applies only if one does.
2. **ring version.** The "Apache-2.0 AND ISC" expression matches recent ring 0.17.x. Older releases carried OpenSSL/SSLeay-licensed files. Pin a version and confirm that no such files remain in the resolved ring release.
3. **Feature unification beyond the crypto issue:**
   - egui/epaint `default_fonts` could be re-enabled by another dependent, so verify its absence in the actual tree, not just eframe's flags.
   - accesskit's Linux backend and ksni also use zbus. Check whether zbus ends up with both the async-io and tokio backends; the "Tokio backend" claim depends on this.
4. **Fonts.** Subsetting Noto Sans to reduce size creates an OFL "Modified Version". Noto currently declares no Reserved Font Name, but record that fact explicitly with the file hash. Ship the full OFL text alongside the font in the export, not only in the repository.
5. **NOTICE vs. the shipped binary.** The current ELF statically links Rust std and core components, which carry MIT/Apache and other notice obligations. The ADR says the export preserves them. The root `NOTICE` could point to that location so source and binary distributions stay consistent.
6. **Naming.** The ADR forbids redistributing MicroSIP brands, while the project and repository are named `rust-microsip`. The NOTICE disclaimer helps. Trademark and naming is outside copyright licensing, so this is not a blocker here, but record it as an open decision before any publication or visibility change.
7. **Unsupported specifics.** The "wgpu resolves to its compatible 30.x" claim and the "rsipstack requests Tokio full" claim should cite the exploratory metadata as their evidence source. Otherwise they read as asserted facts.
8. **Mutable URLs.** `source_repository` values containing `/tree/main/...` (eframe, secrecy) are mutable paths. The pinned `source_revision` is what matters, so this is acceptable as written.
9. **Internal consistency checks passed:**
   - The JSON has 16 entries, matching "Sixteen archives".
   - Each declared license expression matches its table row.
   - audio-codec has an empty notice map and a blocked status.
   - Shared notice hashes (serde/serde_json/thiserror; the secrecy/zeroize Apache file) are plausible for common upstream boilerplate.
   - No personal paths, email or credentials appear in the documents.
10. **Scope discipline is good.**
    - The documents correctly keep future gates as unimplemented: VM SONAME measurement (1.8), SBOM/vulnerability policy (9.3/9.9) and runtime/media proofs.
    - The current zero-external-crate `Cargo.lock` matches the claim that the catalog is not the lockfile.
    - The Apache-2.0 → GPLv3 direction, the GPL-2.0-only block and the Unlicense/OFL handling are stated correctly.

After B1 and B2 are addressed, I would expect this ADR to be acceptable as a pre-adoption engineering boundary. This review is not a legal certification.
