# Claude static secure-call review, round 1

Date: 2026-10-13. This is sanitized, tools-disabled static review, not runtime, G0 or release approval.

- Terminal exit: 0; CLI subtype: `success`; is_error: `false`.
- Prompt SHA-256: `e8ba910c032ab6862ffc25d47b4fa7a97e81bbbb595b472c73a53a3ad230ec1f`.
- Raw JSON SHA-256: `b76541238c67501d6259def64d0fed6111fb84eda532e3090f9ddc61e406ae38`.
- No write/browser/MCP tools, hooks, slash commands, auto-memory or session persistence were enabled. The CLI ran in a neutral temporary directory.

## Verbatim result

BLOCK

This is a static review of the stated narrow claims only. It is not runtime, G0 or release approval, and nothing here waives the gates you listed (SRTCP, rekey/ROC continuity, hold/transfer, actor adoption).

## Blockers

**1. `prepare_secure_call.py` does not enforce the provenance it claims.**
- It reads `application_build_inputs_sha256` but only iterates the keys. No copied file is checked against its reviewed hash. The candidate gets whatever is on disk at copy time, not the reviewed inputs.
- The four probe sources (`srtp_adapter.rs`, `sdes.rs`, `secure_call.rs`, `secure_call_example.rs`) have no hash pin at all.
- Receipt paths are not validated. An absolute path or one containing `..` resolves outside `root` or `work`, because `Path('/x') / '/etc/y'` is `/etc/y`. `is_symlink()` checks only the last component, not parent directories.
- The final message prints a fixed "34 explicit application inputs and 4 explicit probe inputs". It does not count what was actually copied.
- The `Cargo.lock` copy says metadata will "update only own nodes", but nothing here checks that. There is no `--locked`/`--offline` build and no post-resolution comparison against the 100-package/195-notice inventory.
- The `[dependencies]` replace in the telephony manifest silently does nothing if that header is missing.
- Fix:
  - Verify the SHA-256 of every input, including the probe files, and fail on any mismatch.
  - Reject absolute paths, `..`, and symlinked parent directories.
  - Assert the input count instead of printing a constant.
  - Assert the manifest edits actually applied.
  - Build with `--locked --offline` and diff the resolved graph against the inventory.

**2. The TLS negative oracle cannot support `"rejected_before_call": true`.**
- `secure_call_example.rs` treats any `Err(CallProbeError::Signaling)` as TLS rejection. In `EndpointOwner::connect`, lookup failure, the TLS/remote-address mismatch, contact parse failure, `incoming_transactions()` failure and a non-200 REGISTER all map to `Signaling`. Later INVITE/BYE failures do too.
- Whether rsipstack 0.7.3 honors `ca_certs` and `sni_hostname` for client verification, rather than a permissive verifier, cannot be seen from this source. These two negatives are therefore the only evidence for the claimed CA/SNI validation, and they currently pass on unrelated failures.
- Fix: give the failure in `transport.lookup(...)` its own error, e.g. a `TlsRejected` variant or a stage marker. Require that exact error in both negatives, and check that no REGISTER was sent. Emit `rejected_before_call` only from that verified condition.

## Verified against the narrow claims (no blocker found)

- **SDES parser** (`sdes.rs`):
  - Rejects AVP, a missing crypto line, multiple or session-level crypto lines, lifetime/MKI/session parameters, other suites, and non-canonical or wrong-length keys (40 chars decoding to exactly 30 bytes).
  - Rejects a second `m=`, `c=` addresses other than the PBX, IP family mismatches, non-sendrecv directions, and DTLS/ICE/`rtcp`/`k=`/key-mgmt lines.
  - Enforces size, line-count and printable-ASCII bounds.
  - The answer tag must equal 1, and the incoming offer is restricted to tag 1 as you stated.
- **Key use** (`secure_call.rs`):
  - `Admission::claim` (an atomic swap) limits each endpoint's single local key to one offer or one answer.
  - `local_key.take()` moves each key into exactly one `SecureSender`.
  - Each probe creates fresh endpoints, so keys are not reused across calls. Both directions use independent keys, so there is no IV/keystream reuse.
- **Media direction mapping:** the caller decrypts with the key from the PBX's answer, and the callee decrypts with the key from the PBX's offer. This is correct SDES semantics.
- **Media checks:**
  - Media comes only from the SDP-bound PBX address.
  - Exact 182-byte packets are required.
  - Outgoing ciphertext must differ from the clear packet, with exactly a 10-byte overhead.
  - Packets with the wrong key, a tampered tag or sequence, the clear packet, or a replay all fail with the specific errors expected.
  - The sequence starts at 65500 and wraps, which exercises the ROC wrap within one call.
- **Transport:**
  - Only a TLS client connection is installed, and its remote address is checked.
  - Incoming transactions are dropped unless they arrive on that TLS connection from the PBX.
  - No plaintext path exists.
- **Lifetimes:** the 30 s timeout and cancellation drop the owners, which aborts their tasks.
- **`srtp_adapter.rs`:** the replay window is committed only after authentication, unbound sessions are not retained, and sender index reuse and lifetime limits fail closed.
- **Output:** numeric aggregates only, with no SIP, SDP or sample logging.

## Non-blocking notes (fix or reword)

- **Zeroization wording:** `LocalKey::description` uses `format!`, and `NegativeOffer::UnsupportedSuite` uses `str::replace`. Both grow a `String` through reallocation, which leaves freed, unzeroized copies of the base64 key. Writing into a pre-sized `Zeroizing<String>` with `write!` avoids this. Otherwise, reword the claim as "best-effort".
- **Unzeroized credential copies:** the example's intermediate `BTreeMap<String, String>` is not zeroized. These are your own copies, not SDK copies.
- **Unchecked hard-coded fields:**
  - `CallWire` marks every ACK, BYE and BYE-200 without binding to the dialog. That is adequate only because there is one dialog per endpoint.
  - `ack_and_termination_verified`, `bye_200_verified` and `distinct_signaling_and_media_sockets` are constants, not checked results.
- **`private_file`:** it checks that each file has the same owner as `/lab`, but not that `/lab` is owned by the effective UID.

## Disposition

BLOCK was preserved. Input path/hash/probe pins and the exact typed TLS-certificate negative oracle were corrected before round 2. Generic signaling errors no longer count as certificate rejection. Own allocation/key-erasure limits are explicitly documented.
