## Why

Users need calls without client installation. The user confirmed a complete web softphone in a later release, not a remote panel dependent on the installed Linux app.

## What Changes

- Rust/WASM compact desktop-like UI, light/dark/system themes, local contacts/history.
- Browser WebRTC audio and SIP/WSS to a compatible PBX, minimal web-API bridge.
- One account/call, dial/incoming/answer/reject/hangup/DTMF/mute/hold/DND/confirmed blind transfer per interoperability gate.
- HTTPS static assets, no native client/extension/plugin installation.
- Document WebRTC PBX/separate legacy-SIP gateway, needed STUN/TURN, background-call limits.
- In-memory per-session credentials, non-secret web storage, no silent native config/SQLite import.
- Separate Docker build/deployment and Asterisk WebRTC lab, browser/real-voice tests.

## Capabilities

### New Capabilities

- `web-wasm-softphone`: installation-free web softphone, WebRTC/WSS, permissions, privacy, operation, distribution.

### Modified Capabilities

No existing canonical specs. Separate contract without reducing desktop requirements.

## Impact

Later than Linux v0.1/CLI/MCP; depends on shareable domain/platform contracts and its own WebRTC gate. No promise of browser UDP/TCP SIP, native SQLite/keyring/tray, permanent listening, or offline calls. Web assets are multiple files, not one ELF. Deployment needs HTTPS/access to PBX/gateway; no WASM SIP server. Native macOS/Windows remain separate roadmap. No web/deployment code implemented at planning time.
