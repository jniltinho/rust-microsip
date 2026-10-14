## Purpose

Integrate the softphone with Windows 10 1809+ and Windows 11 safely and with graceful degradation, with the same guarantees as the Linux integration and with unsafe code confined to one adapter crate.

## ADDED Requirements

### Requirement: Supported Windows versions without runtimes
rust-microsip SHALL run on Windows 10 version 1809 or later and Windows 11, x86_64, without installing a runtime (Visual C++ redistributable, .NET, MSYS2, Wine or a Rust toolchain). The executables SHALL import only DLLs shipped with Windows.

#### Scenario: WIN-01 clean machine
- **WHEN** the portable zip is extracted on a clean Windows 11 installation and `rust-microsip.exe` starts
- **THEN** the window opens without a missing-DLL error and the release check finds only Windows system DLLs in the import tables

### Requirement: Single instance and forwarding on Windows
The application SHALL keep one instance per user and profile through a local named pipe whose name includes the user SID, created only if no instance of that name exists, rejecting remote clients and granting access only to the current user. A second launch SHALL forward a `sip:`/`tel:` request to the owner and exit before SIP, audio or the database are touched; the owner SHALL close without a response any connection whose client process does not run as the same user or runs with a restricted token.

#### Scenario: WIN-02 second launch with a link
- **WHEN** a second process starts for the same profile with a valid `sip:` link
- **THEN** the number is filled in the original window, the second process exits, and no second registration or database worker exists

#### Scenario: WIN-02 impostor pipe
- **WHEN** another user's process created a pipe with the expected name before rust-microsip started
- **THEN** rust-microsip does not serve on it, reports that the instance channel could not start, and clients refuse to talk to it

### Requirement: Notifications and tray on Windows
The application SHALL show incoming and missed calls as Windows toast notifications with Answer and Reject actions when notifications are available, and a tray icon whose click shows the window and whose menu offers Show and Quit. Toasts SHALL carry only an opaque call token in their arguments and caller text sanitized as on Linux. When toasts are unavailable or suppressed (Focus Assist, no AppUserModelID), the incoming call SHALL remain visible and answerable in the window and the taskbar button SHALL flash.

#### Scenario: WIN-03 answer from a toast
- **WHEN** a call arrives and the user presses Answer on the toast
- **THEN** that call, and no replacement call, is answered once

#### Scenario: WIN-03 Focus Assist on
- **WHEN** a call arrives while Focus Assist suppresses toasts
- **THEN** the window banner shows the call with Answer and Reject and the taskbar button flashes

### Requirement: Opt-in links and autostart on Windows
Registration of `sip:` and `tel:` URL protocols and of autostart SHALL be per-user (HKCU), off by default, enabled only by an explicit user action and removable from the application; the application SHALL never write the user's default-app choice. Link arguments SHALL pass the same strict parser as on Linux and SHALL only fill the number.

#### Scenario: WIN-04 malicious link
- **WHEN** a link containing CR/LF, a shell command or unsupported parameters is opened
- **THEN** it is rejected and nothing is dialed or executed

#### Scenario: WIN-04 removal
- **WHEN** the user disables link handling and autostart
- **THEN** exactly the registry keys the application wrote are removed

### Requirement: Session end and quit on Windows
On sign-out, shutdown or a console close event, the application SHALL end calls best-effort, release audio, persist outcomes and attempt unregistration within five seconds, as on SIGTERM on Linux. Closing the window SHALL follow the Linux close/hide/quit rules.

#### Scenario: WIN-05 sign-out during a call
- **WHEN** the user signs out of Windows during a call
- **THEN** the PBX sees the call ended, the microphone is released, and history records the outcome

### Requirement: Secrets and private data on Windows
Opt-in password storage SHALL use Windows Credential Manager with a non-roaming generic credential; the database and files SHALL hold only a reference. The configuration and data directories, and the grant export directory, SHALL carry a protected access-control list granting access only to the current user; if it cannot be applied or verified, the application SHALL refuse to open them and SHALL name the directory.

#### Scenario: WIN-06 another user
- **WHEN** another standard user on the same machine reads the database or a grant export
- **THEN** access is denied

#### Scenario: WIN-06 Credential Manager unavailable
- **WHEN** writing the credential fails
- **THEN** the password is kept for the session only and the user is told it was not saved

### Requirement: Unsafe code confinement
Direct Windows API calls SHALL be confined to the Windows adapter crate; every other crate SHALL keep forbidding `unsafe` code. Each unsafe block in the adapter SHALL carry a `// SAFETY:` justification.

#### Scenario: WIN-07 unsafe outside the adapter
- **WHEN** a commit adds `unsafe` or relaxes `unsafe_code` in any other crate, or removes the adapter's undocumented-unsafe denial
- **THEN** the lint or layering test fails
