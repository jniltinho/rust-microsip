# Secure call controls receipt — 2026-10-13

**Result: RFC 4733 DTMF, hold/resume and blind REFER/NOTIFY passed over TLS signaling with SRTP media against the lab PBX; not application adoption, PBX-initiated re-INVITE handling, G0 or a usable release.**

Baseline: `429ab0fd321d25dd7b0c8331d1d4c9e8b42a2d92`. The [machine receipt](g0-secure-controls-2026-10-13.json) binds 40 copied compilation inputs (a new `secure_controls.rs` probe module), harness hashes, runtime images, ELF, all control measurements, log hashes and the repeated RTP/SRTCP/rekey matrix. It extends the [rekey receipt](g0-rekey-call-2026-10-13.md) under [ADR 0011](../adr/0011-isolated-initial-tls-sdes-calls.md). The packet adapter and all application inputs are unchanged; the plaintext [DTMF](g0-rust-dtmf-2026-10-12.md), [hold](g0-rust-hold-2026-10-12.md) and [transfer](g0-rust-transfer-2026-10-12.md) probes still refuse SRTP fixtures. Their semantics were re-implemented over the secure endpoint with the same gates.

## Actual PBX execution

All cases use fictional mandatory, non-optimistic SDES fixtures 1201/1202 over validated TLS, PCMA and PCMU. Every media packet, including telephone events, is SRTP-protected with the call's key; received audio is accepted only after SRTP authentication.

| Control | Cases | Result |
| --- | --- | --- |
| RFC 4733 to IVR route 601 | `123` and `124` × 2 codecs | Answer listed and mapped PT 101 `telephone-event/8000`; 24 encrypted event packets per call (three digits, repeated end reports). The decrypted branch tone after the digits: 100 frames, RMS 5081.04–5101.15, expected/other branch power ratio ≥ 6.05 × 10⁶ (1 kHz for `123`, 400 Hz for `124`). |
| Hold/resume, 1201 holds 1202 | 2 codecs | Keyed re-offers `sendonly` (version 2) and `sendrecv` (version 3) with the **unchanged** local key: 200 with `recvonly`/`sendrecv`, same PBX key line and addresses, emitted ACKs for INVITE CSeq 3 and 4. While held, the holder sent 0 and accepted 0 authenticated frames after 250 ms settling; the peer kept sending (51 frames). Before and after: 100/100 frames per endpoint, RMS 5635.86–5675.25, ratio ≥ 1626.65. Sender and receiver SRTP state continued across both re-offers without reset. |
| Blind REFER from 1201, transferee 1202 | route 602 and missing 699 × 2 codecs | REFER 202; two `refer` NOTIFYs correlated by the armed dialog (an `Event` `id`, when present, must equal the REFER CSeq; the JSON does not record whether it was present) (one provisional) with terminal sipfrag **200** (602) or **404** (699). After 200, the transferee measured the destination: 100 frames, RMS 5097.92–5101.06, 1 kHz versus the **former partner's 440 Hz** ratio ≥ 1813.22. After 404, both original dialogs stayed confirmed and restored two-way audio measured 101/101 frames, ratio ≥ 1626.08. |

The earlier RTP/SRTCP matrix, mid-call plaintext re-offer 488, fresh-key/fresh-SSRC rekey, three initial 488 offers and two TLS identity negatives were repeated by the same binary.

## Verification and provenance

- Fresh explicit copy (`prepare_secure_call.py controls-call-work`): `cargo fmt --check`, frozen workspace tests (10 CLI, 12 media, 39 packet-adapter, 57 telephony, 6 registration, 1 CLI and 4 compile-fail doctests), all-target clippy with warnings denied and the release lab build passed offline in the pinned toolchain container. New unit tests cover NOTIFY correlation (foreign dialog, wrong event, provisional terminal, malformed sipfrag, post-final message, mismatched REFER id), the bounded ACK log, directed SDES parsing and telephone-event answers.
- Exact graph before and after compilation: the reviewed 100 registry identities, 195 notices and 4548 archive source files are unchanged; no dependency was added.
- Separate observer: image/ELF identity (`8cdd1dce439f83700c1b8a6510b3cbdac8010c3e2ec21f595d52f018507e5b36`, checked inside the probe container), internal network without published ports, runtime hardening, two channels/one call, zero calls/channels/contacts before shutdown and zero project containers/networks afterward. It asserts every control row above, not only the probe's own exit status.
- Synthetic Docker-context positive/negative controls passed with `secure_controls.rs` added to the probe sentinels.
- Claude CLI static review (tools, MCP, slash commands and session persistence disabled): [round 1](../reviews/claude-secure-controls-2026-10-13.md) APPROVE with documentation notes, applied here without changing the pinned candidate. Static review is not runtime acceptance.

## Known limitations

- `124` yields the 400 Hz branch, which this dialplan also uses for a timeout; the `123` success branch is the evidence that encrypted digits were recognized.
- The PBX re-INVITEd neither peer during hold or transfer; a PBX-initiated re-INVITE is still answered 488, and secure answering of one is not proven.
- No music-on-hold is configured; the peer's silence while held is not required.
- NOTIFY/ACK correlation is in-process SDK evidence, not an independent packet capture.
- Identical RMS/ratio values across windows are deterministic: the synthetic tone repeats every 50 frames and each 100-frame window covers two periods. Freshness of each window comes from SRTP authentication and replay rejection, not from those metrics.
- Both endpoints advertise REFER but an incoming REFER is not exercised; a production actor must reject one explicitly.
- The fixed observer timeout grew to 420 seconds for the longer matrix.

## Open gates

PBX-initiated re-INVITE/rekey, attended transfer, multi-source lifecycles, production supervision, own/SDK key erasure, human/device audio, UI, persistence, CLI/MCP and all release gates remain open. Task 1.7 stays unchecked; OpenSpec is still **4/54**.
