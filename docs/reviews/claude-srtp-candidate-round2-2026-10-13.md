# Claude SRTP candidate review, round 2 — 2026-10-13 UTC

Scope: final documentary clarification review for the pre-adoption evaluation milestone only. No secure-media/application-adoption/G0/release approval and no runtime or hash verification by the reviewer.

Same sanitized, tools-disabled neutral-directory CLI invocation as round 1. Terminal exit 0; JSON subtype `success`; `is_error=false`.

- Raw review JSON SHA-256: `45a46a3419e4f54762d6602e9dad5ebb4b95f4a1a8472e489cb187a971295310`.
- Sanitized prompt SHA-256: `e9ca5b0873d2ce6947869de13c7f0a44ae044493862aeb1470bcdb8e7f873de5`.

## Reviewer output

# Verdict: **APPROVE** for the pre-adoption evaluation milestone only

This approves only the round-2 documentary clarifications and the added evidence fields, as scoped. It does **not** approve:

- `rustrtc 0.4.7` for application use
- the future adapter
- SDES/SRTP negotiation
- Asterisk interoperability
- OpenSpec 1.7/5.4, G0, or any release gate

The replay acceptance test remains a real, unsatisfied requirement.

This is a static text review. I executed nothing and verified no hash against actual bytes. The only checks were internal consistency across the supplied documents.

## Round-2 items checked

**Debug wording is now accurate.** The excerpt shows `#[derive(Debug, Clone)]` on `SrtpKeyingMaterial` with `pub master_key: Vec<u8>` and `pub master_salt: Vec<u8>`. So derived `Debug` prints key and salt bytes. `SrtpContext` has a manual `Debug` that emits only `ssrc`, `_profile`, `direction` and `rollover_counter`. The ADR's distinction matches the source. It also correctly avoids claiming zeroization or erasure.

**Clone risk wording is accurate.** `SrtpContext` has `#[derive(Clone)]` and carries `rollover_counter`, `last_sequence` and its keys. A cloned sender can therefore protect two packets at the same index with the same keystream. The ADR states this as index/keystream reuse and ties it to the transmit-index-reuse requirement.

**Future requirements now cover the round-1 findings:**
- a specific typed replay error, not just any failure (finding 1);
- replay-window state that changes only after successful authentication (the forged-duplicate case);
- old and reordered packet tests;
- restriction to AES-CM/80 and proof that an unauthenticated or unsupported profile cannot enter the receive path (finding 2, the `Option<HmacSha1>` gate).

**Directory-only license scan is correctly scoped.** The JSON `notice_directory_layout_check` reports 100 archives and 0 omitted files. It limits the claim to this exact graph, and the receipt repeats that limit. The collector's basename rule (`parts[-1]...startswith(NOTICE_PREFIXES)`) is unchanged, so this caveat is necessary and present.

**Reproduction hash checks are consistent:**
- The application list has exactly 34 entries; I counted them. The candidate list has 35, adding only `crates/media/tests/srtp_candidate.rs`.
- The candidate entries differ from the application entries only in `Cargo.lock` (`d00490c1…`) and `crates/media/Cargo.toml` (`76e0677a…`).
- The probe hash `f652e540…` is identical in `candidate_copied_inputs_sha256` and `public_evidence_inputs_sha256`. So are the collector hash (`c2420006…`) and the lock hash.
- The script checks the source-name set against the receipt and rejects absolute paths, `..`, symlinks and non-files. It hashes every source and refuses a manifest that already mentions `rustrtc` or has more than one `[dev-dependencies]`. It then asserts the post-patch hashes of the lock, manifest and probe. This closes round-1 finding 6.

**The 95-not-98 correction is right.** `PROTECTED` has five entries. `version_check` enters through `generic-array`'s build script, and `rustrtc` is the candidate, so all five plausibly sit in the 100-package graph, leaving 95 unpinned. The caveat that lock/archive checksum agreement does not authenticate a modified lock is accurate.

**Task state is consistent.** All `gates` are `false`, `application_dependency_adopted` is `false`, and the receipt and ADR both say 4/54 with 1.7 and 5.4 open. Probe counts agree across the JSON, receipt and ADR: 4/1/0, exit 101, with the failing test named.

## Non-blocking nits

1. **"Independent" review label.** The receipt and JSON key (`independent_static_review`) call this review "independent". The reviewer, however, only sees implementer-sanitized excerpts, and you state that no independent proof is claimed. Consider "separate sanitized static review" to avoid overstating.
2. **No machine record of the reproduction run.** The receipt says the reproduction snippet was executed, but the JSON has no field for it, such as an exit code or log hash like those for `verify`/`context`. Add one if you want machine evidence to match the claim.
3. **Hash checks are self-consistency only.** They compare against hashes in the same checkout, which the receipt already frames as "implementer-recorded evidence". Keep that framing wherever the reproduction is cited.
4. **MKI wording.** "MKI handling offers adaptive behavior when configured" may understate it. The field comment says `rx_mki_active` is `None` until the first packet settles it, which reads as adaptive by default per SSRC. The adapter plan already rejects unsupported MKI explicitly, so this is wording only.

No concrete correctness, security or privacy issue blocks this documentation/evidence milestone.

## Final implementer checks

The final public-recipe copy was also compiled with fmt/clippy and executed in the same isolated, network-disabled toolchain, reproducing exactly 4 passing tests, 1 replay assertion failure, 0 ignored, exit 101. The machine receipt now records its log hash separately. The final context/scaffold-checksum rerun passed and is also hash-bound. Review labels distinguish separate sanitized static review from independent runtime/cryptographic attestation. Configured nonzero MKI fallback wording was made explicit. These evidence/wording updates do not change the reviewed collector/probe source or any acceptance gate.
