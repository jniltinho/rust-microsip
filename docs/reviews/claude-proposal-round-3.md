# Claude CLI review — rust-microsip, CLI/MCP, web, round 3

Date: 2026-10-12. Sanitized planning only. English summary translation of the historical findings, not a new review.

# Verdict: REVISE

Desktop remains coherent; later web scope is well bounded. Blockers concern CLI/MCP authorization, idempotency/restarts, and headless ownership.

## Blockers

### B1. TTY approval is not outside agent control
Same-UID shell agents can invoke cli approve/grant and allocate ptys through script/expect. TTY does not prove human presence; claimed external authorization/AG-04 protection is false.

Minimum fix: owner-GUI operation approval; pre-agent-session service grant creation; honestly document unrestricted same-UID shell as user-equivalent/outside boundary; test pty self-approval attempts with GUI requirement or explicitly documented exclusion. Later revised design chooses GUI-only issuance with no cli approve/grant.

### B2. Idempotency namespace and pending approval are inconsistent
Including authenticated scope in the key allows different CLI/MCP grants to produce duplicate INVITEs despite AG-05. Approval bound to request_id conflicts with five-second request/60-second human deadlines and unspecified resending.

Minimum fix: global profile/epoch/idempotency_key; changed payload/scope returns conflict. Bind approval to key/payload fingerprint, not request_id. Specify/test either owner-pending intent executed on approval and operation_get, or same-key resending. Updated plan chooses pending execution/querying.

### B3. Restart/epoch semantics conflict with durable outcomes
Old-epoch rejection and blanket post-restart denial leave outcome_unknown/terminal outcome queries undefined despite durable ledger.

Minimum fix: old results are read-only, never execute; operation_get works across epochs for 24 hours with current read grant; distinct typed old-mutation error; explicit AG-05 case.

### B4. Headless ownership lacks URI/approval channel
Service has no window to confirm external URIs, a later GUI loses the same lock, and approval-required operations have no channel.

Minimum fix: visible invoker URI error/no dialing; specify later-GUI exit or read-only behavior without registration; service requires prior grant; align LIN-03/AG-01.

### B5. Durable DTMF hash leaks PINs
Short DTMF/PIN sequences can be trivially brute-forced from a plain stored hash, violating AG-05 despite no plaintext digits.

Minimum fix: random per-epoch memory-only HMAC key or no persisted DTMF fingerprint; include scanner test in task2.5.

### B6. Approval/gate order conflicts between changes
Desktop approves both plans before G0; earlier CLI task said approve after G0. AG gates were missing from G0–G5 order.

Minimum fix: approve both plans before G0, implement CLI/MCP after technical G0; place AG-01..06 explicitly before G4/G5 (G3b), align desktop9.10/CLI3.5.

## Nonblocking observations

**Scope/docs:** README must list requested CLI/MCP; AG-02 should say planned config/doctor, not existing; web proposal must include required DND.

**Ownership/IPC:** no-DISPLAY service still needs private XDG runtime/user audio/optional session D-Bus/keyring; document preparation/errors. Add owner-global request limit beyond per-connection limits. Default status masks remote number.

**Grants:** separate answer scope/incoming-origin policy, not outbound allowlist; GUI shows read scope/lifetime/quota before confirmation.

**Desktop:** reference extension production IPC-v1/SO_PEERCRED in design/task1.8; unavailable storage must block CLI/MCP mutation before effects/ledger writes.

**Web:** disable/inspect SIP.js console/Authorization/SDP logs; mandatory Origin-validating WSS reverse proxy; explicitly external temporary-TURN issuer and lab ticket delivery; no JS/WASM zeroization guarantee; expect/justify wasm-unsafe-eval in ADR.

**Gates:** Wine/Xvfb proves layout only; preserve this distinction. Desktop count 46 =6+9+4+4+6+7+10 is correct.
