# G0 Rust G.711 and synthetic-call receipt — 2026-10-12

**Result: verified experimental sub-milestone, not completed G0, AUD-01, task 1.6/5.1 or a usable release.** Baseline: `bfe953961423b262937c0c46f6ad99a84993cedc`; this receipt is committed with the tested own sources. See [ADR 0005](../adr/0005-own-g711-and-synthetic-rtp.md), the [historical registration receipt](g0-rust-registration-2026-10-12.md), and the [sanitized numeric evidence](g0-rust-media-2026-10-12.json).

## Exact identities

| Input/artifact | SHA-256 |
| --- | --- |
| Cargo.lock | `0935dabb58e0d6246fca3c9fe71f5d9b473ce82e7fea7ede24004e2bf5b3e0a7` |
| Own media source | `822285e5bac1d1e815d5a14ee4b6d51ce052ef060d56e4f278330a26bb870391` |
| Call adapter source | `dd9c6629117777e7e9746a2fd2b63b2d3fc0e60c8c62b33b71f49643974c4a09` |
| Call example source | `ca22786c6c6f2dd5d07bd55cc27947cfa90b4fd197674aa11f275b1315c685e0` |
| Build source manifest (27 individually copied inputs) | `6f24b7e9b7029c76a06d77c62d69c7f825ac22058779c606e7338804e0180930` |
| Selected Cargo inventory | `815e71745bd2170de871dc0a2304a0350fd2a5dd8c4f078579674bc1ba22ff4a` |
| Call runner local image ID | `7595335193d6ba88d7cfc2f22e28b48d82ef99aabb50c4acfecdf2a54d203e3c` |
| Call runner ELF (4,166,136 bytes) | `f42d64b8790706fcea95917c35d0c419205ad81bbe401f40419eea6bfe42ad3d` |
| PBX wrapper local image ID | `ff5e8bcf5835a957c7ad66f8c9cc6fad5ef65cdfe02a6abcad2d3c4f0fb0ff29` |

The wrapper retains the exact pinned `andrius/asterisk` foundation from [ADR 0002](../adr/0002-disposable-asterisk-lab.md). Local image IDs identify this execution, not signed/independently reproduced registry artifacts. The CLI scaffold ELF remains `d24ff6ebc1496bbddb8fd1004c2840b6d22a97056432701bbb3865a25ddc8ea0`, with no calls/device access. No AppImage, tag or release was produced. Build/ELF hashes and source checksums are local integrity evidence, not authorship attestation.

## Executed tests

1. **G.711 TDD:** tests first failed against missing APIs. The first implementation then failed the exhaustive PCMU encoding hash: clipping at 8159 allowed an out-of-range segment after bias. Correcting the clip to 8158 passed both codecs' full 65,536 encodings and 256 decodings against independently generated CPython 3.12.3 functional hashes. No upstream source, codec tables or recorded audio were copied. `python3 packaging/g711-reference-vectors.py` regenerates hashes only on little-endian CPython 3.12; it is not required by the Docker build/product.
2. **RTP and negative controls:** eight media tests cover codec vectors, static payloads/silence, caller-owned serialization, version/truncation/oversize, CSRC/extension/padding, non-partial output failure and deterministic malformed bytes over lengths 0–1500. A separate tracker test first failed to compile before implementation, then passed sequence/timestamp rollover, duplicate/late handling, loss accounting and bad timestamp/SSRC rejection. Tone analysis accepts the other endpoint's tone but rejects silence, own echo and wrong frequency. Admission tests reject mandatory-SRTP accounts, mismatched transports and pre-cancelled calls before network setup. No coverage-percentage claim is made.
3. **Offline workspace:** 10 CLI process tests, 8 media tests, 5 telephony unit tests, 6 registration integration tests and 1 doctest pass. All-target fmt/clippy with warnings as errors and warning-free rustdoc pass. Seventeen packaging tests pass normally and under Python `-O`; fourteen PBX/Compose fixture tests pass. An intermediate lint failure for a test-only `expect()` was fixed using a constructed socket address, not a lint exemption. All three OpenSpec changes validate strictly; no task acceptance criteria were reduced.
4. **Actual calls:** the exact final runner above exited 0 in repeated real Asterisk executions, not a Python SIP simulation. Two Rust endpoints in **one process**, with different dialogs and signaling/media sockets, exchanged 440 Hz and 880 Hz through anchored PBX media. Each matrix row below includes both origination directions (two calls) and both receiving endpoints:

   | Signaling | Codec | Calls | Duration each | Packets sent/received per endpoint | Loss/late |
   | --- | --- | --- | --- | --- | --- |
   | UDP | PCMA | 2 | 5 seconds | 250 / 250 | 0 / 0 |
   | UDP | PCMU | 2 | 5 seconds | 250 / 250 | 0 / 0 |
   | TCP | PCMA | 2 | 5 seconds | 250 / 250 | 0 / 0 |
   | TCP | PCMU | 2 | 5 seconds | 250 / 250 | 0 / 0 |
   | TLS | PCMA | 2 | 5 seconds | 250 / 250 | 0 / 0 |
   | TLS | PCMU | 2 | 5 seconds | 250 / 250 | 0 / 0 |

   Observed RMS is 5635.86–5675.25; remote-to-own tone power ratio is 1626.65–1768.12 (required minimum 20). The probe's packet thresholds are ≥240 sent/≥200 received; **zero loss/late is the observed result, not its enforced acceptance threshold**. Receiver checks include source/codec/frame length, stable SSRC and sequence/timestamp continuity. The two source tones distinguish received remote media from a local echo. ACK, BYE/200, both endpoint termination observations and both authenticated unregistrations gate return success. The phase watcher is endpoint-wide rather than bound to a dialog ID; per-dialog correlation remains a production gate. Their report booleans summarize these in-process checks, not independent wire capture. Fixed sender sequence/timestamp values exercise rollover; the separate tracker unit test verifies receiver rollover logic. Production randomness/RTCP/jitter/impairments are not tested. Identical deterministic tone rows are expected in this lossless fixture; transport variation covers signaling only, since all RTP is UDP, not independent media transports.
5. **Independent PBX/runtime corroboration:** an external Docker/Asterisk observer saw two active channels and one active call during media, both containers non-root/read-only/capability-dropped with no published ports and one private read-only bind each, and `rewrite_contact=false` on all six fictional endpoints. After successful calls, PBX queries found zero active channels/calls and no registered contacts. Teardown left zero project containers/networks. This is independent signaling/lifecycle/isolation corroboration, **not PBX-side RTP-tone measurement**, a packet capture, separate-machine execution or hardware audio proof.

   The first observer validations failed because they expected `1 active call`/`0 active calls`; Asterisk's configured call limit actually prints `1 of 8 max active call`/`0 of 8 max active calls`. These failures were retained, not counted as passed observer runs. Captured active/final CLI counts were checked using the actual bounded format, with positive/negative parser controls, and the final saved-evidence verifier exited 0. The calls themselves had exited 0. The ignored verifier is `artifacts/verify-media-evidence.py`; its SHA-256 and hashes of the saved active/final CLI counts are recorded in the public JSON, without publishing raw operational logs. Verifier SHA-256: `c206223b41674ab976467d0010d0a5182c75c817a80736a1d8eeb716361f32db`. No source change or weakened zero-channel/contact criterion was used to repair the observer interpretation.
6. **Security scope:** TLS validates the lab CA/hostname but calls use **1301/1302 explicit plaintext RTP**, not SDES/SRTP. 1201/1202 are refused. No personal configuration, production account, microphone, installed AppImage or actual `base/` was accessed. Raw SIP/SDP/auth, audio samples, keys and credential values are not serialized in this receipt.
7. **Build/publication boundaries:** selected normal/build and dev-inclusive trees pass provider/source/feature checks. The selected registry inventory remains exactly **73 packages / 143 original notice files**, byte-identical to the historical registration inventory; [current inventory](../inventory/g0-media-cargo.json). The media crate adds no runtime dependency; its SHA-256 test crate was already selected. Positive context audit, deliberately permissive negative control identifying synthetic `.env` and `base/private.txt`, and clean individually enumerated own-source export all pass. Export checksums pass; no actual local reference enters the context. The functional-oracle developer script and PBX sources are not among the 27 application build inputs; their receipt hashes are separate. Staged publication must still match these identities and pass the privacy scan.
8. **Graph-assisted inspection:** codebase-memory MCP search, both-direction trace, exact snippets and coverage checks were used for the media/call source, examples, tests and Compose checker. Checked paths report `metadata_match` with no recorded gap at generation `2026-10-13T00:42:45Z`. The async/macro trace is incomplete despite that best-effort signal; direct source/tests support the call lifecycle, not an exhaustive graph claim.

## Reproduction and mandatory follow-up

From the repository root, with current still-valid fictional fixtures:

```sh
sh docker/asterisk/lab.sh build
# Run init only with a fresh, empty ignored lab directory.
sh docker/asterisk/lab.sh init
sh docker/asterisk/lab.sh unit-test
sh docker/asterisk/lab.sh calls-build
sh docker/asterisk/lab.sh calls-test
sh packaging/check-context.sh
```

Raw local logs/evidence remain ignored in `artifacts/`, including `g0-media-red.log`, `g0-media-green.log`, `g0-tracker-red.log`, `g0-tracker-green-final.log`, `g0-calls-build-final.log`, `g0-calls-reviewed-runtime.log`, the observer diagnostics and `g0-media-context-final.log`. The public JSON is a selected aggregate receipt, not a raw log or credential/configuration export.

Independent read-only Claude review [round 1](../reviews/claude-rust-media-round-1.md) requested the missing receipt and resolved observer evidence; [round 2](../reviews/claude-rust-media-round-2.md) records the follow-up disposition. Static review is not call-testing evidence. Endpoint-wide rather than per-dialog lifecycle correlation, compact macro bodies needing formatting before production reuse, upstream **and adapter-originated** secret copies/read-parse buffers, unbounded channels/parser limits, authenticated failure-path teardown and IPv6 remain unproved. DTMF/hold/REFER/SRTP, real devices/resampling/jitter/RTCP, host/VM/NAT, complete-stack desktop/IPC ELF and exact-artifact human voice/history acceptance remain mandatory. The short synthetic matrix does not close task 1.6 or authorize full UI/release work.
