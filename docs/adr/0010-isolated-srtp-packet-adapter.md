# ADR 0010: Bounded per-SSRC SRTP/SRTCP primitive under evaluation

- Date: 2026-10-13 (UTC)
- Status: isolated packet tests verified; application/PBX acceptance pending
- Scope: implementation work toward OpenSpec 1.7/AUD-05, not narrower release criteria

## Implementation boundary

The [own candidate module](../../tests/probes/srtp_adapter.rs) wraps the exact
`rustrtc 0.4.7` no-default-feature data plane reviewed in
[ADR 0009](0009-srtp-candidate-evaluation-boundary.md). It lives outside the
application workspace and Docker build context. No application Cargo manifest,
lock, runtime capability, or call control was changed.

The isolated media manifest additionally uses exact `thiserror 2.0.21` and
`zeroize 1.9.1`, already selected in the previous inventory. The actual selected
graph has the same 100 registry identities, archives, features and 195 notice
hashes; the full source/cache comparison was repeated before compilation.
The [candidate lock](../inventory/g0-srtp-adapter.Cargo.lock) belongs only to the
isolated copy. It is not the application's lockfile or adoption approval.

All cipher/MAC/key-derivation work stays in the library. Own code adds packet
validation, resource/ownership boundaries and replay/index state, not new crypto.
Only AES-CM with the 80-bit authentication tag can be constructed; no profile
selector, unauthenticated mode, MKI setter or plaintext fallback is exposed.

## Ownership and state

- A sender owns one SSRC and a non-cloneable provider context. Valid RTP indices
  advance strictly; duplicates/backward indices are rejected before encryption.
  Forward gaps within the provider's index-estimation range are supported.
- SRTCP has a separate transmit counter. Reaching either key/index bound closes
  both transmit paths. An unexpected protect failure also closes the sender,
  including failures after the provider increments its SRTCP counter.
- The per-SSRC receiver learns its source only after successful authentication,
  decryption and structural validation. Failed initial sessions are discarded,
  rather than accumulating contexts for unauthenticated SSRCs. Once bound, other
  sources never enter this receiver's provider session.
- RTP and SRTCP have distinct, fixed 64-packet bitmaps. Proposed changes are
  committed only on success; old packets and duplicates have distinct typed
  errors. [RFC 3711 §§3.3–3.4](https://www.rfc-editor.org/rfc/rfc3711.html#section-3.3)
  describes the authentication/replay ordering and separate SRTCP index/list.
  Key/index overflow is rejected, not wrapped into nonce reuse.
- SRTCP requires the encrypted E-bit. Its clear compound data receives bounded
  structural validation, **not** semantic report decoding. Receiver code uses
  the provider's public Session API for SRTCP; empty, unusable local key material
  prevents accidental transmission from that private receive-only session.
- Clear datagrams are bounded to 1500 bytes, protected RTP to 1510 and protected
  RTCP to 1514. All copying/parsing operates within those bounds. This is a
  **worker-allocating** primitive, not a zero-allocation audio callback or an
  MTU/fragmentation guarantee. RTP padding is serialized canonically.
- Master material, sender and receiver are non-cloneable with redacted Debug.
  Typed errors have no input-bearing fields. Own master storage uses Zeroizing;
  provider/caller key copies and returned audio Vecs are not guaranteed erased.

## Evidence and limits

The [receipt](../receipts/g0-srtp-adapter-2026-10-13.md) records 29 adapter tests,
three non-Clone compile-fail doctests, three assertion-failing mutants, full
isolated-workspace regression checks, context exclusion and read-only Claude
review. The original unadapted provider diagnostic is kept red; it was not
ignored or inverted to obtain green tests. Successful self-roundtrips still do
not establish independent cryptographic conformance or PBX interoperability.

This object is a **per-SSRC building block**, not the complete secure-call owner.
Before application adoption, the integration must prove validated-TLS-only
SDES, independently generated source keys, transport-peer/payload checks and
rate limits before expensive first-packet authentication, and bounded
authenticated SSRC/key lifecycles for hold/transfer/re-INVITEs. RTCP-first
learning and distinct legitimate reporter/media sources belong to that routing.
Legitimate source changes must not simply lose audio; recreating a receiver with
the same key must not be used to erase replay history. In-place rekey and its
ROC continuity are not implemented here. These are required integration work,
not exceptions or post-release deferrals of secure call controls.

The constructor cannot prove global key uniqueness: recreating multiple senders
from identical raw bytes can still reuse keys. Key/session uniqueness belongs
to the supervised call owner. First-packet ROC is zero; joining an existing
SRTP stream with out-of-band ROC is not implemented. No complete key-erasure,
compiler constant-time, independent-vulnerability-audit or human-audio claim is
made. SDK internal behavior is version-specific and remains review-sensitive.

An [initial TLS/SDES call experiment](0011-isolated-initial-tls-sdes-calls.md) now
connects this primitive to the PBX, without adopting it in the application.
G0, tasks 1.7/5.4, secure-control/SRTCP coverage and every usable-release gate remain open.
