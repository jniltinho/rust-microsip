## ADDED Requirements

### Requirement: Windows cross build in Docker
Windows executables SHALL be built in the same pinned Docker image model as the Linux release (base image by digest, snapshot packages, checksummed Rust components including the Windows target standard library, `--locked`, network only in the fetch stage). Building SHALL NOT require a Windows host, a Microsoft SDK download or a toolchain on the developer machine.

#### Scenario: WIN-11 build on a Linux host
- **WHEN** a developer with only Docker runs the documented Windows target
- **THEN** both executables, the zip, the MSI, `SHA256SUMS`, notices and the SBOM appear under `dist/`

### Requirement: Windows release artifacts
A release that includes Windows SHALL ship a portable zip and a per-user MSI that installs without elevation, adds a Start Menu entry and uninstalls without deleting user data or credentials. All artifacts SHALL be in `SHA256SUMS`, covered by the SBOM and licence notices (including the statically linked mingw runtime) and pass the publication scanner on their extracted members. Executables and MSI SHALL be Authenticode-signed when a certificate is configured; otherwise the release notes SHALL say they are unsigned and describe the SmartScreen and firewall prompts.

#### Scenario: WIN-12 per-user MSI
- **WHEN** a standard user installs, upgrades and uninstalls the MSI
- **THEN** no elevation prompt appears, the Start Menu entry works, and data and opt-in registry keys written by the user remain or are removed as documented

#### Scenario: WIN-12 scanner on Windows artifacts
- **WHEN** a zip or MSI member contains a personal absolute path, a fictional secret marker or a forbidden path
- **THEN** the scanner fails

### Requirement: Windows quality gates
CI SHALL cross-compile workspace tests for Windows in Docker and run them on a hosted Windows runner without invoking any toolchain installed there. Adapter integration tests SHALL run there. Windows releases SHALL additionally require receipts from the Windows Server and Windows 11 VMs against the lab through the host-only profile, a human two-way audio check, a read-only Claude CLI review and the owner's decision. Missing Windows gates SHALL NOT block a Linux-only release.

#### Scenario: WIN-13 Windows regression
- **WHEN** a Windows test fails on the runner
- **THEN** CI is red and no release can be tagged on that commit

#### Scenario: WIN-13 Linux release without Windows gates
- **WHEN** a Linux release is tagged while the Windows gate section is absent or incomplete
- **THEN** the release publishes Linux artifacts only and attaches no Windows artifact
