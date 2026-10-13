# Claude SRTP candidate review, round 1 — 2026-10-13 UTC

Scope: static pre-adoption evaluation milestone only; not candidate application adoption, runtime verification, secure-media acceptance, or release approval.

The CLI ran from a neutral temporary directory with safe mode, no tools, strict empty MCP configuration, no slash commands/session persistence, hooks disabled, and configuration sources disabled. Input contained sanitized own source/docs/evidence and exact licensed candidate excerpts, never personal configuration or production traffic. Terminal exit 0; JSON subtype `success`; `is_error=false`.

- Raw review JSON SHA-256: `1d8b95c660fe10369e4321e3670926be2d8bacc1874d5e18794d966a49d326f7`.
- Sanitized prompt SHA-256: `356a8432b2f58d6d1bdaae707d138ba09ee8fd58497fbcb7f6c6d96be7fada48`.

## Reviewer output

# Verdict: **APPROVE** for the pre-adoption evaluation milestone only

This approval covers the collector change, the context-isolation tooling, the validity of the red diagnostic, and the documentary claims as scoped. It does **not** approve `rustrtc 0.4.7` for application use, the future secure adapter (which does not exist yet), SDES/SRTP negotiation, Asterisk interoperability, G0, or any release gate. The replay failure is a real, unsatisfied acceptance condition. The four passing tests are self-roundtrips. They are not independent conformance evidence or PBX evidence.

This is a static review of the supplied text. I did not execute anything or verify any hash, checksum, or log.

## Verified by inspection

**Diagnostic validity**
- The panic location `srtp_candidate.rs:101:5` matches the `assert!` in `candidate_must_reject_duplicate_authenticated_packet`. The assertion uses `.is_err()`, so the red result means the duplicate returned `Ok`. A parse or compile failure would have surfaced elsewhere.
- The excerpt supports the cause:
  - `unprotect_with_mki` authenticates, decrypts, then calls `update()`.
  - `update()` only advances the highest index when `new_index > current_index`.
  - There is no replay list check, so an equal index is silently accepted.
- I traced the rollover test against `estimate_roc` and `update`. The sequence 65534 → 0 → 65535 yields ROC 0 → 1 → 0 with correct state, so the passing result is meaningful.
- The tamper and wrong-key tests check `AuthenticationFailed` exactly. The tamper test also confirms that the failed attempt does not poison the valid packet.

**Collector**
- Protected pins are enforced by name. Any other version of a protected name is rejected, as is a protected crate whose lock checksum differs.
- Both counterfeit tests raise before any archive read or output creation.
- Archive handling is sound:
  - The SHA-256 of the whole `.crate` is compared to the lock checksum, which is the correct Cargo semantics.
  - Streaming tar is read without extraction.
  - The prefix is enforced, and empty, `.`, `..`, and absolute paths are rejected.
  - Symlinks and hardlinks are rejected; non-regular members are skipped.
  - Member count and size are bounded, and output is written with `open("xb")`.
- The legacy `MIT/Apache-2.0` exception is limited to the exact `(version_check, 0.9.5)` identity with both top-level notice names. That identity is also checksum-pinned.

**Tooling and documents**
- `check-context.sh`:
  - It copies exactly 34 inputs, matching `application_build_input_count`.
  - The candidate set is 35, adding `crates/media/tests/srtp_candidate.rs` with the same hash as the probe.
  - The negative control requires the specific `CONTEXT_REJECTED` sentinels, including the probe.
  - All synthetic private files are removed before the artifact build.
- The scan totals match the ADR:
  - 27 added packages.
  - 3 build scripts (`crc32fast`, `generic-array`, `serde`).
  - No `links` or native files.
  - AES-GCM and both AES/CTR generations present.
- The RFC 3711 §3.3.2 (replay window ≥ 64) and RFC 4568 §6.2.1 (16-byte key, 14-byte salt, 80-bit tag) citations are correctly characterized.
- The JSON gates are all `false`, and the receipt's "4/54" and open-task statements are consistent.

## Non-blocking findings (fix before the adapter milestone)

1. **The replay assertion is too permissive for future acceptance.** `.is_err()` would pass on any error, including an unrelated parse or auth failure. When the adapter lands, assert a specific replay error variant. Also add tests for:
   - packets older than the window;
   - in-window unseen packets;
   - a forged duplicate, which must not mutate replay state.
2. **The authentication bypass depends on the profile.** In the excerpt, the MAC check runs only `if let Some(proto) = self.rtp_auth_prototype`. The adapter should refuse to operate unless authentication is present for `AES_CM_128_HMAC_SHA1_80`. Add this to ADR 0009's open concerns.
3. **Cloning carries a keystream-reuse risk.** `#[derive(Clone)]` on a sender context allows two contexts to protect with the same index. The ADR lists "contexts are cloneable"; it should say explicitly that this is a two-time-pad hazard behind the transmit-index-reuse requirement.
4. **The ADR's Debug wording is imprecise.** `SrtpContext`'s manual `Debug` omits keys, as the excerpt shows. The real issue is plain `Vec<u8>` key ownership with no zeroization. Debug exposure through `SrtpKeyingMaterial` was not shown either way. Reword the claim so it does not overstate.
5. **Notice discovery checks only the filename.** Files under a REUSE-style `LICENSES/` directory, such as `LICENSES/MIT.txt`, would be missed. The "complete notices" claim is fine for the current graph only if no package uses that layout. Consider also matching on directory components.
6. **The reproduction script does not verify the patched manifest.** It inserts a `[dependencies]` header and never checks the result. If `crates/media/Cargo.toml` already had a `[dependencies]` table, the output would be invalid TOML. Assert that the resulting SHA-256 equals the recorded `76e0677a…`, and assert that the copied lock equals `d00490c1…`.
7. **A failed collector run leaves partial output.** A failure partway through `collect` leaves some notice files in the new output directory. The error message already denies publication, but deleting the directory on failure would be cleaner.
8. **Lock trust is uniform except for the pins.** For the 98 unpinned packages, a counterfeit archive paired with a matching counterfeit lock entry would still be accepted. The ADR scopes its counterfeit claim to the two pinned packages, which is accurate. Keep that scoping in any future wording.

## Implementer follow-up

- Kept the failed replay acceptance test red and outside the shipping workspace/context.
- Clarified the distinct key-material and context Debug behavior, sender-clone/index reuse hazard, and future typed replay/authentication requirements in ADR 0009.
- Scanned all 100 exact archives for directory-only license layouts omitted by basename discovery: zero such files found in this graph. No general claim for future packages.
- Added recorded input/manifest/lock/probe hash checks to the reproduction snippet and executed that snippet independently of the original preparation.
- Collector failures still deny publication; partial local output is not accepted/exported. General atomic cleanup and broader future notice-layout support remain tooling improvements, not permissions to adopt unreviewed packages.
- Count correction to finding 8: the actual 100-package graph contains **five** protected pins (rsipstack, rustls, ring, rustrtc, version_check), leaving **95**, not 98, without independent collector pins. All 100 still undergo lock/source/archive checksum checks. Those checks do not independently authenticate an arbitrary modified lock.
