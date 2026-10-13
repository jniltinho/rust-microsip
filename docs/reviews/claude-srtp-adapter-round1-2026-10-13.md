# Claude SRTP packet-adapter review, round 1 — 2026-10-13 UTC

Scope: isolated per-SSRC packet-adapter evaluation only; no application adoption, TLS/SDES/PBX interoperability, G0 or release acceptance. The CLI ran from a neutral temporary directory in safe mode with tools/hooks/slash commands/session persistence disabled, empty MCP configuration and disabled configuration sources. Input was sanitized own code and exact licensed SDK excerpts, never personal data or production traffic. Static review, not runtime/hash verification. Terminal exit 0; subtype `success`; `is_error=false`.

- Raw JSON SHA-256: `dae3e0dd755474c0704cc56f0dfe9cdc877dec8f0a8187d70cb6daea8fd41785`.
- Prompt SHA-256: `620a4ab26dd3c01794294e6193b6284dddeb32661b71a320f7f11f2226ac07d6`.

## Reviewer output

# Verdict: **APPROVE** for the isolated packet-adapter evaluation scope

I found no concrete correctness, security, or privacy blocker in the adapter as submitted, judged against the excerpts provided. This approval does not cover application adoption, TLS/SDES/PBX interoperability, G0, or release.

## Targeted cases checked

| Case | Finding |
|---|---|
| **Receive ROC consistency** | `rtp_index` uses the same `±32768` thresholds as the provider's `estimate_roc`, from the same state: the highest accepted index. The provider's `update` advances only when `new_index > current_index`, which matches `ReplayWindow`. The adapter's checked arithmetic rejects (`TooOld`/`KeyExpired`) the cases where the provider would wrap at roc 0 or `u32::MAX`, so the provider never sees them. |
| **Replay windows** | `propose` runs before provider use and is committed only on full success. A forged duplicate is rejected before the provider runs, so it cannot clear a seen bit. The provider has no RTP or RTCP replay check of its own, and the adapter supplies both. The seeded set-model test covers the bitmap. |
| **No state divergence after authentication** | Every post-authentication failure in the RTP closure maps to `Provider`, which closes the receiver. The provider's padding error fires before `update`. An authenticated but malformed RTCP leaves the provider's `rtcp_index` advanced, but that index is unused for replay or decryption. |
| **Source learning** | `ssrc` is set only in the `Ok` arm. An unbound failed session is dropped, so the provider's `rx_contexts` holds at most one entry, well below its eviction watermark. Other SSRCs are rejected by `check_source` before any provider call. |
| **SRTCP** | It has its own 64-entry window. The E-bit is checked before authentication and the packet is rejected if it is clear. The index is read from the authenticated trailer, and the offsets are safe given the `≥ 22` length floor. |
| **Transmit index reuse** | RTP requires the index to be strictly greater than `highest` before the provider runs. RTCP's `control_count` moves in lockstep with the provider's pre-incremented `rtcp_index`, starting at 1. Exhaustion at `2^48-1` or `2^31-1` closes both send paths before any reuse. |
| **Provider failure** | `take()` without restore closes the sender on any protect error, including the RTCP path where the provider increments before failing. |
| **Bounds and panics** | `validate_control` cannot index out of range: `length ≥ 4` and `≤ remaining`. Every `source_at` call is guarded by a length check. `control_count += 1` cannot overflow because of the check before it. |
| **Redaction** | Debug output for the key, sender, and receiver is redacted. Error variants carry no input. `ReplayWindow` exposes only indices. |

## Non-blocking notes

1. **Sender-side provider ROC is not in the excerpt.** The adapter's no-reuse guarantee assumes the provider sender's `(ROC, seq)` follows the adapter's index. A cheap way to confirm this: add a test with a forward gap across the wrap, such as seq 40000 → 5000 (roc 0 → 1), and decrypt it with an independent `SecureReceiver`. The receiver's estimator matches the adapter's, so a successful roundtrip proves agreement.
2. **Oversize check on transmit.** The `MAX_PLAINTEXT + 1` sender case uses all-zero bytes, which fail the version check first. A valid-header 1501-byte packet would show that the size limit is actually enforced in `crate::RtpPacket::parse`.
3. **Header-field tamper coverage.** The provider authenticates a re-marshaled header, not the wire bytes. The tests tamper only sequence, timestamp, payload, and tag. Adding M/PT/CC/CSRC/extension flips would document that parse→marshal is lossless. If it weren't, the output is the canonical form anyway, so this is malleability without semantic effect.
4. **Accepted residuals:**
   - Provider key copies, including one per failed unbound attempt via `receiver_session`, are not erased.
   - Returned plaintext `Vec`s are not zeroized.
   - An authenticated peer can permanently close the receiver by sending a packet that fails the post-checks. This is fail-closed and limited to a key holder.
   - The first-packet ROC is assumed to be 0.

   All of these are within the stated limitations.

## Implementer follow-up

Added and executed the suggested forward-wrap gap, valid-header oversize, and header-field tampering tests; also verified authenticated malformed SRTCP after binding. Final adapter count is 29, up from the reviewed 26. A second receiver using the same provider is cross-object agreement, not independent cryptographic conformance. Per-SSRC objects are building blocks: bounded call source/key transitions, replay continuity, signaling and PBX evidence are still mandatory before adoption.
