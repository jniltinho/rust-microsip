# ADR 0009: Isolated SRTP candidate evaluation, not direct-use approval

- Date: 2026-10-13 (UTC)
- Status: pre-adoption inventory verified; unadapted secure-media acceptance blocked
- Scope: OpenSpec 1.7 and AUD-05; no change to release criteria

## Decision

Evaluate the registry archive of `rustrtc =0.4.7` with
`default-features = false` and an **empty feature set**, in an isolated copy of
the workspace. Keep the application's manifests and `Cargo.lock` unchanged.
The pinned archive SHA-256 is
`7fc50a0fd78b4b7a61fab57d1e4ee915eb9a8d710c22333f231cf8fc79f3befe`;
its packaged VCS record identifies `b89dc2c4edf96b872b8114689fcc483dec318c58`.
The archive, not a moving upstream branch, is the evaluated source.

The [receipt](../receipts/g0-srtp-candidate-2026-10-13.md) records the exact
candidate lock, 100-package inventory, 195 retained notices, source/cache
comparison, and five-test diagnostic. This permits the recorded isolated
evaluation, **not application adoption, production approval, or G0 completion**.

## License and native boundary

The candidate adds 27 registry packages to the prior 73-package workspace
inventory. Their packaged notices provide an MIT licensing path; the complete
available MIT and Apache notices were retained, not replaced by declarations.
This follows [ADR 0003](0003-g0-license-and-native-boundaries.md), not an assertion
of legal or cryptographic certification. The
[ASF compatibility guidance](https://www.apache.org/licenses/GPL-compatibility.html)
also addresses Apache-2.0 incorporation into GPLv3 projects; upstream components
retain their own terms.

`version_check 0.9.5` has the legacy declaration `MIT/Apache-2.0`. Its verified
archive contains both complete `LICENSE-MIT` and `LICENSE-APACHE` texts. The
collector now permits that spelling only for this exact, source-pinned package
and only with both notices. It does not reinterpret arbitrary slash expressions.
Both this archive and `rustrtc` now have independent protected checksum pins in
the collector. A counterfeit archive plus a matching counterfeit lock checksum
is rejected before preserving notices.

The 27 added archives have no `links` declaration or files with the scanned
C/C++/header/assembly/shared-library suffixes. Three added build-script targets
were inspected: `crc32fast`, `generic-array`, and `serde`; they query the compiler
or emit Rust configuration/source, rather than compile/download a native crypto
provider. Rust intrinsics, inline assembly, and third-party `unsafe` still exist;
this is not a claim of memory-safety or constant-time proof. The existing ring
native boundary is unchanged. AES-GCM and both AES/CTR generations are still
selected transitively even though this evaluation calls only the AES-CM/80-bit
profile; they were not omitted from the license inventory.

The target-selected dependency deny checks pass. Cargo metadata's feature union
is not identical to the Linux tree's: metadata retains Tokio's `windows-sys`
feature while the Linux tree does not select it. The candidate inventory records
both `features` (metadata) and `target_tree_features` (actual target tree).
All 100 registry package identities agree between those views.

## Concrete direct-use blocker

The isolated probe passes encryption/roundtrip, exact authentication failures
for payload tampering and a wrong key, and authenticated reordering across a
sequence rollover. It **fails** the assertion that a second copy of the same
authenticated packet is rejected. This is a real execution result, not an
inference from a source search, and is intentionally kept red.

In the inspected archive's `src/srtp.rs`, `SrtpContext` authenticates and decrypts,
then advances its highest index; the evaluated receive path does not reject the
duplicate. Authentication is not replay protection.
[RFC 3711 §3.3.2](https://www.rfc-editor.org/rfc/rfc3711.html#section-3.3.2)
describes a bounded receive window of at least 64 packets, updated for accepted
authenticated packets. The current failing probe blocks direct use of this
context as a complete secure-media receiver.

Other adapter concerns from this source inspection remain open: session maps
can accumulate recently active SSRCs; `SrtpKeyingMaterial` derives Debug/Clone
over ordinary key/salt Vecs, while `SrtpContext`'s manual Debug omits keys.
Contexts remain cloneable: cloning a sender can enable index/keystream reuse,
not merely an extra allocation. The no-default build has injected RNG/clock
seams. With a nonzero inbound MKI length configured, the candidate tries that
length and then no-MKI until the first successful packet settles the mode.
No guarantee of
complete key erasure or compiler-enforced constant-time behavior was established.
These are not permissions to activate high-level peer/DTLS machinery.

## Packet-primitive follow-up

[ADR 0010](0010-isolated-srtp-packet-adapter.md) now records an own isolated
per-SSRC SRTP/SRTCP primitive with green replay/resource/ownership tests. The
unadapted provider diagnostic above remains red. No application dependency was
adopted, and secure signaling, call source/key lifecycles and PBX acceptance are
still required before approving the complete adapter.

## Next acceptance work

The planned narrow data-plane adapter needs fixed resource/SSRC limits, a bounded
replay window whose state changes only after successful authentication, rollover
and old/reordered-packet tests, transmit-index reuse prevention, secret-redacted
ownership, and explicit authenticated decryption errors. Future replay tests
need a specific typed replay error, not merely any failure. The adapter must
restrict the profile to AES-CM/80-bit authentication and demonstrate that an
unauthenticated/unsupported profile cannot enter its receive path.
Cryptographic transforms
and key derivation remain library operations; no copied or invented cipher/MAC.
Alternative versions/implementations need a new exact-source inventory and
interoperability evidence before incorporation.

The negotiated profile remains validated TLS plus SDES,
`RTP/SAVP` and `AES_CM_128_HMAC_SHA1_80`, never plaintext fallback. Source keys
need OS-backed randomness, separate send/receive ownership, and no SDP/key
logging. [RFC 4568 §6.2.1](https://www.rfc-editor.org/rfc/rfc4568.html#section-6.2.1)
specifies the 16-byte master key, 14-byte salt, and 80-bit tag for this suite.
The narrow initial negotiation can reject unsupported MKI/session parameters
explicitly rather than use the candidate's adaptive mode.

Real encrypted PCMA/PCMU media in both directions through the pinned
andrius/asterisk PBX, downgrade/invalid-crypto/key/authentication/replay negatives,
teardown, and leak scanning are still required. The four positive local tests
are self-roundtrips, **not independent cryptographic conformance or PBX evidence**.
OpenSpec 1.7 and 5.4, G0, and every release gate remain open.
