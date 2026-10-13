# Claude review — synthetic hold/resume, round 1

Date: 2026-10-12. Baseline: `ac9248d`. Independent read-only Claude CLI in a neutral temporary directory, with tools/MCP/hooks/session persistence disabled. Sanitized own adapter/example/shared endpoint, Compose checker/tests, builder, ADR/receipt/JSON and separate-process observer supplied. Input SHA-256: `f148405111885fde469bcf8da48587eba11d0206052e2293db7550dd454e903c`. CLI exit 0, subtype `success`, `is_error=false`.

Verdict: **REVISE — narrow experimental slice only.** Static review did not execute calls or verify hashes and does not approve production, full SIP-06, task 1.7, G0 or release.

Required findings:

- **R1 — meaningful negotiated-hold media gate:** local zero transmission and absent remote tone mostly restated the locally muted sender. Require zero PBX audio reception at the holder after settling while the opposite endpoint still transmits ≥40 frames. Mirror the gate in the separate-process observer; do not require zero peer reception, since music-on-hold can be legitimate. Added a red/green held-window gate test and strengthened the runtime checks; final execution is recorded in the receipt.
- **R2 — actual blocked verifier:** the initial notification test consumed stored permits, not a concurrently waiting `Wire::verify`. Replace it with a task/readiness handshake that reports only after verification polls Pending; malformed ACK must wake it with `Err(Signaling)`, matching ACK returns the exact CSeq, and a different response Call-ID fails. Removing the malformed-ACK notification in the ignored sandbox demonstrably fails on timeout; restored code passes.

Non-blocking recommendations addressed in the final revision:

- Check stable anchored target in the hold answer as well as resume; require recvonly (inactive answers remain a documented fail-closed limitation).
- Unit-test incoming answer directions/origin versions and incompatible codec rejection. Declare that anchored PBX runs do not prove execution of forwarded Rust re-INVITE/488 handling or dynamic media retargeting.
- Correct the RTP-clock comment: it advances in measured windows, not re-INVITE signaling gaps.
- Name safe output `ack_emitted_cseqs` to avoid implying remote ACK reception.
- Describe the implementing agent's ignored observer as separate-process, not independent third-party execution. Public lab commands do not run it. Running image/ELF binding is to pre-run snapshots, not an embedded source-manifest attestation; source→image attestation remains unimplemented.
- Retain the 360-second outer deadline: very slow cases/startup may hit the wrapper before all per-call deadlines. It is a fail-closed bound, not a completion-time guarantee.

Existing credential-copy/parser/channel/failure-cleanup, production-media, REFER/SRTP, desktop/IPC ELF and human exact-artifact gates remain unchanged. Follow-up disposition: [round 2](claude-rust-hold-round-2.md). Executed checks: [hold receipt](../receipts/g0-rust-hold-2026-10-12.md).
