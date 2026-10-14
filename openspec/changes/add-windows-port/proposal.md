> Approved for implementation on 2026-10-14; see the [approval record](approval.md).

## Why

MicroSIP is a Windows application, so most people who would switch to rust-microsip run Windows; rust-microsip ships only for Linux. The architecture already keeps telephony, media, storage and core free of Linux APIs ([architecture](../../../docs/architecture.md), "Porting"); what is Linux-specific lives in `crates/platform` (Unix sockets with `SO_PEERCRED`, D-Bus notifications, StatusNotifierItem tray, Secret Service), in Unix file modes inside `config`/`storage`, and in the packaging. This change plans a Windows port that tests that promise without touching the engine. The sibling project's draft (`<rustXterm checkout>/openspec/changes/add-windows-port/`) faced the same stack; its decisions are reused where they fit a softphone.

## What Changes

- Target Windows 10 1809+ and Windows 11 on x86_64; aarch64 later with the same design.
- New crate `crates/platform-windows` implementing the same platform API as the Linux adapter: single instance and control channel over a named pipe restricted to the user SID (replacing `SO_PEERCRED`), toast notifications with Answer/Reject, tray icon (`Shell_NotifyIconW`), Credential Manager instead of Secret Service, owner-only DACLs instead of 0600/0700, opt-in `sip:`/`tel:` URL protocol registration and opt-in autostart under HKCU, session-end handling instead of SIGTERM.
- **Unsafe policy (owner decision required):** `unsafe` only inside `crates/platform-windows`, every block with `// SAFETY:`; the layering test fails if any other crate relaxes `unsafe_code = "forbid"`.
- Small `#[cfg]` changes outside the adapter: Unix modes in `config`/`storage`/grant export, Windows default folders, Unix-only tests gated.
- Audio through cpal's WASAPI backend in shared mode, devices persisted by stable id.
- Two executables from the same app crate: `rust-microsip.exe` (GUI subsystem) and `rust-microsip-cli.exe` (console: CLI, `mcp --stdio`, `service`, `--doctor`).
- Docker-only cross build for `x86_64-pc-windows-gnu` (pinned mingw-w64); Windows tests cross-compiled in Docker and executed on a `windows-latest` runner without using its toolchain.
- Artifacts: portable zip and per-user MSI, `SHA256SUMS`, SBOM and licence notices; optional Authenticode signing.
- Manual verification on a shared Windows Server 2022 VM and a new Windows 11 desktop VM against the Asterisk lab through the host-only profile.

### Non-goals

Windows on arm64, Windows 7/8.1, WASAPI exclusive mode, MSIX/Microsoft Store/winget, a Windows Service (SCM) mode, per-machine installs, automatic firewall rules, automatic updates, a portable "data next to the exe" mode, headset HID controls, macOS. Feature scope is exactly the Linux v0.1 scope; no Windows-only telephony features.

## Capabilities

### New Capabilities

- `windows-integration`: Windows adapter behavior: single instance and control pipe, notifications, tray, links, autostart, session end, data protection, Credential Manager, unsafe confinement.

### Modified Capabilities

No canonical specs exist yet. Added requirements extend capabilities defined in unarchived changes: `audio-media`, `config-storage`, `reproducible-distribution` ([Linux release](../create-native-rust-linux-release/proposal.md)) and `agent-cli-mcp` ([agent extension](../add-agent-cli-mcp/proposal.md)).

## Impact

- New: `crates/platform-windows`; Windows-only dependencies `windows` (Win32/WinRT bindings) and a resource embedder for the app build script.
- Changed: `apps/rust-microsip` (adapter selection per target, second binary, icon/manifest/version resources), `crates/config` and `crates/storage` (cfg around Unix modes, Windows folders), `crates/core/tests/layering.rs`, `Dockerfile`, `Makefile`, `.github/workflows/{ci,release}.yml`, licence/SBOM/scan scripts, `docs/`.
- Unchanged: `core`, `media`, `telephony`, `control` logic, the UI reducer, the database format.
- **Does not block v0.1.0 for Linux.** Windows artifacts join a release only when the Windows gates in this change are checked; until then releases stay Linux-only. Depends on the Linux change's task 1.5 (host-only lab profile) for call tests. Planning only; nothing here is implemented.
