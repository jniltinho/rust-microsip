# Claude CLI review — corrected rust-microsip planning, round 4

Date: 2026-10-12. Sanitized planning only. English summary translation of historical findings, not new runtime evidence.

# Verdict: APPROVE

All six round-3 blockers are resolved; no concrete blocker remains. Planning only: G0–G5/GW0 still pending.

## Blocker verification

| Finding | Status | Evidence |
|---|---|---|
| B1 TTY/pty does not prove human | Resolved | GUI-only approvals/grants, no cli approve/grant/TTY fallback; single-use atomically consumed GUI bootstrap; unrestricted same-UID shell outside boundary; AG-04/task2.2 test self-approval |
| B2 Namespace/pending approval | Resolved | Global profile/epoch/key independent of client/grant; changed payload/scope conflict; profile/epoch/key/payload-HMAC binding, not request_id; awaiting_approval/operation_get without resending |
| B3 Cross-epoch ledger | Resolved | stale_epoch mutations without effects; old operation_get for 24h with current read grant then result_expired, never new intent; AG-05/task2.3 |
| B4 URI/GUI against service | Resolved | Visible uri_confirmation_unavailable/no dial; later GUI engine_owned_by_service exit; grant_required instead of false channel; LIN-03/AG-01 aligned |
| B5 DTMF/PIN fingerprint | Resolved | Random memory-only per-epoch HMAC-SHA-256 key; pending digits memory-only; crash outcome_unknown/cancelled without digit restoration; task2.5 scanner |
| B6 Gate order | Resolved | Both plans approved before G0; spike IPC-v1/SO_PEERCRED; CLI after technical G0; G3b then G4/G5, desktop9.10 requires AG gates |

## Nonblocking observations

1. Explicitly expire prior-epoch awaiting_approval without effect/re-presentation after restart; add task2.3 test.
2. One-hour grants/single-use bootstrap require deliberate service stop/GUI/new bootstrap for renewal; document expiry error/threat model.
3. Human AG-06 in GUI/service needs explicit G4 host-only placement and separate mode/hash receipts, no inherited binary/mode approval.
4. Consumed grant-file ID should return bootstrap_consumed; service must not delete/rewrite it without instruction.
5. Web has no mandatory application backend but does require operator WSS proxy; replace ambiguous direct-PBX wording.
6. Web fixes are present: DND, disabled/inspected SIP.js/SDP/Authorization logs, proxy Origin validation, required external TURN issuer for relay, no zeroization claim.
7. Wine remains opening/layout evidence only; DIST-06 correctly rejects window/mocks/review as telephony proof.
