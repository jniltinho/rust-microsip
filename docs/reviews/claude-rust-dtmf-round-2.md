# Claude review — negotiated DTMF/IVR, round 2

Date: 2026-10-12. Baseline: `ca43366`. Independent read-only Claude CLI in a neutral temporary directory; tools/MCP/hooks/session persistence disabled. Sanitized revised ADR/receipt/JSON, both observer sources, validated matrices/proofs and round-1 findings supplied. Input SHA-256: `32746ebdb84a6f024b6c61543c46b839cc968bddcc3f76666f647108a62df9d3`. CLI exit 0, subtype `success`, `is_error=false`.

Verdict: **APPROVE — isolated RFC 4733/IVR evidence sub-milestone only.** Not production, G0, full SIP-06, task 1.7 or release approval. The reviewer checked supplied text, not runtime, hashes, source equality or staging; those checks were executed separately by the implementing agent.

All four round-1 blockers resolved: correctly rounded extrema; exact 12-cell validated matrix/object and saved-file hash; running controls/PBX image and in-container ELF binding; honest distinction between positive digit recognition and negative-sequence non-acceptance. A final identity-bound rerun supplied the published matrix. Both original and corrected attempts remain ignored evidence, not conflated successes. Compiled sources, fixture behavior, lock and acceptance criteria stayed unchanged.

The current-source twelve-pair-call regression separately binds actual runner/PBX images and ELF, corroborates lifecycle/isolation/cleanup and records its matrix hash. Historical media evidence is not rewritten. Original Python registration regression is explicitly infrastructure-only, not Rust media or SRTP proof.

Non-blocking follow-up:

- Clarify that only some deterministic tone rows are identical; phase/onset/window alignment can produce small variations. Added after review as documentation only.
- Simplify the final packet count from 100–100 to 100 and list the additional ignored observer/matrix artifacts. Added after review as documentation only.
- The supplementary pair-call observer relies on the runner's exit/record count for per-call acceptance, has weaker count-format checks than the IVR observer, lacks per-subprocess timeouts and relies on its outer runner deadline plus Compose teardown. Its receipt claims only identity binding/lifecycle corroboration; tighten it before reuse. The implementing agent additionally checked all saved pair-call rows' packet/teardown fields before publication.

Existing parser/channel/secret-copy, per-dialog callback correlation, remote failure-path teardown and production media/randomization gates remain. Hold/resume/REFER/SRTP, human exact-artifact voice/devices/history, jitter/resampling/RTCP, host/VM/NAT/IPv6, complete-stack desktop/IPC ELF and CLI/MCP remain required. [Executed receipt](../receipts/g0-rust-dtmf-2026-10-12.md).
