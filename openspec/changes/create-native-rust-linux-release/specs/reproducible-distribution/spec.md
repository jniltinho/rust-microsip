## Purpose

Enable building, testing, and distributing the first Linux release without a host toolchain, with verifiable artifacts and no publication of local references or personal data.

## ADDED Requirements

### Requirement: Executable with embedded resources
The release SHALL deliver an x86_64 ELF with embedded UI, licensed fonts, icons, tones, essential codecs, migrations, and SQLite, without first-use downloads or application subprocesses. It SHALL run on clean Ubuntu 24.04 GNOME/Wayland and KDE/X11 without installing Rust, Node, Qt, an external database, or application-specific libraries. Standard Linux dependencies SHALL be enumerated in an audited manifest.

#### Scenario: DIST-01 clean machine
- **WHEN** only the ELF is copied to a clean supported-matrix VM with its standard desktop/audio, connected to the auxiliary PBX and second endpoint through the isolated lab network
- **THEN** the user configures an account and completes a two-way call without downloading application resources or installing extra packages

### Requirement: Additional AppImage does not hide dependencies
The release SHALL provide an additional AppImage with the same core, checksums, and documented FUSE-free execution. AppImage MUST NOT be presented as proof that the ELF meets the single-binary requirement.

#### Scenario: DIST-02 separate ELF and AppImage
- **WHEN** both artifacts are tested in supported environments, including AppImage without FUSE
- **THEN** each has its own receipt and one success does not replace the other's failing test

### Requirement: Docker build without host toolchain
The project SHALL build from a clean clone with only Docker Engine/BuildKit, requiring neither host Cargo nor `base/`. Toolchain, lockfile, images, and dependencies SHALL be pinned; artifacts SHALL export locally without unnecessary root privileges.

#### Scenario: DIST-03 clean-clone build
- **WHEN** an operator runs the documented command on a Docker machine without Rust installed
- **THEN** `dist/` contains ELF, AppImage, SHA256SUMS, licenses, SBOM, and dependency manifest, without copying `.env`, databases, or local references into the context

### Requirement: Disposable Asterisk lab
The project SHALL provide a Docker Asterisk lab with two fictional extensions, echo/IVR, TLS/SRTP, SIP tests, and teardown. Defaults SHALL isolate the network without publishing host ports; the manual host profile SHALL bind loopback only, without real credentials/external trunks. An additional VM acceptance profile SHALL be opt-in, exclusively on an isolated host-only network, bound to the explicit auxiliary-VM IP with test-client-only ACL, never public/wildcard interfaces. Test credentials SHALL be locally generated and Git-ignored.

#### Scenario: DIST-04 lab and isolation
- **WHEN** the operator starts the default lab, runs tests, and removes it
- **THEN** the PBX serves automated scenarios, no ports are published, there are no PSTN routes, and teardown removes disposable resources

#### Scenario: DIST-05 manual desktop profile
- **WHEN** the operator explicitly activates the local host-call profile or isolated VM acceptance profile
- **THEN** only documented SIP/RTP ports are available on 127.0.0.1 for host or the auxiliary-VM IP/ACL for VM, with anchored media and no 0.0.0.0 or production-network exposure

### Requirement: Real gates and independent review
The release MUST require unit/integration tests and audio calls on the exact artifact, both-theme visual matrices, and Claude CLI review. Text review, mocks, or window smoke tests MUST NOT count as functional telephony proof.

#### Scenario: DIST-06 incomplete release blocked
- **WHEN** only a visible window, mocked tests, or architecture approval exists
- **THEN** release remains blocked until call/two-way audio evidence exists, without prematurely announcing a usable version

### Requirement: Traceable, private-by-default publication
The project SHALL publish only audited own files and documentary references on GitHub, never `base/`, credentials, databases, dumps, recordings, or personal logs. A release SHALL identify commit, license, checksums, SBOM, and sanitized receipts. Initial visibility SHALL be private unless the owner explicitly instructs otherwise.

#### Scenario: DIST-07 pre-push audit
- **WHEN** staged content and release artifacts are audited
- **THEN** no `base/` path, secret, or personal data is found, and each distributed artifact's origin is identifiable
