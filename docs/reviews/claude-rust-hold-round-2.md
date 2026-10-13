# Claude review — synthetic hold/resume, round 2

Date: 2026-10-12. Baseline: `ac9248d`. Independent read-only Claude CLI, neutral temporary directory, tools/MCP/hooks/session persistence disabled. Sanitized revised own code/ADR/receipt/JSON/observer and round-1 findings supplied. Input SHA-256: `c0511775eeeb6ecac91792e66eb6a229021e592818c8e807cc636844ccd1c084`. CLI exit 0, subtype `success`, `is_error=false`.

Verdict: **REVISE — documentation only.** Both round-1 code blockers are resolved; no new code correctness/security/privacy blocker was identified. Static review did not execute tests, verify hashes or approve production, full SIP-06, task 1.7, G0 or release.

- The holder-reception/active-peer gate and matching observer check are accepted. A post-settling valid frame counts immediately; wrong-source or invalid RTP fails rather than disappearing.
- The actual verifier readiness handshake, consumption of the initial permit, malformed-ACK wake/error, matching ACK/CSeq and mismatched response Call-ID tests are accepted. Removing the malformed-ACK notification demonstrably produces the expected timeout in the separately executed negative control.
- Stable hold/resume targets, direction/version checks, preserved audio after 488, bounded operation, explicit emitted-ACK naming and declared production gates remain acceptable for this slice.

Required documentation corrections: final voice windows have **100–101**, not uniformly 100, emitted/accepted frames; held peer transmission has **50–51**, not uniformly 51, frames. The implementing agent independently recalculated the final ratio maximum as **1768.26** (the reviewer did not identify this third stale extremum). Receipt prose now derives all ranges from the final validated matrices, preventing reuse of earlier-run extrema. No source, image, runtime result or acceptance gate changed.

Non-blocking clarifications: the peer ≥40-frame gate covers the whole one-second held window, including settling; clarify that wording. Additional negatives for the two pre-existing local-send/amplitude guards remain a future test improvement; the new PBX-reception/peer-activity negatives already pass. No claim of exhaustive interleaving/branch coverage is made.

The follow-up is [round 3](claude-rust-hold-round-3.md); the [receipt](../receipts/g0-rust-hold-2026-10-12.md) is experimental evidence only. The review process completed normally; a later attempted termination found that its process had already exited and did not interrupt it.
