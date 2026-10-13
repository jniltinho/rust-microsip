# Claude review — Rust registration, round 1

Date: 2026-10-12. Baseline: `8fb57a2`. Independent Claude CLI, read-only, no tools/MCP/hooks/session persistence; sanitized own code/config/documentation and test summary only. Input SHA-256: `0ee7b94a96843af845af62e452604698e64fb982ffd0440845640c1e299fe3a7`. CLI exit 0, result subtype `success`, `is_error=false`.

Verdict: **REVISE**, isolated registration milestone only. Not G0/release or call approval.

- B1: correct outgoing Contact was not asserted; PBX rewriting could mask an invalid address. This was an explicitly labelled upstream inference, not a verified bug. A diagnostic socket test showed upstream already resolves wildcard UDP to a concrete loopback address (its transport serialization is uppercase); that did not prove every lab transport. Add route-specific binding, local-socket validation, outgoing Contact checks and rerun with PBX rewriting disabled.
- B2: three 200 responses alone did not establish refresh identity/ordering. Add bounded outgoing Call-ID/CSeq evidence, require one Call-ID and increasing CSeq across completed phases, and report booleans without publishing identifiers.
- B3, conditional: confirm the linked receipt exists and is committed. It was created after the review input snapshot; verify links/staging before commit.

Additional recommendations: include dev edges in the deny check; use a bound silent test socket rather than a possibly occupied localhost SIP port; exercise in-flight cancellation; document the runner build command; plant private sentinels under the new source/example directories; avoid release provenance claims for a lab image. Revisions address these without changing full task acceptance criteria.

Explicit production gates remain parser/channel bounds, credential-copy lifetime, scheduled renewal/recovery, all calls/media/SRTP, complete-stack packaging/native audit, vulnerabilities, VM/NAT and human exact-artifact acceptance. Static review is not runtime evidence. See the [executed receipt](../receipts/g0-rust-registration-2026-10-12.md) for final results and subsequent review.
