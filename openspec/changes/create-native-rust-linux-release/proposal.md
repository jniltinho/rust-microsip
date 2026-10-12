## Why

Create a native Rust Linux softphone with MicroSIP's visual simplicity, complementary GOnnect telephony/desktop references, simple installation, and persistent local configuration. The first release must make real calls with reliable audio, not merely reproduce screens or repackage the existing Qt application.

## What Changes

- Create `rust-microsip` v0.1.0 initially for Linux x86_64, Ubuntu 24.04, GNOME/Wayland and KDE/X11; no support promise for other environments in this release.
- Use native Rust UI (egui/eframe as candidate), with PhotoCraft as an architectural reference, without Electron, Tauri, webview, or Qt runtime.
- Create a compact window with Dialpad, Calls, and Contacts tabs, local search, contact favorites, call controls, and MicroSIP-inspired settings; offer light/dark/system themes. Explicit scope exceptions, no falsely functional buttons.
- Deliver one enabled SIP account and one simultaneous call, UDP/TCP/TLS, registration, incoming/outgoing/reject/hangup, DTMF, mute, hold, DND, and blind transfer.
- Include RTP/G.711 PCMA/PCMU media, device selection, reconnection, and secure TLS + SRTP/SDES; do not promise universal NAT.
- Offer TOML or explicit `.env`, settings UI, and embedded SQLite with explicit precedence/protected credentials, never plaintext database passwords.
- Maintain local/CSV contacts, favorites, searchable history, notifications, optional tray, and user-confirmed `tel:`/`sip:` links.
- Plan an executable self-contained in application resources, additional AppImage, Docker build, and Docker Asterisk lab with fictional extensions.
- Organize the Rust workspace by domain, UI, telephony, media, configuration, storage, and platform contracts with a Linux adapter; require documentation, tests, and Claude CLI review.

### Non-goals

Windows/macOS/mobile/web delivery in this release; complete MicroSIP/UC parity; simultaneous multiple calls/accounts; conference, attended transfer, video/Jitsi, RTT, chat, LDAP/CardDAV/CalDAV/EDS/M365, BLF, headset HID/busylights, remote provisioning, and automatic updates. Do not promise emergency calls, recording, or certification. Never silently migrate installed-application data/settings.

## Capabilities

### New Capabilities

- `native-desktop`: compact MicroSIP-inspired Linux UI on essential screens, themes, and honest states.
- `sip-telephony`: registration, call lifecycle, DTMF, hold, DND, transfer, recovery.
- `audio-media`: two-way audio, devices, RTP, codecs, secure media.
- `config-storage`: TOML/env/SQLite precedence, editing, migrations, secret protection.
- `contacts-history`: contacts, CSV, favorites, search, history, retention.
- `linux-integration`: notifications, optional tray, URI, single instance, shutdown.
- `reproducible-distribution`: binary, Docker build, Asterisk lab, tests, safe publication.

### Modified Capabilities

None; new project without previous specs.

## Impact

New independent `jniltinho/rust-microsip` repository without publishing `base/`. A technical gate must prove Rust/SIP/media/audio and packaging before the complete UI. No PBX backend is embedded: Asterisk is disposable test infrastructure; normal use depends on an accessible SIP server. Sources, boundaries, and reference-AppImage identity are in [reference-sources](../../../docs/reference-sources.md). This proposal supplies planning; executable code/infrastructure are created only through approved implementation.

## Linked roadmap

macOS/Windows are future targets prepared through a `platform` crate, not supported in v0.1. [CLI/MCP](../add-agent-cli-mcp/proposal.md) is a requested agent extension planned for the Linux candidate after G0; combined approval/release requires both changes' gates. [Web/WASM](../add-web-wasm-softphone/proposal.md) will make full WebRTC calls in a later release, per user choice; it neither replaces Linux ELF nor reuses native SIP sockets in browsers.
