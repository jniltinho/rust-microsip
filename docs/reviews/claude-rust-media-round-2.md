# Claude review — Rust G.711 and synthetic calls, round 2

Date: 2026-10-12. Baseline: `bfe9539`. Independent read-only Claude CLI in a neutral temporary directory, tools/MCP/hooks/session persistence disabled. Sanitized own sources/configuration/docs, exact receipt/aggregate evidence, selected inventory and test sources supplied. Input SHA-256: `478ab3b4629a697d13cfa0c9347bf6c6ed710f7f40f5db8494109398c4dc968d`. CLI exit 0, subtype `success`, `is_error=false`.

Verdict: **APPROVE — own G.711/RTP and short synthetic-call sub-milestone only**. No correctness/privacy/security/evidence blocker identified for this narrow scope. No G0, task 1.6/1.7/1.8/5.1, AUD-01, production use or release approval. The reviewer executed no tests, hashing or staging audit; it checked supplied text for internal consistency only.

Round-1 evidence blockers are resolved: the receipt/JSON identify source, lock, runner/PBX images and ELF consistently; failed observer interpretations are retained and corrected lifecycle/isolation corroboration is clearly distinguished from RTP-tone measurement. Supplied sources/counts agree: 10 CLI + 8 media + 5 telephony unit + 6 registration integration + 1 doctest, 73 registry packages / 143 notices and 27 individual build inputs.

Residual findings:

1. Medium, non-blocking: record the corrected saved-evidence verifier and raw active/final CLI evidence hashes. These identities were added after review to the receipt/JSON; ignored operational evidence itself is not published.
2. Low: include the base Compose and calls-overlay test source hashes. Both were added to the JSON after review, closing this identity omission without modifying source/runtime behavior.
3. Low: example read/parse buffers can retain earlier allocations in addition to the documented String/InviteOption copies. ADR 0005 and the receipt now explicitly track this broader secret-lifetime gate.
4. Low: lifecycle phases are endpoint-wide, not correlated to the exact dialog ID. BYE/200, remote BYE, inbound confirmed state and external zero-channel observations mitigate this fixture only. The receipt now distinguishes endpoint termination observations from per-dialog proof; production correlation remains required.
5. Low: rustfmt skips compact statements inside macro bodies. Format/refactor lifecycle/error paths before production reuse; a passing fmt command alone does not establish readability.
6. Info: bit-identical tone rows are expected for deterministic lossless sources; all media uses UDP, and changing SIP transports does not establish independent media transports. Clarified in the receipt.
7. Info: the documented prohibition on combining Rust overlays is not generally enforced for arbitrary manual Compose invocations. The current wrapper selects one exact overlay and its checker validates the calls identity; preserve/enforce this restriction when expanding profiles.
8. Info: write this review record in the same publication as the linked receipt, avoiding anticipatory/dangling evidence. This record is part of that commit.

Post-review changes above are documentation/evidence-identity clarifications only; no compiled code, lockfile, fixtures or acceptance criteria changed. Required gates remain: human five-minute exact-artifact voice/devices/history, DTMF/hold/REFER/SRTP, jitter/impairment/resampling/RTCP and production random initialization, parser/channel/secret hardening, authenticated error-path teardown, IPv6 and host/VM/NAT, full desktop/audio/D-Bus/IPC ELF, CLI/MCP, full license/SBOM/security/provenance audits. [Executed receipt](../receipts/g0-rust-media-2026-10-12.md).
