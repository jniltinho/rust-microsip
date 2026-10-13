# ADR 0011: Isolated initial TLS/SDES calls

Status: **experimental initial-call proof, not application adoption or G0 approval**.

## Decision

Connect the [per-SSRC packet adapter](0010-isolated-srtp-packet-adapter.md) to a
strict initial SDES subset and the disposable Asterisk PBX. Keep the experiment
under `tests/probes/`, outside application manifests, the application lockfile,
and its Docker build context. Explicit copying is required to compile it.

The selected Rust graph remains the previously reviewed 100 registry packages
and 195 notices, with identical archives, metadata features and actual Linux
normal/build/dev features. Direct `ring = 0.17.14` and `base64 = 0.22.1` reuse
existing reviewed nodes; base64 has defaults disabled and `std` enabled.
The [isolated lock](../inventory/g0-secure-call.Cargo.lock) is not an application
lock update. No new cipher/provider, native dependency or SDK feature is adopted.

## Narrow contract

- Admit only fictional 1201/1202 TLS fixtures at the same disposable registrar.
  Install only a TLS client transport, with explicit CA/SNI and no custom
  permissive verifier. Incoming transactions must use the PBX TLS connection;
  UDP/TCP listeners and signaling fallback are absent.
- Generate fresh, independent thirty-byte local master material with ring's
  OS-backed `SystemRandom`. One atomic admission owns one initial dialog per key;
  each local key moves once into one non-cloneable transmitter. The PBX's offer
  or answer supplies the corresponding receive key. SDES's negotiated tag,
  directional keys and protected signaling are described in
  [RFC 4568 §§5.1 and 8.3](https://www.rfc-editor.org/rfc/rfc4568.html#section-5.1).
- Parse at most 8192 SDP bytes, 128 lines and 512 printable ASCII bytes per line.
  Require one media-level crypto line, `RTP/SAVP`, AES-CM/80, a thirty-byte inline
  key, matching answer tag and the PBX's address/media range. The fixed incoming
  fixture accepts tag 1. Reject duplicate/session-scoped crypto, AVP downgrade,
  lifetime/MKI/session parameters, other suites, extra media, unsupported
  directions, explicit RTCP negotiation, ICE and DTLS. This deliberately narrow
  initial-call subset is **not a general SDP implementation**.
- On the media worker, authenticate/decrypt before decoding; enforce the
  negotiated socket, codec and 160-sample payload. The fixed stream crosses the
  sender's sequence rollover. No encryption, allocation or parsing runs on an
  audio-device callback.
- Require actual INVITE/200/ACK, remote-tone measurements, BYE/200, dialog
  termination and two unregistrations under a finite deadline. Reports' success
  flags are populated only after these waits/checks, not on a 200 INVITE alone.
  Wire flags rely on the single admitted dialog; general dialog correlation is
  still production work.
- The TLS negative oracle accepts only typed rustls `UnknownIssuer` or
  `NotValidForName` errors from the transport lookup. It does not construct a SIP
  endpoint or reach REGISTER/INVITE. Connection-refused, expired-certificate and
  unrelated signaling errors do not satisfy those negative cases.

## Evidence and remaining gates

The [receipt](../receipts/g0-secure-call-2026-10-13.md) binds the copied source,
lock, ELF, runtime images, measurements, negatives and cleanup observations.
Asterisk terminates and re-encrypts the anchored media; this is **hop-by-hop**,
not end-to-end secrecy from the PBX. Both Rust endpoints run in one process with
independent sockets and keys. The separate observer is a reproducible local
process, not an independent third-party lab or packet capture.

Only five-second initial G.711 **RTP** interoperability is claimed. SRTCP
interoperability/report routing, bounded retained authenticated SSRC banks,
pre-authentication rate limits, rekey/ROC continuity, secure DTMF/hold/REFER-NOTIFY,
renegotiation and production task/parser/channel supervision remain mandatory
before adoption. Unsupported re-INVITEs receive 488 without resetting security
state; that refusal does not fulfill secure hold/transfer requirements. The
existing plaintext-control probes do not fulfill secure-control coverage.

Own key arrays and some live temporary buffers use `Zeroizing`, but full erasure
is **not** established: own String/credential allocations and reallocations,
SDK SDP/credential/provider copies, decoded audio and allocator history can
retain data. No key-erasure, compiler constant-time or whole-source/vulnerability
audit is claimed. The mandatory production boundaries in ADRs 0004/0009/0010
remain open. Human voice, actual audio devices, UI, persistence, CLI/MCP controls,
packaging/VM validation and every usable-release gate remain required.
