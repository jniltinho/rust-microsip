# Claude review — Rust G.711 and synthetic calls, round 1

Date: 2026-10-12. Baseline: `bfe9539`. Read-only Claude CLI in a neutral temporary directory, tools/MCP/hooks/session persistence disabled. Sanitized own source/configuration/docs and test summary only. Input SHA-256: `8867c65ca6ab18b91b1c9549b37a06acc0e9b6f43db49d785faf0ec68aa1de03`. CLI exit 0, subtype `success`, `is_error=false`. No tests executed by the reviewer.

Verdict: **REVISE — evidence blocker, no code-level blocker for the isolated sub-milestone**. No G0, task 1.6/5.1, production-media or release approval.

1. High: supply the missing media receipt/inventory and resolve the pending independent-observer Boolean before publication. Identify whether the observer measures RTP or only lifecycle/isolation. This is the sole publication blocker identified.
2. Medium: own call setup clones a plaintext credential into InviteOption; OwnedRegistration does not clear it. Document it as an adapter-originated production gate, not only an upstream limitation.
3. Low: loss/late counts are reported, not pass-gated; record the actual ≥240 sent/≥200 received threshold and distinguish observed zero loss from an enforced requirement.
4. Low: report true booleans summarize successful in-process checks, not independent measurements. State that distinction.
5. Low: generic address types do not establish IPv6 support; note it as untested or reject it explicitly.
6. Low: update lab wording for RTP-only positive call fixtures, untested routes/SRTP and remaining human/device acceptance.
7. Low: serde String-to-SecretString conversion can leave an old allocation uncleared; track the example's additional secret-lifetime gate.
8. Info: supplied G.711/RTP arithmetic/bounds, strict SDP/secure-account refusal, reverse-tone mapping and owned-task error cancellation have no identified blocker for this trusted fixture. Adversarial-network acceptance is not claimed. Supplied source confirms eight media/five telephony unit tests; other counts were accepted only from the summary.

Disposition: add the exact source/lock/image/ELF receipt and selected inventory; correct the observer's interpretation of Asterisk's bounded-call-limit CLI format using retained active/final evidence and positive/negative parser controls; clarify all above limits in ADR 0005 and the receipt. [Round 2](claude-rust-media-round-2.md) reviews these changes. No code behavior or acceptance scope is weakened. Human exact-artifact audio/history, SRTP, DTMF/hold/REFER, jitter/resampling/RTCP, desktop/IPC ELF, host/VM/NAT and CLI/MCP remain mandatory.
