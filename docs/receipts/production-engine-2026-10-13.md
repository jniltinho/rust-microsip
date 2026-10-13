# Production engine and desktop slice receipt — 2026-10-13

**Result: the first production slice (domain contracts, telephony engine, ALSA audio bridge, configuration, egui desktop) builds from a clean Docker context and passed end-to-end calls against the disposable Asterisk lab. This is not a release: VM/desktop, human audio, persistence, IPC, AppImage and the remaining release gates are open.**

Baseline: `eed96d84638cc46a7cae30b3980ca8c3781a440e`. Dependency boundary: [ADR 0012](../adr/0012-production-dependency-graph.md) and [production graph inventory](../inventory/production-graph.json).

## What was built

| Crate | Role |
| --- | --- |
| `rust-microsip-core` | Validated account settings, dial targets, DTMF keys, call/registration state machine, redacted commands and events, `MediaIo` boundary. No I/O. |
| `rust-microsip-telephony::engine` | One-account, one-call engine on a dedicated Tokio thread: UDP/TCP/TLS registration with renewal and bounded backoff, outgoing/incoming calls, CANCEL, reject, DND (486), PCMU/PCMA, RFC 4733 DTMF, hold/resume, mute, blind REFER with correlated NOTIFY, PBX re-INVITE answers (including a peer key change), optional or mandatory SDES/SRTP without downgrade, bounded jitter buffer, bounded event backlog that never sheds call snapshots. TLS uses the system CA bundle unless a custom CA is configured. |
| `rust-microsip-audio` | cpal/ALSA capture and playback with lock-free SPSC rings, windowed-sinc resampling to 8 kHz, drop/underrun counters; falls back to silence with a visible notice. |
| `rust-microsip-config` | CLI > env > explicit `.env` > TOML > defaults with provenance, strict permissions for secret-bearing files, atomic 0600 saves, sanitized `--doctor`/`config export`, passwords never accepted as arguments. |
| `rust-microsip` (app) | MicroSIP-like 300x460 egui window (Phone/Calls/Contacts, keypad, Call/End, DND/MUTE/HOLD/XFER, incoming banner, settings, themes, About/licenses) wired to the engine. |

The SRTP packet adapter proven in the isolated experiments was adopted unchanged into `rust-microsip-media`.

## Runtime evidence

`crates/telephony/examples/engine_lab.rs` drives two real engines (test `MediaIo` with 440/880 Hz tones and Goertzel meters) through the lab PBX, once per profile. Final run after the review fixes: log SHA-256 `7cb7292ee3cf66bc4b16b7c2d864340cede8d5fe3ab157f5477b53ccfca33feb`; example ELF `a0d0ccd10df7803438f4108945077e31174954478986c87ad7eada4b69fe6d13`.

| Step | UDP / RTP (1001→1002) | TLS / SRTP (1201→1202) |
| --- | --- | --- |
| Register, ringing event, answer | passed | passed, both calls `secure` |
| Two-way audio before hold (frames, min ratio) | 100 / 1670 | 100–101 / 1656 |
| Hold: frames heard by the holder | 0 | 0 |
| Resume (frames, min ratio) | 100 / 1670 | 100 / 1670 |
| Mute: RMS heard by the peer | 8.0 (silence keeps flowing) | 8.0 |
| Blind transfer to missing 699 | final NOTIFY 404, call kept, audio after (75 frames, ratio 1768) | same |
| Blind transfer to 602 | caller ended `Transferred`; transferee hears 1 kHz (100 frames, ratio vs former partner 1798) | same |
| DTMF `123` to IVR 601 | 1 kHz branch (90 frames, ratio 2.8 × 10⁷) | same |
| Reject / caller cancel / DND | Busy / Canceled on both sides / Busy | same |
| Bounded shutdown | both engines stopped | same |
| TLS negatives (untrusted CA, IP-literal server, PEM without certificates) | — | each `Failed(Certificate)`, never registered |
| Wrong password | `Failed(Unauthorized)`, exactly one REGISTER attempt in 15 s (no automatic retry) | — |
| Second incoming call while busy (third party on a **TCP** account) | refused with 486 (`Busy` at the caller), never shown to the busy callee; the active call kept two-way audio (76 frames, ratio ≥ 1661) while DND was switched on mid-call | — |
| Reverse direction (B calls A, A answers and hangs up) | two-way audio, callee sees `Normal` | same |
| CANCEL racing the callee's 200 OK (3 runs) | no call left on either side; caller `Canceled`, callee `Normal` | same |

Two engine defects found by these runs were fixed before this receipt: a jitter buffer that discarded a new RTP source after a PBX bridge change (now restarts on SSRC change or a long late streak, with a unit test), and test-harness races (mute criterion, DND end detection).

**Real application smoke.** The release ELF (`ea40ef0cfacb85395b5204ea29fc683a6a8ac85a5df2fc6ba4593a973df6b093`, DT_NEEDED: `libasound.so.2`, `libgcc_s.so.1`, `libm.so.6`, `libc.so.6`, loader) ran under a private Xvfb (llvmpipe, no host display/devices/network) on the lab network with a fictional 1001 UDP account. The PBX itself reported the contact registered, **one active call** while the window showed "Out 600 · Connected · Unencrypted (RTP)", and zero channels after Esc hung up. On SIGTERM the window closed through the normal path and the process exited 0 after unregistering (the PBX no longer listed the contact). No sound card exists in the container, so the app used its silent fallback; device audio was not tested. The captures exposed and led to fixing a clipped DND/MUTE/HOLD/XFER row during calls.

## Independent review

Three Claude CLI static review rounds (tools, MCP, slash commands and session persistence disabled) are recorded in [the review record](../reviews/claude-production-engine-2026-10-13.md). Round 1 blockers were fixed before the final runs above: a TLS account no longer listens on UDP and requests are accepted only from the configured registrar address over the configured transport; answers carry only the negotiated codec (and telephone-event only when offered); a configured CA bundle is read with size/type/owner/permission checks and failure never falls back to the system roots. Other fixes from the reviews: RTP inactivity and SRTP failures end the call, CANCEL and transfer deadlines, re-INVITE without SDP, section-aware SDP parsing with group-address rejection, stricter dialog correlation, hold answer directions, display-name and caller-identity sanitization, and config files owned by another user are refused. Static review is not runtime acceptance.

## Build evidence

`sh packaging/check-context.sh` passed: the allowlisted clean context ran `packaging/verify.sh` offline (fmt, clippy `-D warnings`, all workspace tests, doctests, rustdoc `-D warnings`, 33 packaging tests, selected-tree deny rules, reviewed notice collection, reproducible `Cargo.lock`), built the workspace in release and exported the ELF with notices, build manifest, SBOM and `SHA256SUMS`; the permissive-filter negative control still fails with `CONTEXT_REJECTED`.

## Not established

Clean GNOME/Wayland and KDE/X11 VMs, real headsets/devices and hotplug, human two-way voice, keyboard/screen-reader acceptance, persistence (SQLite history/contacts), Secret Service, single instance/IPC, signals/tray/notifications, AppImage, NAT/STUN, the CLI/MCP extension and all v0.1.0 publication gates. OpenSpec boxes are unchanged.
