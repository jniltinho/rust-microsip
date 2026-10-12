# Claude PBX review — round 2

Date: 2026-10-12. Scope: disposable PBX/registration fixtures, not the Rust adapter or complete G0. Read-only Claude CLI, no tools/MCP/session persistence/hooks, sanitized own-source input only. Input SHA-256: `e7d78a40b79ac41a1869a044d0a09ba06f6fd31b30dc6924f29ed3002c0b8a4d`. CLI exit 0, result subtype `success`, `is_error=false`. The reviewer did not execute tests; runtime evidence is in the [PBX receipt](../receipts/asterisk-lab-2026-10-12.md).

The review below is preserved in English. Nonblocking suggestions remain limitations, not silently waived release requirements. Host-profile egress is now explicit in ADR 0002. No blocker was reported.

---

# Second Review: Disposable Asterisk Lab Scaffold

**Verdict: APPROVE.** This is a read-only review. I did not execute anything. Test results are as you reported them.

## Blockers

None found.

The `.gitignore` check from the first review is resolved:

- `/artifacts/`, `*.key` and `*.pem` are ignored.
- The root `.dockerignore` is a strict allowlist that cannot reach `artifacts/`.
- The lab build context has its own allowlist.

The CA issuer key lives only in `artifacts/asterisk-lab/issuer/`. That path is git-ignored, outside both build contexts, and not mounted into any service. Your `git check-ignore` and build-context tests are the right evidence for this.

## Verified by reading

- **Image:** two-stage build. The pinned digest is flattened onto `scratch`, which drops the inherited VOLUME, entrypoint and healthcheck. It runs as numeric non-root, and build and Compose both pin the platform.
- **Compose:**
  - Every service is read-only, with `cap_drop: ALL` and `no-new-privileges`.
  - Each has exactly one read-only bind mount with `create_host_path: false` and private tmpfs.
  - The internal network is internal and publishes no ports.
  - The host overlay publishes only to loopback, and peer and test-runner are profiled out.
- **`verify_compose.py`:**
  - It checks the normalized output.
  - It rejects privileged mode, devices, `cap_add`, `volumes_from`, host pid/ipc/network and uid 0.
  - It checks exact service sets and exact port tuples.
  - Using `network_mode: service:`/`container:` fails closed through the network checks.
- **Secrets:**
  - Files are created with `O_EXCL|O_NOFOLLOW` at 0600 and validated for ownership and mode on load.
  - Account values are regex-restricted, which blocks config injection.
  - Clients never receive the server key, and nothing receives the CA key.
  - Top-level errors print sanitized messages in `lab.py` and `ProbeError`.
- **Runner:**
  - Root is refused, and the artifacts symlink is checked.
  - The 120s runner uses kill-after.
  - The log write skips a symlink target, and cleanup always runs `down --volumes`.
- **Probe:**
  - Reads are bounded, and Call-ID and CSeq are matched.
  - The three-transport positive path, wrong-password rejection and unknown-CA rejection are all present.
  - All clients are closed even when one close fails.
  - The receipt honestly reports `calls_tested: false` and `g0_passed: false`.
- **ADR:** the provenance, license and acceptance limits match the code.

## Nonblocking suggestions

1. **TCP/TLS deadline (`sip_probe.py`, `request`/`receive`):** for streams, `attempts = 1`. A single `recv` hitting its 1.5s socket timeout raises `socket.timeout`, which ends the only attempt and gives up long before the 6s deadline. It fails closed, but it may flake on a slow PBX. A fix is to retry `recv` on timeout inside `receive()` until the deadline expires.
2. **`verify_compose.py` exception handling:** consider catching `Exception` so malformed input, such as a `TypeError`, gives the same clean rejection message instead of a traceback. It already exits non-zero, so this is cosmetic.
3. **`verify_compose.py` hardening:** you could optionally also reject:
   - `userns_mode: host`
   - `seccomp:unconfined` or `apparmor:unconfined` in `security_opt`
   - `sysctls`
4. **Symlinked checkout path:** `Path(...).resolve()` resolves symlinks, but Compose may emit the unresolved path. A checkout under a symlinked directory would then be rejected falsely. This fails closed.
5. **Partial `generate()` failure:** if a later `rename` fails, a partially populated lab directory is left behind and blocks a rerun until manually deleted. This fails closed and is acceptable for a disposable lab.
6. **PBX healthcheck timing:** up to six `asterisk -rx` calls at 5s each can exceed the 10s healthcheck timeout under extreme slowness. This only delays readiness.
7. **Host profile egress:** the `host-lab` network is non-internal, so the PBX has outbound egress in that profile. It is documented as opt-in and there are no trunks or wildcard routes, so this is fine. One ADR sentence noting it would help.

## Scope reminder (unchanged)

This approval covers the isolated registration and TLS lab infrastructure only. G0 remains open for:

- calls, SDP and NAT rewriting
- bidirectional media and SRTP negotiation
- the Rust adapter
- VM ACLs
- the long-running call peer

Each needs its own receipt.
