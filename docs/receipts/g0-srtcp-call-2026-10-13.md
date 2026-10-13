# TLS/SDES RTP+SRTCP call receipt — 2026-10-13

**Result: isolated SRTCP interoperability passed; not application adoption, secure controls, G0 or a usable release.**

Baseline: `48906271f4b624853c19d0befb34f7482b65d94c`. The [machine receipt](g0-srtcp-call-2026-10-13.json) binds 39 copied compilation inputs (the previous 38 plus `rtcp.rs`), harness hashes, actual runtime images, ELF, numeric measurements and log hashes. It extends the [initial TLS/SDES receipt](g0-secure-call-2026-10-13.md) under the same narrow [ADR 0011](../adr/0011-isolated-initial-tls-sdes-calls.md) contract. Application manifests/lock/runtime remain unchanged.

## Actual PBX execution

Four twelve-second calls (PCMA/PCMU, both initiator directions) used fictional mandatory, non-optimistic SDES 1201/1202 fixtures on the pinned Asterisk lab over validated TLS. Each endpoint binds an even RTP port and the contiguous RTCP port.

| Measurement, per endpoint and call | Value |
| --- | --- |
| RTP sent / received | 600 / 600, zero lost or late |
| Minimum decoded RMS | 5635.86 |
| Minimum remote/own tone power ratio | 1626.65 |
| SRTCP sender reports sent / peer reports received | 12 / 2 |
| Peer reports echoing our sender report (LSR) | 2 |
| Peer-reported highest sequence (crosses rollover) | 65999 |
| Retained authenticated remote sources | 1 |

Each endpoint also verified SRTCP packet negatives against an actual PBX report: tampered tag, tampered index, cleared E-bit, clear valid-shaped packet and replay were all rejected. The three 488 offer negatives (plaintext, missing crypto, unsupported suite; callee saw no INVITE) and the two typed TLS identity negatives (untrusted CA, wrong hostname; no SIP endpoint constructed) were repeated. Media is anchored and re-encrypted by Asterisk, not end-to-end hidden from it.

## Verification and provenance

- Fresh explicit copy (`prepare_secure_call.py srtcp-call-work`): `cargo fmt --check`, frozen workspace tests (10 CLI, 12 media, 39 packet-adapter, 52 telephony, 6 registration, plus 1 CLI and 4 compile-fail doctests), all-target clippy with warnings denied and the release lab build passed offline in the pinned toolchain container.
- Exact graph before and after compilation: the reviewed 100 registry identities, 195 notices and 4548 archive source files are unchanged.
- Separate observer process: actual image/ELF identity (`5427e8e5a5eae777623e5045d812ef09b9a1c61e83ce79ac29a01f9abf3f7f12`), internal network with no published ports, runtime hardening, two active channels/one call, zero calls/channels/contacts before shutdown and zero project containers/networks afterward.
- Claude CLI static review (tools, MCP, slash commands and session persistence disabled): [round 1](../reviews/claude-srtcp-call-2026-10-13.md#round-1) BLOCK, corrected scope and an overclaiming observer key; round 2 APPROVE. Static review is not runtime acceptance.
- Synthetic Docker-context positive/negative controls passed with `rtcp.rs` added to the probe sentinels.

## Known limitations from static review

- Peer LSR/rollover values come from PBX reports decoded by the probe under test; the observer reads the probe's JSON and does not query Asterisk RTCP statistics.
- SRTCP negatives use exact typed errors for tag, index and E-bit; replay and the clear packet are asserted as rejected. All are reported as one aggregated `packet_negatives_verified` flag, and the source count is not rechecked after the replay/clear cases.
- `ReceiverBank` closes the whole epoch on a provider anomaly, including one from a temporary unknown-source context. Forged packets cannot reach that path without the key (authentication failures map to `Authentication`); a production owner must still decide this policy explicitly.
- The probe aborts on an off-size datagram from the negotiated PBX address before authentication. This is lab-only behavior, not production DoS handling.
- The `!Clone` compile-fail doctests do not pin an error code.

## Open gates

Rekey/ROC continuity across re-INVITE, secure DTMF/hold/transfer/renegotiation, multi-source lifecycles beyond one retained source, production supervision and own/SDK key-erasure remain mandatory before adoption. Human/device audio, UI, persistence, CLI/MCP and all release gates remain open. Task 1.7 stays unchecked; OpenSpec is still **4/54**.
