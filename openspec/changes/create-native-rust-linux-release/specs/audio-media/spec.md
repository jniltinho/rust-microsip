## Purpose

Ensure genuinely bidirectional voice audio, selectable devices, and secure media negotiation in the distributed Linux artifact.

## ADDED Requirements

### Requirement: RTP and essential codecs
The application SHALL negotiate G.711 PCMA/PCMU, send and receive RTP, and handle sequence numbers, timestamps, jitter, and lost or out-of-order packets with bounded memory. An incompatible codec MUST produce an understandable error instead of silent success without audio.

#### Scenario: AUD-01 bidirectional voice
- **WHEN** the release artifact establishes a five-minute PBX call with each codec
- **THEN** both participants hear voice in both directions without a crash, and the sanitized receipt records the codec/devices

#### Scenario: AUD-02 degraded network and incompatible codec
- **WHEN** the lab injects 1% loss, up to 30 ms jitter, and reordering, or offers only an incompatible codec
- **THEN** the degraded session remains controllable with bounded buffers, and the incompatible session fails explicitly without a false active-audio indicator

### Requirement: Devices and hotplug
The application SHALL list/select input, output, and ringing output, report missing devices/denied permissions, and allow recovery after headset disconnection. Switching microphones during a call MUST require explicit selection, never silently activate another microphone.

#### Scenario: AUD-03 selection and headset removal
- **WHEN** the user selects a headset and disconnects it during a call
- **THEN** the UI reports device loss and allows another selection, resuming capture only after confirmation, without freezing or changing global system settings

### Requirement: Mute and local testing
The application SHALL allow capture/playback testing without a call and microphone mute during a call. While muted, it SHALL prevent captured voice transmission without disabling reception.

#### Scenario: AUD-04 test and mute
- **WHEN** the user tests audio and then enables mute in a lab call
- **THEN** the local test confirms the device, the other extension stops receiving microphone voice, and the user continues hearing the call

### Requirement: Secure media without downgrade
The application SHALL offer a TLS + SRTP profile using SDES and AES_CM_128_HMAC_SHA1_80; SDES keys MUST travel only over validated TLS. The secure profile MUST reject peers without SRTP. Unencrypted RTP SHALL require explicit selection and a persistent security warning.

#### Scenario: AUD-05 mandatory SRTP
- **WHEN** the secure profile negotiates with a compatible peer and then an RTP-only peer
- **THEN** the first call uses lab-verified SRTP, and the second is rejected without downgrade or key logging

### Requirement: NAT with explicit limits
The application SHALL support rport, keepalive, symmetric RTP, and configurable STUN for compatible networks. It SHALL document LAN/VPN/media-relaying PBX as release topologies without guaranteeing arbitrary symmetric NAT or TURN.

#### Scenario: AUD-06 supported topology
- **WHEN** the application calls through simple NAT with a media-anchoring PBX and configured STUN
- **THEN** audio flows both ways and connectivity failures are distinguishable from missing devices, without claiming universal support
