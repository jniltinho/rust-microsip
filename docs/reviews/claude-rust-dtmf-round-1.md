# Claude review — negotiated DTMF/IVR, round 1

Date: 2026-10-12. Baseline: `ca43366`. Independent read-only Claude CLI in a neutral temporary directory; tools/MCP/hooks/session persistence disabled. Sanitized own sources, tests, configuration, receipt/aggregate evidence and ignored observer supplied. Input SHA-256: `f54e2b833e3d93487f700f09797418a15376240a2f5d35bf7c3079e5ed81f743`. CLI exit 0, subtype `success`, `is_error=false`. No tests, hashes or staging audit performed by the reviewer.

Verdict: **REVISE — evidence/documentation blockers; no identified code blocker for this isolated milestone.** Not production, G0, full SIP-06, task 1.7 or release approval.

1. Correct the RMS maximum's rounding: 5103.394 rounds to 5103.39, not 5103.40.
2. Hash-bind the published IVR object to the exact sanitized matrix validated by the observer. Explain identical steady-state tone values instead of leaving ambiguous copy-edit evidence.
3. Bind the running container's actual image ID (and ELF) to the receipt; running a mutable local tag alone does not prove identity.
4. Clarify the negative IVR case: 400 Hz also follows absent input/timeouts. Positive `123` proves recognized RFC 4733 digits (only silence is sent as audio); `124` demonstrates non-acceptance, not independent recognition of its final `4`.

Disposition: correct/clarify documentation, add the validated matrix hash/object equality, and rerun an observer that compares actual running controls/PBX image IDs and the controls ELF. Current-source pair-call regression claims must likewise bind actual runtime identity; registration-only regression is separately identified. [Round 2](claude-rust-dtmf-round-2.md) reviews the resolved evidence. No compiled behavior, lockfile or acceptance criterion is changed by these fixes.

Non-blocking residuals: fixed synthetic RTP initialization, one offered dynamic event payload (101; tests use 111), event parsing relies on the earlier strict single-stream SDP validator, reused LabEndpoint auto-answers one unsolicited INVITE, public probe registrar IP is caller-provided despite its fixed dialed URI, event-loopback tests do not wrap inside the event, no RTCP/continuous send during branch measurement, one PBX implementation, plaintext UDP media only. Existing parser/channel/secret-copy, per-dialog callback correlation and authenticated failure-path teardown gates remain. Production reuse must address these, not infer approval from trusted-lab tests.
