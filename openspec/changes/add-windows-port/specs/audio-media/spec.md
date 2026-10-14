## ADDED Requirements

### Requirement: Windows audio through WASAPI shared mode
On Windows, capture, playback and ringing SHALL use WASAPI in shared mode, with the same bounded rings, resampling and silent-fallback rules as on Linux. Selected devices SHALL be persisted by their stable endpoint id, with the friendly name kept only for display. A device that disappears SHALL fall back with a notice, and a newly connected microphone SHALL need confirmation before use. When Windows privacy settings block microphone access, the application SHALL say so instead of sending silence without explanation.

#### Scenario: WIN-08 five-minute call on Windows
- **WHEN** a Windows 11 client calls the lab PBX for five minutes per codec (PCMU, PCMA) with a headset
- **THEN** both directions are audible without sustained dropouts, and the devices are released after the call

#### Scenario: WIN-08 device renamed or unplugged
- **WHEN** the selected headset is unplugged and plugged back, or its friendly name changes
- **THEN** the call continues on the fallback device with a notice, and the saved selection still resolves to the headset by id

#### Scenario: WIN-08 microphone blocked
- **WHEN** desktop-app microphone access is disabled in Windows privacy settings and a call starts
- **THEN** a notice names the privacy setting and the call is not reported as having a working microphone
