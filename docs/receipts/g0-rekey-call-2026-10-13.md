# TLS/SDES mid-call rekey receipt — 2026-10-13

**Result: one caller-initiated SDES rekey and one refused plaintext re-offer passed against the lab PBX; not application adoption, PBX-initiated rekey, secure hold/transfer/DTMF, G0 or a usable release.**

Baseline: `de92e00a20417bbb0f77a46ed8c32e66b62d74c9`. The [machine receipt](g0-rekey-call-2026-10-13.json) binds the 39 copied compilation inputs, harness hashes, runtime images, ELF, measurements, log hashes and the unpinned discovery runs. It extends the [SRTCP receipt](g0-srtcp-call-2026-10-13.md) under [ADR 0011](../adr/0011-isolated-initial-tls-sdes-calls.md). The packet adapter is unchanged; application manifests/lock/runtime remain unchanged.

## Discovery: why a rekey starts a new SSRC

The pinned `rustrtc` provider has no ROC setter, and the lab Asterisk `res_srtp` imports only `srtp_create`, `srtp_add_stream` and `srtp_remove_stream` (no ROC get/set). Three disposable, unpinned single-call builds re-INVITEd with a fresh key after the caller's sequence had rolled over:

| Re-offer with a fresh key | 200 OK | PBX key changed | Callee packets after the switch |
| --- | --- | --- | --- |
| Same SSRC, ROC continued (provider ROC walked with discarded packets) | yes | no | **0** |
| Same SSRC, ROC restarted at zero | yes | no | 349 |
| New SSRC, ROC zero, sequence continued | yes | no | 349 |

This Asterisk 22.11/libsrtp2 lab restarts the ROC on a key change, so same-SSRC ROC continuity does not interoperate here. The probe therefore uses **fresh key + fresh SSRC with ROC zero**. The new source's RTP sequence restarts near the wrap (65500), so it crosses a rollover under the new key within about one second; a fresh random key means no (key, index) pair is reused. The ROC-walking workaround was discarded and is not in the pinned source. This is observed lab behavior, not a general standards claim.

## Actual PBX execution

Four sixteen-second calls (PCMA/PCMU, both initiator directions) over validated TLS with mandatory, non-optimistic SDES fixtures. In each call, the caller:

1. at 3 s sends a key-free plaintext re-offer (`RTP/AVP`, no `a=crypto`, `o=` version 2): the PBX answered **488** and the keyed call continued;
2. at 6 s sends a fresh-key re-offer (`o=` version 3): the PBX answered **200** with a valid SDES answer on the same anchored address and the **same PBX key line** (a changed peer key fails closed and is not exercised);
3. replaces its sender with a fresh key and SSRC; the old sender and its own key bytes are dropped. The initial PBX answer body is moved (not copied) into `Zeroizing` for the key-line comparison, and the rekey answer must keep both the RTP and RTCP addresses.

| Measurement, per endpoint and call | Value |
| --- | --- |
| RTP sent / received | 800 / 800, zero lost or late |
| Authenticated RTP received after the switch | 499 |
| Minimum decoded RMS / remote-to-own tone power ratio, whole call | 5635.86 / 1626.65 |
| Minimum RMS / tone power ratio of audio received **after** the switch (gated) | 5635.90 / 1626.77 |
| SRTCP sender reports sent | 16 |
| Caller: PBX blocks about the new SSRC echoing an SR sent under the new key | 1 |
| Caller: PBX-reported extended highest sequence for the new SSRC only (gated > 65535) | 65948–65949 |
| Callee: authenticated PBX sources (the PBX also changed its outbound SSRC) | 2 |

The echoed LSR proves the PBX authenticated SRTCP protected with the new key; the per-source extended sequence shows the PBX followed the new source across its rollover (ROC 0→1) under the new key. RTP decryption under the new key is established by the callee's gated post-switch audio: a separate tone meter fed only after the switch must pass the RMS/ratio limits, with at least 200 authenticated packets (499 observed), which requires the PBX to decrypt the caller's new-key stream. The caller's identical gate shows continued inbound media under the unchanged PBX key. Earlier SRTCP/RTP packet negatives, the three initial 488 offers and the two TLS identity negatives were repeated.

## Verification and provenance

- Fresh explicit copy (`prepare_secure_call.py rekey-final2-work`): `cargo fmt --check`, frozen workspace tests (10 CLI, 12 media, 39 packet-adapter, 53 telephony, 6 registration, 1 CLI and 4 compile-fail doctests), all-target clippy with warnings denied and the release lab build passed offline in the pinned toolchain container.
- Exact graph before and after compilation: the reviewed 100 registry identities, 195 notices and 4548 archive source files are unchanged.
- Separate observer: image/ELF identity (`cf7aad4a2ec73db379aaac5943f68c3f7b804024173040b564e94ab1fbccd6ee`, checked inside the probe container), internal network without published ports, runtime hardening, two channels/one call, zero calls/channels/contacts before shutdown and zero project containers/networks afterward.
- Synthetic Docker-context positive/negative controls passed.
- Claude CLI static review (tools, MCP, slash commands and session persistence disabled): [round 1](../reviews/claude-rekey-call-2026-10-13.md#round-1) BLOCK on the "index zero" wording and an ungated post-switch tone claim; round 2 BLOCK on an ungated rollover claim. Each was corrected with a rebuilt, re-pinned and re-observed candidate, and the rollover is now gated per source; round 3 APPROVE. Static review is not runtime acceptance.

## Known limitations

- Only a caller-initiated rekey is exercised. A PBX-initiated re-INVITE is still answered 488, and a changed PBX key in an answer fails the call instead of opening a new receive epoch.
- The PBX-side RTCP schedule allows only one post-rekey report per call; the evidence is that single echoed block.
- `rsipstack` 0.7.3 emits re-INVITE bodies at `debug` tracing level. The probe installs no tracing subscriber; the application must never enable that level with SDES bodies.
- The old key's erasure covers only own bytes; SDK/provider copies remain unproven (ADR 0011).

## Open gates

Secure DTMF/hold/transfer/renegotiation, PBX-initiated rekey, multi-source lifecycles, production supervision, human/device audio, UI, persistence, CLI/MCP and all release gates remain open. Task 1.7 stays unchecked; OpenSpec is still **4/54**.
