# Architecture

rust-microsip is a Cargo workspace split into layers with one-way dependencies. The domain layer knows nothing about sockets, devices, files or the UI toolkit, so the telephony engine, audio, storage and UI can each be replaced or ported without touching the others.

```
apps/rust-microsip (binary + egui UI)
 ├── crates/config     CLI, environment, .env, TOML -> EffectiveConfig
 ├── crates/storage    SQLite history, contacts, CSV import, backups
 ├── crates/audio      cpal device bridge (ALSA today) -> MediaIo
 └── crates/telephony  engine: SIP (rsipstack), SDP, RTP/SRTP media
       └── crates/media    G.711, RTP header, RFC 4733 DTMF, SRTP adapter
all of the above depend on
 crates/core           domain types, state machine, Command/Event, MediaIo
```

| Crate | Responsibility | Platform-specific code |
| --- | --- | --- |
| `rust-microsip-core` | Validated account settings and dial targets, DTMF keys, registration/call states and allowed transitions, redacted `Command`/`Event` contract, the `MediaIo` trait. No I/O. | none |
| `rust-microsip-media` | G.711 µ-law/A-law, RTP packets, RFC 4733 events, SRTP/SRTCP AES-CM-128/HMAC-SHA1-80 with replay protection. | none |
| `rust-microsip-telephony` | `engine::EngineHandle`: one account and one call on a dedicated Tokio thread; registration, calls, hold, DTMF, mute, DND, blind transfer, SDES, jitter buffer. The `g0_*` modules are the earlier isolated lab probes. | none (Tokio and rustls are portable) |
| `rust-microsip-audio` | `DeviceMedia` implements `MediaIo` with lock-free rings and 8 kHz resampling. | cpal backend: ALSA on Linux; cpal also provides CoreAudio and WASAPI |
| `rust-microsip-config` | Six-layer resolution with provenance, secret handling, atomic saves, sanitized doctor/export. | XDG paths and Unix permissions |
| `rust-microsip-storage` | SQLite schema/migrations, call history, contacts, CSV import, online backups. | Unix permissions |
| `rust-microsip` (app) | Command-line front end and the egui/eframe window; wires config, audio and engine. | none beyond eframe/wgpu |

## Runtime model

- **UI thread**: eframe event loop. It only sends `Command`s with a non-blocking `try_send` (a full queue yields `Notice::Overloaded`) and drains `Event`s each frame. The engine wakes it with `request_repaint`.
- **Engine thread**: a two-worker Tokio runtime owns the SIP endpoint, dialogs, timers and per-call media tasks. A single actor serializes all state changes; spawned tasks report back over a bounded internal channel.
- **Media**: one task per call ticks every 20 ms: capture → encode → (SRTP) → send, and receive → authenticate → decode → bounded jitter buffer → play. RTP is accepted only from the negotiated address.
- **Audio devices**: cpal callbacks only move samples through preallocated SPSC rings; resampling runs on the media tick, never in a device callback.
- **Signals**: SIGTERM/SIGINT close the window through the normal path, so the engine ends calls and unregisters within a bounded shutdown.

## Security boundaries

- SDES keys travel only over validated TLS; SRTP accounts reject plaintext offers (488) and a TLS account listens on no UDP port.
- Requests are accepted only from the configured registrar/proxy address over the configured transport.
- Passwords are never accepted as command-line arguments, never written by the application, and redacted from every `Debug` output; secret-bearing files must be private and owned by the user.
- A configured CA bundle replaces the system roots; if it cannot be read safely, the account is not registered.

See [ADR 0012](adr/0012-production-dependency-graph.md) for the dependency graph, licences and native libraries, and the [production engine receipt](receipts/production-engine-2026-10-13.md) for runtime evidence.

## Porting and UI changes

- **macOS/Windows**: core, media, telephony and storage are portable Rust. Porting needs the cpal backend for the platform (already provided by cpal), platform paths/permissions in config, and packaging. Nothing in the engine depends on Linux APIs.
- **Another UI toolkit** (for example Slint or a webview shell): implement the same `Backend` contract — send `Command`, poll `Event` — and keep the reducer in `apps/rust-microsip/src/ui/state.rs` as the model. The engine, audio and storage do not change.
- **Agent CLI/MCP and web**: separate OpenSpec changes reuse `core` and the engine through the same command/event contract.
