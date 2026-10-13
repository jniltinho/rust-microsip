# Claude review — Rust registration, round 2

Date: 2026-10-12. Baseline: `8fb57a2`. Independent read-only Claude CLI in a neutral temporary directory, tools/MCP/hooks/session persistence disabled. Sanitized own sources/config/docs and exact test summary only. Input SHA-256: `17517c69c2054f106a8468f34f884be86ebc772fc1076a739fd83b02c4b0fda5`. CLI exit 0, result subtype `success`, `is_error=false`.

Verdict: **APPROVE — experimental registration sub-milestone only**. No concrete blocker remains for this scope. No approval of task 1.6, G0, production use, calls, media, SRTP or release. The reviewer read supplied evidence and executed no tests or hash/staging checks.

- B1 resolved: route-specific local binding, outgoing Contact inspection, independent UDP source comparison, internal-only PBX rewriting disabled and independently confirmed.
- B2 resolved: bounded nonempty stable Call-ID, nondecreasing observed CSeq, strictly advancing completed-phase checkpoints, bounded message counter and boolean-only output.
- B3 resolved conditionally: the receipt exists; its staging, links and identities must be verified by the implementing agent before commit, together with the inventory and privacy audit.

Residuals retained for subsequent hardening/G0 work:

1. TCP/TLS Contact is compared with the transport-reported address, not an independent accepted-socket peer address. The UDP test supplies that independent check only for UDP. Do not interpret `contact_verified` as incoming-call reachability. After review, rustdoc was clarified accordingly without changing behavior.
2. The receipt does not separately count authenticated retry callbacks; require observed requests to cover the initial three plus challenges before treating inspector evidence as exhaustive wire coverage.
3. WireEvidence's negative unit cases lack a separate positive/retransmission unit control; the actual PBX probe is the current positive control.
4. A poisoned inspector mutex skips that callback, although later checkpoint locking fails; a separate fail-closed flag would make this clearer.
5. The generic internal Compose checker covers isolation, not Rust image/proof-environment identity. Current image inspection, explicit overlay and runtime endpoint query support this run; add a dedicated profile regression test later.
6. Only eight PBX unit cases were supplied in `test_lab.py`; four additional existing tests are in the other fixture test modules. Actual container discovery ran twelve, not an inferred count from the single supplied file.

These are not waived release requirements. Parser/channel bounds, upstream credential-copy lifetime, renewal/recovery, calls/media/SRTP, licenses/security/native packaging, host/VM/NAT and exact-artifact human acceptance remain open. See the [receipt](../receipts/g0-rust-registration-2026-10-12.md). Commit conditions require matching source/lock/export hashes, staged inventory/receipt, no local artifacts/personal paths/secrets and task 1.6 left unchecked.
