## Purpose

Enable complete browser voice calls without native installation, using Rust/WASM UI, secure web transport, and consented audio access.

## ADDED Requirements

### Requirement: Independent web access
The product SHALL load through HTTPS without installed Linux app/extension/plugin. It SHALL offer compact Dialpad/Calls/Contacts UI, light/dark/system themes, keyboard operation, incompatibility messages. Web release SHALL be later than and independent of Linux v0.1.

#### Scenario: WEB-01 clean browser
- **WHEN** a user opens HTTPS in a clean supported Chromium/Firefox Linux desktop profile without native client
- **THEN** WASM UI loads and allows session configuration/themes/keyboard navigation without software installation

### Requirement: Complete web telephony
Client SHALL register one SIP account through WSS and support one WebRTC DTLS-SRTP call, interoperating with compatible PBX/gateway. It SHALL support dial/incoming/answer/reject/hangup/DTMF/mute/hold/DND/confirmed blind transfer without insecure WS/RTP downgrade. It SHALL negotiate at least one browser-validated interoperable codec with PBX G.711.

#### Scenario: WEB-02 call and controls
- **WHEN** two lab sessions/extensions make/receive calls and exercise controls in supported browsers
- **THEN** PBX confirms registration/DTMF/hold/REFER-NOTIFY/DND, both participants hear five-minute voice, and real outcomes enter history

### Requirement: Consented audio and tab lifecycle
Client SHALL request microphone only after deliberate prepare-audio/answer/dial actions; it SHALL show active capture, handle denial/missing devices/hotplug, and stop unneeded tracks. It SHALL explain autoplay/suspended/closed-tab/notification limits; it MUST NOT promise permanent reception with closed browser.

#### Scenario: WEB-03 permission and suspension
- **WHEN** the user denies/revokes microphone, removes headset, closes/reloads/suspends a session tab
- **THEN** active UI shows errors/limits, no alternative capture starts silently, and resources close best-effort without automatic redial

### Requirement: Web credentials and storage
Client SHALL keep SIP passwords/TURN credentials only in session memory, never localStorage/sessionStorage/IndexedDB/URLs/logs/service workers/assets. It SHALL persist only preferences and explicitly opted-in local contacts/history with cleanup/sanitized export. It SHALL distinguish origin/profile-local data and never automatically import desktop SQLite/TOML/dotenv.

#### Scenario: WEB-04 storage and cleanup
- **WHEN** the user configures an account, calls, opts into local data, then ends session/clears storage
- **THEN** no password appears in storage/export/logs, capture/registration stop best-effort, and consented data can be removed without forensic-erasure/private-mode-persistence promises

### Requirement: Explicit network and deployment
Distribution SHALL document HTTPS/WSS/certificates/WebRTC PBX or gateway/ICE/STUN/TURN/network needs, including external temporary-TURN-credential issuer/generator and Origin-validating WSS proxy. It SHALL NOT promise direct WASM SIP UDP/TCP or offline calls. Public config MUST NOT contain secrets. Lab SHALL isolate accounts/media and expose no PBX admin/PSTN/personal environment.

#### Scenario: WEB-05 connectivity and NAT
- **WHEN** users encounter legacy PBX without WSS/WebRTC, invalid certificates, blocked networks, or a relay-required lab scenario
- **THEN** honest diagnostics offer no TLS bypass, supported TURN completes two-way media without permanent credentials, ephemeral credentials come from documented external infrastructure, and SIP/SDP/Authorization logs are disabled/sanitized

### Requirement: Separate build and acceptance
Project SHALL offer pinned Docker export of web assets/checksums/licenses/SBOM/HTTPS hosting guide. Release SHALL require published pinned Chromium/Firefox Linux tests, keyboard/themes, two real endpoints, privacy, exact-deployment tests. Desktop approval/fake media MUST NOT replace browser human audio.

#### Scenario: WEB-06 clean clone and deployment
- **WHEN** an operator Docker-builds a clone without base and deploys bundle to lab HTTPS
- **THEN** clean browsers use verified assets, pass WEB-01..05, and real voice has hash/version receipts without Linux-client installation
