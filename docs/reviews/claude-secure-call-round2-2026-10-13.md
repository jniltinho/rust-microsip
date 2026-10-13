# Claude static secure-call review, round 2

Date: 2026-10-13. This is sanitized, tools-disabled static review, not runtime, G0 or release approval.

- Terminal exit: 0; CLI subtype: `success`; is_error: `false`.
- Prompt SHA-256: `08033a8997fcaf57106dee86839eae2cf5714bb3fc4bcb3ac0b7c43b55a85e01`.
- Raw JSON SHA-256: `9a9b2de564b87c882af54fe22623ff2a43410de1f2400ecc13ca22ed70af5c33`.
- No write/browser/MCP tools, hooks, slash commands, auto-memory or session persistence were enabled. The CLI ran in a neutral temporary directory.

## Verbatim result

APPROVE

This approval covers the stated narrow static claims only. It is not runtime, G0, adoption or release approval. I found no concrete correctness, security or privacy blocker in these files. Both round-1 blockers are addressed:

- **Input pinning.** The 34 application inputs are checked against the receipt allowlist and hashes. The 4 probe sources and the candidate lock are checked against `secure_call_inputs.json`. Symlinked inputs and parents are rejected. A 38-entry manifest is emitted and re-verified before compiling. The registry graph check covers 100 packages, features, archives, extracted sources and 195 notices.
- **TLS negatives.** `probe_tls_certificate_rejection` builds only a `TransportLayer` and accepts only the typed rustls `UnknownIssuer` or `NotValidForName(Context)` errors. If rsipstack 0.7.3 stringifies or rewraps the error, or `lookup` succeeds without a handshake, the result is `None` or `Ok`. Either way the probe fails closed, so it cannot falsely pass. The positive call plus the wrong-hostname negative together show that verification uses `sni_hostname` and not the IP address.

Other checks against the stated claims:

- **SDES parser** (`tests/probes/sdes.rs`):
  - Size, line-count and line-length bounds hold, and only printable ASCII with CRLF framing is accepted.
  - Exactly one media-scoped crypto line and `RTP/SAVP` only.
  - The 40-character key length rejects lifetime, MKI and multiple key-params, and 30 bytes is exactly 40 base64 characters with no slack bits.
  - The field count rejects session parameters.
  - Address must equal the PBX, port must be in 10000–10019, the answer tag is bound, and ICE, DTLS, RTCP and non-sendrecv directions are rejected.
- **Key use.** Keys are directional: the local key is used for send, the PBX key for receive. Each local key goes to exactly one sender with a fixed SSRC across the sequence rollover, so I see no keystream reuse. `Admission` limits each key to one dialog.
- **Media checks.** Every packet is authenticated before decoding. Socket, size, payload type, length and the plaintext≠ciphertext check are enforced. The forged-tag, sequence-tamper, clear and wrong-key attempts run before the genuine unprotect and assert that no SSRC was bound, which also shows failures do not poison state. Replay is checked after.
- **Signaling.** Only the INVITE and 200 carry keys. They travel on the verified TLS connection, and incoming transactions are rejected unless they come from that TLS PBX connection.
- **Output.** Only aggregate numbers and labels are printed, and error strings are sanitized.
- **Teardown.** Success flags are set only after the ACK, BYE/200, termination and unregistration waits. The observer verifies hardening, the ELF hash, the internal network, no published ports, the SDES non-optimistic settings, the (2,1) to (0,0) channel counts, zero contacts, and cleanup.

Nonblocking hardening, in priority order:

1. **Assert-only verification.** `observe_secure_call.py` and `verify_secure_dependencies.py` rely entirely on `assert`. Under `python -O` or `PYTHONOPTIMIZE` they silently pass and write a proof with `True` values. Add `if not __debug__: raise SystemExit(...)` at the top of both.
2. **Unpinned manifest.** The manifest records hashes after copying and modification but is not compared to reviewed expected values. `prepare_secure_call.py` hashes the source and then re-reads it for `copyfile`, a time-of-check/time-of-use gap. Fix by writing the bytes already hashed, and pin the expected post-modification hashes (`Cargo.toml` files, `lib.rs`, `g0_calls.rs`, lock).
3. **Extra files in the work tree.** The verifier does not check for files outside the manifest. An auto-discovered `build.rs`, a `.cargo/config.toml`, or an extra example or test file would compile undetected. Parent-directory `.cargo/config.toml` files also apply if the build is not run in a container that mounts only the work directory. Assert that the work tree, excluding `target/` and generated outputs, equals the manifest.
4. **Ambiguous 488 source.** `probe_rejected_offer` cannot tell whether the 488 came from the PBX or from callee `b`. A forwarded leg would most likely be answered 200, so a false pass is unlikely, but you could additionally assert that `b` received no INVITE transaction.
5. **Package set equality.** `verify_secure_dependencies.py` should compare the inventory and tree package identities as sets. The current length-plus-lookup check would miss one duplicated and one missing entry.
6. **Things the build or runtime will settle:**
   - **Undeclared dependencies.** `rustls`, `libc` and `serde_json` are used directly. If any is not already a direct telephony dependency, the build fails, and adding it changes the "only ring/base64 are new direct dependencies" claim.
   - **Media manifest.** The appended `[dependencies]` table assumes `crates/media/Cargo.toml` has none yet.
   - **Asterisk SDP.** If Asterisk offers several `a=crypto` lines, or an `a=rtcp:` line, the strict parser rejects the call. That fails the run rather than producing a false pass, but it may block the experiment.
   - **Dead code.** The `udp`/`tcp` branches in the example are unreachable and can be removed.

## Disposition

APPROVE was static only. Its hardening notes led to fail-closed optimized-Python checks, pinned post-modification inputs, byte-identical checked writes, extra-source/config rejection, package-set equality and callee-INVITE observation for 488 origin. The final changes receive a separate round 3 review.
