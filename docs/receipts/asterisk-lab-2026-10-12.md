# Disposable PBX infrastructure receipt — 2026-10-12

Status: **task 1.4 verified; complete DIST-04, task 1.5 and G0 remain pending**. This is infrastructure/registration evidence, not a usable client or proof of calls/media. Scaffold milestone: `1bfbe7d`. The following hashes identify the tested PBX source before its milestone commit.

## Identity

- Foundation: `andrius/asterisk:22.11.0_debian-trixie@sha256:eaa6aaaa09ede2829514769d5d1aba3041ec8b3055899feebfc8e8a9a1af7c0f`, selected linux/amd64 platform manifest.
- Reviewed upstream revision: `4e421e1cef64dd864f86881f852a173c9b8b7e0d`.
- Local wrapper image ID: `sha256:c5116ce205df6b00484bd466834a1f6736334268a4a4bc8b78d06e2eb0d4029d`. This local content identity is not a registry signature or reproducibility claim.
- Runtime: Asterisk 22.11.0, Python 3.13.5, OpenSSL 3.5.7; Engine 29.1.3, Compose 5.6.0.
- Source-manifest SHA-256: `868555678d680a2fdfa141b6199dc18b82cfb01956dba603d3ed83d85ec6e3fc` (UTF-8 JSON of the following path/hash map, sorted keys, compact comma/colon separators). Documentation is not part of the runtime-image manifest.

| Source | SHA-256 |
| --- | --- |
| `docker/asterisk/Dockerfile` | `56c1f5095f12aba61b6b4581a138690b6a24d9a0ce22841ffd3f083c611a53a7` |
| `docker/asterisk/.dockerignore` | `eef5bf077fab3d87c345d121ee58174eda7e54a96e990c7a1963c4f4989bd31e` |
| `docker/asterisk/compose.yaml` | `ba4522214d2549c16ea07ae552d53df4fca5c25cd9ecd69dace302512e878a68` |
| `docker/asterisk/compose.host.yaml` | `cdce354da24a67df35152da3644a77644ec71ab7fc8f21598362d5bbf631c85e` |
| `docker/asterisk/lab.py` | `e1d18e79c8c65fa01028080e72609e2f795bc618900cc02eecc49afb5a60ea40` |
| `docker/asterisk/sip_probe.py` | `ff60317b4015be807c644d1d0b2e405636c0c8641bc47847ae9aa8ca83c249f6` |
| `docker/asterisk/lab.sh` | `8f853e1c64de3673ce06d641dcec09c4834b85e02435636f915702c4a5d66607` |
| `docker/asterisk/verify_compose.py` | `67dc0090483926373df7278e21266c560a6fe6cf7fa5b6f2525625bc69308c97` |
| `docker/asterisk/tests/test_lab.py` | `9157640df8450410ef4c6b356bf4ad4b6bbf379d6822cfe9006c2b6d0eb9e93c` |
| `docker/asterisk/tests/test_sip_probe.py` | `b8226db99db92ec1c1483bda5ccb92ff6366f03603ac5c49194ec5c7c6524c06` |

## Actually executed

1. Initial generator/config tests failed RED when implementation was absent. Final `sh docker/asterisk/lab.sh unit-test`: **11 passed** in the pinned non-root, read-only, networkless container. Covers permissions, symlinks, overwrite refusal, account validation, routes, bounded SIP parsing, digest/header injection and password non-disclosure.
2. `lab.sh build`: exit 0. Runtime checks assert Asterisk version and each allowlisted module exists. Image inspect confirms own entrypoint, non-root UID and no inherited anonymous volumes.
3. Fictional CA/password generation: owner-only 0700 directories/0600 files. Explicit separate mounts exclude the issuer key and keep the server key out of clients. Git ignore checks confirmed credentials/issuer key excluded. No credentials or CA private material are in this receipt/build contexts.
4. `lab.sh config-check`: both internal and host normalized configurations passed. Checks remain active under `PYTHONOPTIMIZE=1`. A modified uid-0 configuration was rejected under Python `-O`, proving it does not rely on disableable assertions.
5. Actual default `lab.sh test`: exit 0. PBX and peer became healthy; explicit readiness checks found UDP/TCP/TLS transports, all fictional endpoints, `res_srtp` running and echo/IVR/tone routes. Peer contacts 1002/1102/1202 were independently inspected during an earlier live run.
6. Probe authenticated **401 → 200** over UDP, TCP and validated TLS; wrong-password rejection passed on each transport; a client without the generated CA rejected TLS. JSON reported `calls_tested=false`, `g0_passed=false`. No Authorization/SDP keys or passwords were printed.
7. Container inspection confirmed non-root, read-only filesystem, dropped capabilities, one exact read-only disposable mount per service, internal bridge and **no published ports**. Default dialplan has only exact local destinations, no PSTN/trunk route. Host configuration has only loopback port mappings; it was normalized/checked, not used as host-media acceptance evidence.
8. On completion, runner, peer and PBX exited 0; Compose removed containers and the isolated network. A label-scoped inventory found no remaining lab containers/networks. Secret files remain local/ignored until explicitly deleted.
9. Read-only sanitized Claude CLI [round 1](../reviews/claude-pbx-round-1.md) and [round 2](../reviews/claude-pbx-round-2.md) returned **APPROVE for lab infrastructure only**. Conditional Git-ignore concern was verified against actual ignore rules; follow-up explicitly resolved it. No test was executed by the reviewer.
10. English/privacy/link checks and strict OpenSpec validation passed. The Docker builder's separate context negative test passed with synthetic private sentinels; the lab context also has an explicit own-file allowlist and cannot reach `base/` or `artifacts/`.

Final raw outputs stay ignored in `artifacts/asterisk-andrius-final-build.log`, `artifacts/asterisk-unit-final.log` and `artifacts/asterisk-andrius-final-test.log`. The startup warning about Stasis threadpool `minimum_size` metadata did not prevent readiness or tested registrations; it remains an upstream/runtime observation, not evidence of call health.

## Boundaries

Task 1.4 covers the pinned PBX, routes and default isolation/readiness, now implemented. **Full DIST-04 also needs the future automated call scenarios; it is not declared passed.** Echo/IVR/tone audio and SRTP negotiation have not yet been called. The peer is registration-only and may reject unsolicited dialog traffic. Host bridge/loopback configuration and the secret generator are partial task 1.5 work; host SDP/media and auxiliary-VM ACL acceptance remain unimplemented/unverified.

No Rust registration/call adapter, bidirectional G.711, DTMF/hold/transfer interoperability, full native/license audit, desktop/audio VM acceptance, usable release, tag or visibility change is established. Passwords do not expire by themselves; delete them after teardown. See the [lab guide](../../docker/asterisk/README.md) and [ADR 0002](../adr/0002-disposable-asterisk-lab.md).
