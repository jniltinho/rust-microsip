# Initial TLS/SDES call receipt — 2026-10-13

**Result: isolated initial encrypted-RTP interoperability passed; not application adoption, G0 or a usable release.**

Baseline: `cb5e17abcb6a20ecfed78be93adb12eaa0447979`. The [machine receipt](g0-secure-call-2026-10-13.json) binds 38 copied compilation inputs, the separate lock, source/lab/observer hashes, actual runtime images, ELF, numeric measurements and log hashes. Application manifests/lock/runtime remain unchanged. [ADR 0011](../adr/0011-isolated-initial-tls-sdes-calls.md) defines the narrow contract; [reproduction](../secure-call-experiment.md) uses explicit copies and offline pinned containers.

## Actual PBX execution

Four five-second calls used fictional mandatory, non-optimistic SDES 1201/1202 fixtures on the pinned andrius/asterisk 22.11.0 lab. Two independent Rust endpoints run in one process, with different sockets, tones and fresh OS-backed directional keys. Signaling validates the PBX CA/identity over TLS; media is anchored and re-encrypted by Asterisk, **not end-to-end hidden from it**.

| Codec | Initiator direction | Sent / received per endpoint | Minimum decoded RMS | Minimum remote/own tone power |
| --- | --- | --- | --- | --- |
| PCMA | first → second | 250 / 250 | 5665.93 | 1670.00 |
| PCMA | second → first | 250 / 250 | 5665.93 | 1670.00 |
| PCMU | first → second | 250 / 250 | 5635.86 | 1626.65 |
| PCMU | second → first | 250 / 250 | 5635.86 | 1626.65 |

All endpoints had zero measured loss/late packets. Each call verified INVITE/200/ACK, different remote tones, BYE/200, dialog termination and authenticated unregistration. Sender sequence rollover crossed during each encrypted stream. Outgoing packets had the expected authentication overhead and encrypted payload; Asterisk returned authenticated/decryptable packets with the remote tone. This is actual peer interoperability, not just the adapter's self-roundtrip, but is not an independent cipher-conformance or packet-capture audit.

Each endpoint used an actual first PBX packet for wrong-key, tag/sequence-tamper and replay negatives; it also refused a clear valid-shaped packet. Failed pre-authentication attempts left no learned SSRC. Three real offers (AVP without crypto, SAVP without crypto, unsupported-only suite) received 488; the callee inspector observed **no INVITE**, preventing its own rejection from satisfying the PBX-policy oracle. Unrelated valid CA and wrong hostname failed with the exact typed rustls certificate category before constructing any SIP endpoint. Unrelated signaling, refused connections and expired-certificate errors cannot satisfy that oracle.

## Verification and provenance

- Workspace: 10 CLI integration, 12 existing media, 29 packet-adapter, 41 telephony (13 new), 6 registration integration tests; 1 CLI and 3 non-Clone compile-fail doctests. Formatting, frozen checks and all-target clippy with warnings denied passed.
- TDD: the initial valid-SDES test actually failed with a stub (one test, exit 101). Three deliberate downgrade/tag/session-parameter mutants each failed its exact assertion with exit 101; sources were restored and the full suite passed afterward.
- Exact graph: the prior reviewed 100 registry identities, archive hashes, metadata/actual Linux target features and 195 notice hashes are unchanged. A bounded archive/cache comparison verified all 4548 regular source files before and after the fresh-copy compilation. This is identity/inventory evidence, not whole-source, cryptographic, vulnerability or legal approval.
- A freshly prepared, pinned explicit copy passed the same workspace checks and produced the identical lab ELF: `97837b7bb344a4981c5ea9ec532b52962cb782903ce9f99f1c03e3ff5f5c8461`. This is one same-toolchain reproduction, not a cross-toolchain/VM or release reproducibility claim.
- A separate local process observed two active PBX channels/one call, actual image/ELF identity, internal networking, no published ports and runtime hardening (including zero effective capabilities and no-new-privileges). Both endpoints were mandatory SDES/non-optimistic. It observed zero channels/calls/contacts before shutdown and zero project containers/networks afterward. It is not a third-party test lab.
- All three Python preparation/verification helpers refuse optimized Python. Exact post-modification pins, extra-source/config rejection and package-set equality prevent silently widening the compiled candidate. The observer intentionally uses the fixed candidate directory and a stricter outer deadline.
- Synthetic Docker-context positive/negative controls passed: research probes and fake private sentinels stay excluded from the application context; a permissive filter fails specifically with `CONTEXT_REJECTED`. No real local reference directory was copied or inspected.
- Claude CLI [round 1](../reviews/claude-secure-call-round1-2026-10-13.md) returned BLOCK; provenance and the TLS oracle were corrected. [Round 2](../reviews/claude-secure-call-round2-2026-10-13.md) approved with hardening notes; those were addressed before final [round 3](../reviews/claude-secure-call-round3-2026-10-13.md) static APPROVE. Hooks/tools/MCP/session persistence were disabled; verdicts are preserved, not converted into runtime acceptance.

## Open gates

Initial RTP only. SRTCP interoperability, retained authenticated source banks, pre-authentication rate limits, rekey/ROC continuity, secure DTMF/hold/transfer/renegotiation and production supervision remain mandatory before adoption. One SSRC per receiver is not a complete call owner. Own/SDK allocation erasure is unproven. The existing plaintext-control probes do not cover secure controls.

Human/device audio, UI, persistence, CLI/MCP, exact release-artifact/VM testing and all usable-release gates remain open. No personal profile, installed AppImage, audio device or production account was accessed. No executable/key/cache was added to Git. Tasks 1.7/5.4 remain unchecked; OpenSpec is still **4/54**, without narrowing acceptance criteria.
