## Context

Explicit choice: full web calls in a later release, not remote desktop control. See [proposal.md](proposal.md), [spec](specs/web-wasm-softphone/spec.md), and [Asterisk WebRTC deployment reference](https://docs.asterisk.org/Configuration/WebRTC/Configuring-Asterisk-for-WebRTC-Clients/). Linux desktop is not yet implemented beyond its scaffold.

## Goals / Non-Goals

**Goals:** shared domain/visual language, zero client installation, browser-specific contract/interoperability.
**Non-Goals:** blindly compiling Linux adapters to WASM, browser UDP/TCP, guaranteed background capture, WASM SIP server, first-release mobile/Safari certification, or claiming Rust-only integration without JS.

## Decisions

### Rust/WASM and browser boundaries
apps/web uses candidate egui/eframe web and OS-independent core; wasm-bindgen/web-sys bridge browser APIs. platform-web implements themes/lifecycle/permissions/non-secret storage; capabilities explicitly lack tray/keyring/native IPC. Share pure domain/UI, never bundle Tokio/CPAL/rusqlite/Secret Service/Unix IPC. Do not freeze domain around a specific networking stack.

Candidate full-framework SIP.js handles SIP/WSS signaling/WebRTC session description behind a minimal typed JS bridge, pinned/audited in build. Own UI/rules remain Rust/WASM; WebRTC/codecs/capture are browser APIs, not ported desktop RTP/SDES. Do not claim an all-Rust browser stack. Before complete UI, GW0 proves registration/incoming/outgoing/G.711/DTMF/hold re-INVITE/REFER-NOTIFY in both browsers. On candidate failure review proposal/ADR, never silently omit functionality or depend on native app.

### Telephony infrastructure
HTTPS assets; SIP/WSS directly to enabled Asterisk/WebRTC or operator gateway, never browser UDP. WebRTC uses ICE/DTLS-SRTP with RTP/SAVPF/rtcp-mux, not desktop SDES. PBX anchors media/interoperates with G.711 endpoint; prefer PCMA/PCMU where negotiable, allow browser Opus with explicitly audited PBX conversion. PBX/gateway supplies valid codecs; server resources are not bundled.

Separate lab from desktop defaults: exclusive WebRTC/WSS/TLS/ICE/DTLS override, isolated host-only test VMs, explicit ACL, no PSTN. Never copy public binds from examples. Trust lab CA only in test profiles/VMs. WEB-05 uses pinned coturn, locally generated ephemeral credentials, isolated routed-NAT VMs blocking direct media to prove relay. No default-runner privilege increase. Operator owns production domains/certificates/WSS/media/TURN; no open relay. Supported deployment requires WSS reverse proxy validating Origin allowlists before PBX forwarding, approved WSS destination allowlists/rate limits, no public AMI/ARI/admin. UI HTTPS does not eliminate SIP authentication/abuse exposure.

### Credentials, data, security
Public configuration contains non-secret endpoints/features/defaults; build dotenv is no vault. Form-provided account/password stay in session memory, reconnection only while session is active; logout releases tracks/attempts BYE/unregister. No remember-password feature. JS/WASM cannot guarantee zeroization; best-effort reference removal, no physical-memory-erasure promise. Disable internal SIP.js/SDP/Authorization logging; redact before console/telemetry. WEB-04 checks console/debug buffers, not just stores. Operator supplies short-lived session TURN credentials. If TURN is needed, authenticated external temporary-credential issuer/generator is required infrastructure. Lab operator generates a temporary coturn ticket for the session form, never browser master secret; production can integrate an existing authenticated issuer outside bundle, never client master keys. No mandatory application backend; operator WSS proxy is required, relay/provisioning/gateway are external services when needed.

IndexedDB preferences; opt-in consent for local contacts/history with origin/device-storage warning, no implicit sync. Private mode/quota/eviction degrade to session with warning. No SQLite WASM initially: no contract advantage and extra infrastructure; SQLite remains native-only. Sanitized preview/import/export without credentials; preserve domain CSV limits.

No third-party scripts/analytics/CDN. Escape names/labels. Restrictive CSP connect-src for approved endpoints; expected wasm-unsafe-eval for WASM loading must be justified in ADR/tested against actual bundle, no broad unsafe-eval. frame-ancestors none, Permissions-Policy microphone=(self), camera=(). HTTPS example includes security headers, real-bridge/bundle CSP tests. No first-release service worker/PWA; calls require network. Do not claim credential protection against XSS/compromised origin; mitigate with CSP/pinned dependencies.

### Permissions, devices, lifecycle
Prepare-audio action explains capture/requests microphone; otherwise request on answer/dial. Never capture just from load/registration. Ring/playback may need a gesture; UI requests audio enablement/warns about limited incoming calls. getUserMedia/enumeration depend on permission; setSinkId output selection optional with browser-default fallback, no uniform-selection promise. Stop tracks after tests/calls; reuse only with consent/active indicator. Hotplug never silently switches capture.

BYE on close/crash is best-effort, not guaranteed; PBX timers/reconciliation mark interrupted, never fictitiously answered calls. No guaranteed background incoming, autostart/tray/mandatory OS notifications. Resume reconciles registration/state, never redials. One active profile tab through Web Locks where available in supported browsers; competing tabs are read-only/warn without registering. Crash releases lock, never automatically takes over a call.

### Build and validation
Pinned Docker builder contains Rust wasm32, pinned bundler, JS lockfile if SIP.js is selected; no host Node/Rust beyond Docker. Self-hosted HTML/JS/WASM/fonts/icons bundle with manifest/checksums/SBOM/licenses, not “one web binary.” Initially test without mandatory COOP/COEP; threads/shared-memory needs require prior deployment/gate review. No universal-bundle or ELF latency/RSS promise.

Initial web matrix: stable Chromium/Firefox on Ubuntu 24.04 desktop, exact acceptance versions frozen; real headset/second human endpoint, five minutes per agreed codec, separate synthetic integration. GW0 resolves UI/rendering/WASM/bridge/TLS/permissions/G.711/hold/transfer/CSP/bundle. Web release only after GW0 and WEB-01..06; Claude reviews proposal then results, never substitutes calls.

## Risks / Trade-offs

- [Legacy PBX lacks WebRTC] → explicit compatible PBX/gateway, server cost/operation outside client.
- [Browser suspends tabs/autoplay] → gesture/prepare-audio, visible limitations, no permanent-softphone promise.
- [JS signaling alongside Rust/WASM] → isolated bridge, version/license audit, GW0, transparent dependency.
- [NAT requires relay] → forced TURN tests/infrastructure guide, no universal NAT claim.
- [Compromised origin accesses credentials] → session memory, CSP, no third parties/browser-vault claims.

## Migration Plan

Complete Linux/CLI/MCP first. Approve GW0; create web target/adapters; validate PBX/browsers; implement UI/data; build/deploy; real tests/review. Atomically publish versioned assets, compatible-schema bundle rollback, no native-data migration. Never use personal settings/production PBX in tests.
