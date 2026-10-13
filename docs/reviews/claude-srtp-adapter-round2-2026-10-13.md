# Claude SRTP packet-adapter review, round 2 — 2026-10-13 UTC

Scope: isolated per-SSRC packet-primitive evaluation only. Same sanitized, neutral-directory, tools/hooks/MCP-disabled CLI invocation as round 1. Terminal exit 0; subtype `success`; `is_error=false`. Static text review, not runtime/hash verification or application/secure-call/PBX/G0/release approval.

- Raw JSON SHA-256: `986be189630fee60d8f3fe39fc512e9c4a402287935470b6758161f0d6b4aa0c`.
- Prompt SHA-256: `49157b6c358af60e267c12e2e2375d63974ba63c59a10d78d8cec52a55a53a1d`.

## Reviewer output

# Verdict: **APPROVE**, for isolated per-SSRC packet-primitive evaluation only

I found no concrete code, security, privacy, or documentation blocker in the submitted round-2 material. This was a static review of the supplied text only. I did not run any tests, check any hashes, or inspect provider source beyond what round 1 established.

This approval does **not** cover:
- application adoption
- TLS/SDES negotiation
- the secure-call owner, or SSRC/key lifecycles for hold, transfer, or re-INVITE
- ROC-continuous rekey
- PBX interoperability
- G0, tasks 1.7/5.4, or release

As the ADR and receipt state, those remain mandatory pre-adoption work, not exceptions.

## Round-1 follow-ups verified in code

| Follow-up | Finding |
|---|---|
| Forward gap across the wrap | `sender_and_receiver_agree_on_forward_gap_across_wrap`: 40000 → 5000 has a delta below −32768, so the ROC becomes 1. The test asserts that sender and receiver agree on a highest index of 65536+5000. This is a cross-object agreement check through the same provider, as the receipt correctly says. |
| Valid-header oversize | The 1501-byte packet keeps a valid RTP header (`0x80`), so the rejection is size-driven. The receive-side 1511-byte case also leaves the receiver unbound. |
| Header-field authentication | The test flips P, X, CC, M, PT, the CSRC and the extension. Every flip yields `Authentication` with the window unchanged, and the original packet then roundtrips exactly. |
| Authenticated malformed SRTCP | The packet is genuinely provider-protected with a bad length, and is checked both before and after binding. It produces `InvalidPacket`, with no binding, no replay commit, and the session retained. A valid next packet then succeeds, so the provider's unused advanced `rtcp_index` does not desynchronize anything. |

## Rechecked invariants

**Sender lifetime gates**
- Index `2^48−2` with seq 0 needs ROC `2^32`. That fails the `<= u32::MAX` filter, so the sender returns `KeyExpired` and closes.
- An exact `MAX_RTP_INDEX`, or `control_count == 2^31−1`, closes both send paths before the provider is called.
- Because of the pre-check, `control_count += 1` cannot overflow.

**Receiver**
- Replay is proposed before the provider runs and committed only on full success.
- An unbound failed session is dropped; the material is released only on success.
- Every closure failure after authentication either maps to `Provider`, which closes the receiver, or is RTCP structural, which leaves replay state untouched.

**`validate_control`**
- Each block's length is at least 4 and no larger than the remaining bytes.
- Padding is allowed only in the final block, must be 1 or more, and must not exceed `length−4`. No out-of-bounds index is possible.
- The 62×24+12 = 1500-byte maximum test is arithmetically correct.

**Seeded bitmap model**
- `cursor ≥ base+130` prevents subtraction underflow.
- The `MAX_RTP_INDEX−20000` base drifts slightly past the 48-bit space. That is harmless, because `ReplayWindow` uses plain `u64` with no key-bound semantics, and the test does not claim otherwise.

**Docs and receipt consistency**
- There are 34 application inputs, plus the adapter, giving 35.
- Exactly three candidate hashes differ: the lock, the media manifest, and the media `lib.rs`.
- The adapter hash matches the public evidence, and the lock hash matches the inventory lock.
- The mutation baseline hash equals the final source hash.
- The test summary counts (10/29/12/28/6, plus 3 compile-fail doctests) match the receipt.
- `check-context.sh` requires both probe sentinels in the negative control and removes `tests/` before building artifacts.

## Non-blocking notes

1. **Error documentation.** The `unprotect_rtp` and `unprotect_rtcp` docs do not list `Closed` (after a terminal provider failure) or `KeyExpired` (from receive-side ROC overflow). A one-line addition would make the typed-error contract complete.
2. **Unbound-phase cost.** Before binding, each forged packet causes a fresh `SrtpSession::new`, meaning a key derivation plus an un-erased provider key copy. Memory stays bounded and this residual was already accepted in round 1. Integration-level rate limiting and transport-peer filtering should be listed with the existing caller obligations.
3. **Recording round 2.** When this review is recorded:
   - add a round-2 entry to `separate_static_reviews` in the machine receipt
   - make sure the round-2 link in the Markdown receipt resolves
   - consider adding the review files to `public_evidence_inputs_sha256`, so the review record is hash-bound like the source

   Until then, the receipt correctly describes round 2 as pending.
4. **RTCP-first binding.** The receiver can bind its source from an authenticated SRTCP packet before any RTP arrives. That is acceptable for one remote source. The call owner should confirm that the peer's RTCP reporter SSRC equals its RTP SSRC, which is part of the SSRC-lifecycle work already marked mandatory.

## Final implementer follow-up

- Completed error rustdoc for Closed/provider failure and receive ROC exhaustion; no executable logic changed. All 29 tests, three non-Clone doctests, exact assertion-failing mutants and full workspace checks were rerun on the final commented source.
- Added pre-authentication rate limits and RTCP-first/distinct reporter source routing to the mandatory call-owner integration work, not product-scope exceptions.
- Bound both published review records, final reproduction and context evidence in the machine receipt.
- Correction to the incidental seeded-model observation: executing the exact 10,000-step wrapping generator produces a largest relative index of **10,025**, leaving **9,975** below the 48-bit maximum for the highest chosen base. It does not drift past that maximum for this seed. The bitmap model is still generic u64 bookkeeping; cryptographic lifetime gates are tested separately.
