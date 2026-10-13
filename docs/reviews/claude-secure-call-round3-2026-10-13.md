# Claude static secure-call review, round 3

Date: 2026-10-13. Sanitized, tools-disabled static review only; not runtime, G0 or release approval.

- Terminal exit: 0; CLI subtype: `success`; is_error: `false`.
- Prompt SHA-256: `5d698f26ae70bfb14b1df5bd154253dbdbe5309b8df43bebcd7b38fc2e8b427d`.
- Raw JSON SHA-256: `8323c09eaf4db412526613c6d748c8ee04c1a6703e4ec6f5ce4ebb06959ad0ae`.
- Hooks, MCP, write/browser tools, slash commands, auto-memory and session persistence were disabled. A neutral temporary directory was used.

## Verbatim result

**APPROVE** (static review of the stated narrow candidate only; this is not runtime, G0 or release acceptance)

I found no concrete correctness, security or privacy blocker against the narrow claims. Each Round 2 hardening note is addressed in code.

## Round 2 notes, verified

- **Optimized Python is rejected.** `prepare_secure_call.py`, `observe_secure_call.py` and `verify_secure_dependencies.py` all exit under `-O` before any `assert` can be stripped.
- **Preparation writes the bytes it hashed.** Each application input and probe file is read once into `payload`, hashed, then `payload` is written. There is no second read, so no read/write race. The candidate lock follows the same pattern.
- **All 38 post-modification inputs are pinned.** `candidate_inputs_sha256` covers the 34 baseline paths plus 4 probe destinations. The `Cargo.lock` entry equals `baseline_candidate_lock_sha256`, so the lock replacement is pinned too. The receipt's input list is not itself hash-pinned, but this does not matter: any drift fails either the allowlist-equality check or the final 38-hash comparison.
- **A failed preparation cannot be verified.** On mismatch, preparation exits before writing `prepared-source-inputs.json`, and verification requires that file.
- **Verification rejects extra files and checks package-set equality.**
  - It walks the candidate without following links and rejects any symlinked directory, including ignored ones.
  - The file set must exactly equal the prepared inputs plus three known artifacts.
  - Every prepared hash is recomputed.
  - Package identity sets are compared for equality, and all three sets must have length 100, which excludes duplicates.
- **A callee-origin 488 cannot pass the negative probes.** The inspector sets `saw_invite` before transaction dispatch. Any callee-generated 488 is therefore preceded by that store, and the 488 reaches the caller only after the PBX relays it. The caller also gets an Err unless the final status is exactly 488.
- **TLS oracle and unit controls are unchanged and remain sound.** They accept only typed rustls `UnknownIssuer` or `NotValidForName*` errors, without constructing a SIP endpoint. Refused, expired and unrelated errors map to `None`.

## Narrow claims, rechecked

- **SDES subset:** Parsing is bounded. The parser rejects:
  - session-scoped or duplicate crypto lines
  - lifetime/MKI parameters (they fail the 40-character key check) and session parameters (they fail the field-count check)
  - AVP downgrade, other suites and non-PBX addresses or ports
  - extra media lines and non-sendrecv directions

  Keys must be canonical 30-byte base64; 240 bits is exactly 40 characters, so there is no trailing-bit ambiguity. The answer tag is bound, and incoming offers are restricted to tag 1.
- **Media:** Packets are authenticated before decoding. Source, length and payload type are enforced. Wrong-key, tag-tamper, sequence-tamper, clear and replay negatives run against a real PBX packet. Because the select is biased and the first tick fires immediately, `send` holds a real clear packet before any receive is processed.
- **Lifecycle:** Each key moves once into its sender. Admission is claimed once per endpoint. Success flags are set only after the ACK/BYE/200, termination and unregistration waits.
- **Privacy:** Output is numeric aggregates and constants only. Errors are sanitized. Credentials are read from a 0700/0600 `/lab` path with `O_NOFOLLOW`.

## Nonblocking notes (verify at runtime; not static blockers)

1. **Cargo configuration outside the candidate directory is not checked.** The verifier checks only `CARGO_HOME` config. Cargo also reads `.cargo/config.toml` from every ancestor of the working directory, such as `root/artifacts/.cargo` and `root/.cargo`, and honors `RUSTFLAGS`/`CARGO_*` environment variables. If the build does not run in a container that mounts only the candidate, the runtime evidence should record that no such configuration or environment variables were present.
2. **Observer paths are not tied to the verified name.** The observer hardcodes `artifacts/secure-call-work`, while prepare and verify accept a name argument. The observer also does not cross-check `prepared-source-inputs.json` against the ELF it runs. The receipt must bind ELF ↔ candidate.
3. **Observer deadline can expire before the probe's own timeouts.** The 160 s deadline is less than the worst-case sum of inner timeouts (about 230 s). This fails closed, but a slow PBX would surface as an observer timeout rather than a typed probe error.
4. **Rejection behavior depends on runtime.** If the rsipstack version maps TLS errors to strings instead of wrapping `io::Error`, the certificate negatives will fail closed and the oracle will need adjustment. Asterisk returning exactly 488 (rather than, say, 415 or 606) for each forbidden offer is likewise runtime-only.
5. **The UnsupportedSuite negative sends a real local key to the PBX.** The key is discarded unused, but it is worth noting in ADR 0011's erasure caveats.

The ADR's open gates remain in force: SRTCP, rekey/ROC, secure hold/transfer/DTMF, production supervision and erasure.

## Runtime disposition

The final source was built and tested in containers mounting only the explicit candidate and dedicated Cargo cache, with no forwarded host build flags/configuration. The separately prepared copy produced the identical ELF. The receipt binds both copied sources, lock, images and ELF. Actual Asterisk 488 and exact typed rustls certificate negatives passed; the callee observed no INVITE for all three rejected offers. The fixed observer directory and stricter outer deadline are deliberate scope limits, not general-account controls. A negative unsupported-suite offer carries a fresh unused local key over TLS; own/SDK allocation erasure remains unproven as documented in ADR 0011.
