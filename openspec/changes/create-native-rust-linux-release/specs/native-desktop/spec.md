## Purpose

Provide a native, compact Linux desktop interface familiar to MicroSIP users, with GOnnect as a complementary reference, without disguising unavailable features.

## ADDED Requirements

### Requirement: Native interface and reference layout
The application SHALL run without webview, Electron, Tauri, or Qt and offer a compact window with Dialpad, Calls, and Contacts tabs, number field, 3×4 keypad, call/hangup, account/status, settings menu, and DND. Mute, hold, and blind transfer SHALL be accessible during calls; local lists SHALL include search/favorites. It SHALL NOT require the GOnnect dashboard/address book or Wine runtime. Out-of-scope features MUST be omitted or show unavailability without simulated data.

#### Scenario: ND-01 comparable layout
- **WHEN** the window opens with fictional data at the same resolution, scaling, language, and state as MicroSIP Lite 3.22.16 single-call mode and the approved Rust mock
- **THEN** essential elements have equivalent order, grouping, and function in the compact flow, without requiring pixel-perfect Win32 identity, with differences recorded in the visual matrix and no overlapping/clipped controls

#### Scenario: ND-02 feature outside the release
- **WHEN** the UI matrix is compared with upstream chat, video, conference, or recording controls
- **THEN** out-of-scope controls are absent or clearly unavailable, without misleading actions or fabricated content

### Requirement: Persistent themes
The application SHALL offer light, dark, and system themes, apply changes without restart, and preserve preferences across openings.

#### Scenario: ND-03 theme switching
- **WHEN** the user switches light/dark during a call
- **THEN** all screens/dialogs change theme, the call continues, and the selection returns on next startup

#### Scenario: ND-04 system theme
- **WHEN** system mode is active and the desktop communicates a color-preference change
- **THEN** the UI follows it, falling back to light if the service is absent

### Requirement: Keyboard operation and scaling
The application SHALL maintain visible focus, keyboard navigation, control labels, and at least 4.5:1 normal-text contrast in both themes. It SHALL keep essential actions accessible at 1024x768 and 100%, 150%, and 200% scaling, scrolling when necessary.

#### Scenario: ND-05 keyboard and HiDPI
- **WHEN** an operator uses only the keyboard on essential screens at each supported scale
- **THEN** they can search, dial, answer, reject, and hang up without relying solely on color or unlabeled icons

### Requirement: Visible states and failures
The application SHALL show registration, ringing, active call, hold, error, and reconnection without freezing the UI or claiming success before backend confirmation.

#### Scenario: ND-06 registration failure
- **WHEN** the server rejects credentials
- **THEN** the UI reports failure and allows configuration correction without displaying the password or closing the application
