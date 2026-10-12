# Claude CLI review — disposable PBX, round 1

Date: 2026-10-12. Sanitized input SHA-256 `0d72e851f839de20016800018d38fea71ad577785acecd3c08d20979bd063e1d`. CLI safe/print mode, no tools, empty strict MCP, disabled hooks/auto-memory, isolated settings, no session persistence. Exit 0, subtype success, is_error false. Full English report below; static review only, not call/G0/release evidence.

The conditional artifacts-ignore item was independently checked: /artifacts/ is ignored, Rust context allowlist excludes it, and clean-context positive/negative tests passed. Nonblocking hardening changes were subsequently applied and retested; see round 2.

---

# Review: disposable Asterisk lab and Rust scaffold

**Verdict: APPROVE.** This approves only the isolated infrastructure and scaffold milestone. It is not a G0 or release approval.

I found no concrete correctness, security, privacy or license blocker in the code shown. One item depends on a file I could not see (point 1 below). If that check fails, it becomes a blocker.

## Requested checks

- **Inherited image metadata is removed.** `FROM scratch` plus `COPY --from=upstream / /` drops the upstream VOLUME, ENTRYPOINT, CMD, HEALTHCHECK, EXPOSE, LABEL, USER, ENV and STOPSIGNAL. The base image is pinned by its amd64 manifest digest, and the build step checks the Asterisk version and the module allowlist. Files keep their upstream ownership and setuid bits, but `no-new-privileges` and `cap_drop: ALL` neutralise the setuid bits.
- **Mounts are minimal and fictional.**
  - Each service has exactly one read-only bind mount with `create_host_path: false`.
  - The PBX sees only `pbx/`, which holds its key, certificate, CA certificate and credentials.
  - The clients see only `clients/`, which holds the CA certificate and credentials.
  - The CA key is never mounted.
  - Nothing mounts HOME, the Docker socket, DBus or devices.
  - The build context is `docker/asterisk` with a `**` deny-first `.dockerignore`, so `artifacts/` cannot enter the image.
- **No public ports by default.** The default network is `internal: true` with no published ports, and `verify_compose.py` checks both. The host profile publishes to `127.0.0.1` only, is opt-in, and runs the PBX alone.
- **Fixture behaviour is bounded in practice.**
  - Messages are capped at 64 KiB.
  - Each `recv` has a timeout.
  - UDP retries at most three times, and each request has a 6 s deadline.
  - Errors carry no payload, and the Authorization header contains no password.
  - Positive results are only emitted after `register()` succeeds, so the receipt is not a hardcoded claim.
  - The receipt reports `calls_tested:false` and `g0_passed:false`, and `sip_probe.py` and the ADR say plainly that this is not the Rust adapter.
- **The secret lifecycle is sound.**
  - Generation is refused unless the target is an empty, owned, non-symlink directory with mode 0700.
  - Files are written with `O_EXCL|O_NOFOLLOW` under umask 077, and the staged output is renamed into place.
  - When reading credentials, the code checks owner, mode and size and validates them against a strict regex, which blocks config injection.
  - Output passed through the top-level error handler is redacted.
- **The PBX surface is closed.**
  - No anonymous endpoint and no AMI or HTTP.
  - CDR and CEL are off.
  - `autoload=no`, the dialplan uses exact extensions only, and there is no recording or PSTN route.
  - All mutable state lives in tmpfs.

## Conditional item to confirm

1. **Confirm `artifacts/` is git-ignored** and excluded from any Rust or AppImage packaging inputs. It holds live lab credentials and a CA private key on the host, and I could not see `.gitignore`. If it is not ignored, treat this as a blocker.

## Nonblocking observations, roughly by priority

2. **`verify_compose.py` uses `assert` for its safety checks.** With `PYTHONOPTIMIZE` set, or `python3 -O`, every check is stripped and the script prints "OK". Either raise explicit exceptions or invoke it as `python3 -I`, which ignores `PYTHON*` variables. It also does not check `cap_add`, `pid`, `ipc` or `volumes_from`. It accepts a user of `0` or `root` because it only rejects the exact string `"0:0"`. In the host profile it does not assert that the service set is exactly `{pbx}`.
3. **The deadline is not enforced inside `receive()` for TCP and TLS.** A peer that trickles bytes can keep the loop running until the 64 KiB buffer fills, well past the 6 s deadline. The PBX is trusted, so this is low risk. To make the bound real, set the socket timeout to `min(1.5, deadline - now)` and check the deadline inside the loop. `lab.sh test` also has no overall wall-clock timeout.
4. **Delete `issuer/ca.key` after signing.** Nothing re-issues certificates, so keeping the key only lengthens its exposure.
5. **The "valid for 3 days" message is misleading.** Only the certificates expire; the passwords persist until someone deletes the directory.
6. **The peer is fragile while running long under `up`.**
   - Any unsolicited request arriving at its socket during renewal causes a `ProbeError` and the process exits. This includes the INVITEs that future call tests will send.
   - With `max_contacts=1` and `remove_existing=no`, a crashed peer blocks re-registration for up to 120 s.
   - Clean unregistration can take about 18 s in the worst case, which is more than the 15 s `stop_grace_period`.
   - In the `finally` loop, if one `close()` raises, the remaining clients are never closed.

   None of this matters for the current scope, but the G0 harness will hit it.
7. **The host profile gives the PBX outbound network access** (`internal: false`). Publishing to `127.0.0.1` also relies on Docker Engine's loopback hardening; older engines allowed direct routing to container IPs from the same L2 segment. Document a minimum engine version or add a firewall check.
8. **`lab.sh` changes worth making:**
   - Refuse to run as uid 0.
   - Add `--load` to `buildx build`, since a non-default driver will not load the image.
   - Add `platform: linux/amd64` in the Compose file.
   - Note that `> artifacts/asterisk-smoke.log` will follow a symlink.
9. **The top-level `except` in `lab.py` is narrow.** Other exception types still print tracebacks. That is acceptable for now, but catching `Exception` would make the "never echo payloads" promise hold everywhere.
10. **The digest parser rebuilds the Authorization header from server-supplied unquoted values**, for example `opaque`, without rejecting `"`. This is trusted-PBX only and not exploitable here.
11. **ADR wording.** Asterisk is "GPL-2.0 with an OpenSSL linking exception", not general linking exceptions. The image also carries the upstream sound and MOH assets under `astdatadir`, which have their own licenses (CC-BY-SA). These are not redistributed, but list them explicitly in the future image-publication audit.
12. **`method=tlsv1_2` pins the PJSIP transport to TLS 1.2 only.** That is fine for the lab, but the Rust adapter's TLS 1.3 path will not be exercised against this PBX unless this changes.

**Rust scaffold:** no issues. Argument parsing is strict, invalid input is never echoed, non-UTF-8 arguments fail cleanly, exit codes are honest, and the diagnostic is static with no host probing. The tests cover all of these.
