## Purpose

Integrate the softphone with Linux desktops safely and with graceful degradation, without requiring shell extensions or changing system preferences without consent.

## ADDED Requirements

### Requirement: Optional notifications and tray
The application SHALL notify incoming/missed calls through the desktop when available, with answer/reject actions when supported. It SHALL work without tray/notification services, keeping incoming calls accessible in the window.

#### Scenario: LIN-01 desktop without tray
- **WHEN** the desktop provides neither StatusNotifierItem nor notifications
- **THEN** incoming calls remain visible/answerable in the application, without crashes or a permanently inaccessible window

### Requirement: Confirmed external URI
The application SHALL accept `tel:` and `sip:` without shell execution, bound/validate destinations, reject CR/LF and unsupported parameters, and request confirmation before dialing. Association installation/autostart SHALL be user-level opt-in.

#### Scenario: LIN-02 legitimate and malicious links
- **WHEN** the user opens a valid link or one containing control characters, a shell command, or an invalid parameter
- **THEN** the valid link opens confirmation, the malicious one is rejected, and no command/call executes automatically

### Requirement: Local single instance
The application SHALL maintain one instance per user/profile and forward URIs through protected local IPC, without a remote API or public control port.

#### Scenario: LIN-03 second instance
- **WHEN** a second process starts for the same profile with a valid URI
- **THEN** a GUI owner presents confirmation in the original window, while a headless service owner returns uri_confirmation_unavailable to the invoker without dialing; neither case causes a second SIP registration or destructive database contention

### Requirement: Safe close and quit
The application SHALL distinguish hiding from quitting. Without a tray, closing SHALL offer safe exit rather than unrecoverable hiding. Quitting during a call SHALL require confirmation; shutdown SHALL release audio, persist outcomes, and attempt hangup/unregistration within five seconds.

#### Scenario: LIN-04 exit during a call
- **WHEN** the user requests and confirms exit during a call or the process receives SIGTERM
- **THEN** the call ends best-effort, the microphone is released, and the process exits within the deadline even without a PBX response
