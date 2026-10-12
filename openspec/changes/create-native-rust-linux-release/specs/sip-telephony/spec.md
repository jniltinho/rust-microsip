## Purpose

Provide interoperable, predictable SIP voice telephony in the first Linux release, with real controls and bounded network-failure recovery.

## ADDED Requirements

### Requirement: Registration and transports
The application SHALL register one enabled account at a time over UDP, TCP, or TLS, authenticate 401/407 challenges, renew registration, and display its real state. A second enabled account MUST be rejected with a clear error in this release.

#### Scenario: SIP-01 registration and renewal
- **WHEN** a valid account is configured on the lab PBX for each transport
- **THEN** PBX/UI confirm registration and renewal without duplication or authentication loops

#### Scenario: SIP-02 invalid configuration and authentication
- **WHEN** the password is wrong or two accounts are enabled
- **THEN** an error is reported without secret leakage, silent partial registration, or continuous rejected-credential retry

### Requirement: Call lifecycle
The application SHALL originate, receive, answer, reject, cancel, and end SIP calls with one simultaneous call. It SHALL distinguish answered, busy, rejected, canceled, missed, and failed calls; a second incoming call SHALL receive busy without dropping the existing one.

#### Scenario: SIP-03 calls in both directions
- **WHEN** the application calls a second extension and then receives a call from it
- **THEN** both complete establishment/termination and each session has exactly one history outcome

#### Scenario: SIP-04 cancellation and answer race
- **WHEN** the user cancels before answering, including a crossing 200 OK
- **THEN** the session ends correctly without ghost calls, persistent ringing, or retained resources

#### Scenario: SIP-05 rejection and second call
- **WHEN** the user rejects an incoming call or receives another while already in a call
- **THEN** the PBX receives the appropriate response and the existing call remains functional

### Requirement: Call controls
The application SHALL send DTMF through negotiated RTP telephone-event, hold/resume through SDP renegotiation, and offer DND rejecting new incoming calls without affecting an ongoing call.

#### Scenario: SIP-06 DTMF and hold
- **WHEN** the user sends digits to an IVR and toggles hold/resume
- **THEN** the IVR recognizes the sequence and bidirectional audio returns without ending the call

#### Scenario: SIP-07 do not disturb
- **WHEN** DND is enabled before a new incoming call
- **THEN** it is rejected with coherent indication, without ringing or interrupting an established session

### Requirement: Confirmed blind transfer
The application SHALL transfer through REFER and track the final result through NOTIFY, never report success on 202 alone. On failure, it SHALL maintain/recover the original call when the dialog still exists.

#### Scenario: SIP-08 transfer and failure
- **WHEN** the user transfers first to a valid and then to an invalid lab destination
- **THEN** success appears only after final confirmation, and failure leaves the original session operable when the PBX preserves it

### Requirement: Signaling security and recovery
The application SHALL validate TLS certificate/hostname, accept an explicit custom CA, and never silently downgrade. After network loss/suspend it SHALL mark unavailability and attempt registration with bounded backoff; it SHALL NOT automatically recreate a lost call.

#### Scenario: SIP-09 invalid certificate
- **WHEN** the server presents an expired certificate, wrong hostname, or untrusted CA
- **THEN** the secure connection fails visibly without TCP/UDP fallback

#### Scenario: SIP-10 network and suspend
- **WHEN** the network fails/returns or the machine resumes
- **THEN** registration recovers without request storms and ended sessions are not redialed
