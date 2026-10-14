## Context

See [proposal.md](proposal.md). Current state:

- `crates/platform` (Linux only) exposes `acquire`/`acquire_owner`/`Listener` (single instance over a private Unix socket), `ControlServer`/`ControlClient` (0600 control socket checked with `SO_PEERCRED`), `Notifier` (D-Bus, `CallAction` Answer/Reject), `Tray` (StatusNotifierItem, Show/Quit), `SecretStore` (Secret Service), `read_private_file`/`read_own_file` (owner and mode checks through `rustix`), `parse_dial_uri` and `desktop_entry`. Errors are typed (`InstanceError`, `ControlSocketError`, `SecretError`, ...); the window only sees user-safe text through `Backend` jobs.
- Unix-only code outside the adapter: `config/src/files.rs` and `storage/src/lib.rs` (0600/0700 through `std::os::unix`), grant bootstrap export in `apps/rust-microsip/src/ui/state/grants.rs`, SIGTERM/SIGINT handling in the app, and tests using `std::os::unix`.
- Native C code in the graph: `rusqlite` (`bundled` SQLite) and `ring`. No OpenSSL (rustls with ring), unlike rustXterm, which lowers the cross-build risk.
- Workspace lints: `unsafe_code = "forbid"`, `missing_docs = "deny"`, `unwrap`/`expect` denied. `crates/core/tests/layering.rs` allowlists dependencies per package from `cargo metadata`.
- Build: pinned Ubuntu 24.04 image, snapshot apt, checksummed Rust components, `--network=none` build stages; CI on GitHub; release gates checked by `packaging/release/check-gates.sh`.

## Goals / Non-Goals

**Goals:** the Linux v0.1 feature set on Windows 10 1809+/11 x86_64 with no change to engine, media or UI logic; Docker-only builds; the same security and privacy rules; Windows artifacts produced by the same release workflow behind their own gates.

**Non-Goals:** see the proposal. arm64 stays reachable (same crates, a second target in a later change).

## Decisions

### W1. Toolchain: `x86_64-pc-windows-gnu` cross-compiled in Docker

| | windows-gnu (mingw-w64 in Docker) | windows-msvc via `cargo-xwin` | MSVC on a Windows host |
|---|---|---|---|
| Docker-only | yes, mingw-w64 from the pinned Ubuntu snapshot | yes, but downloads the MSVC CRT and Windows SDK at build time | no |
| Licensing | GCC with the Runtime Library Exception, mingw-w64 runtime permissive; static `libgcc`/`winpthreads` need notices | Microsoft MSVC/SDK licence must be accepted by us; the image and cache must not be redistributed | normal |
| Reproducibility | apt snapshot + checksummed Rust std | Microsoft manifest versions drift; we would mirror and checksum them | runner image drift |
| C deps (`ring`, SQLite) | both build with `x86_64-w64-mingw32-gcc` | both build with `clang-cl` shims | yes |
| Size / debugging | somewhat larger exe (static runtime), DWARF, no PDB | smaller, PDB | smaller, PDB |

Decision: windows-gnu. It reuses the image model, needs no Microsoft licence acceptance and matches rustXterm, so both projects share knowledge. Link the runtime statically (`-C target-feature=+crt-static`, `-static-libgcc`) so the exe imports only system DLLs; a dist check asserts the import allowlist (`objdump -p`). Deterministic PE: `-Wl,--no-insert-timestamp`, `SOURCE_DATE_EPOCH`; release exes stripped. cargo-xwin stays the documented fallback and needs a new owner decision on the Microsoft licence. wgpu uses DX12, then Vulkan, then GL; eframe's `wayland`/`x11` features become target-specific.

### W2. Unsafe confinement

`unsafe` is allowed only in `crates/platform-windows`. A crate-level `allow` cannot relax a workspace `forbid`, so the adapter declares its own `[lints]` table: the workspace rules copied, `unsafe_code = "allow"`, clippy `undocumented_unsafe_blocks` and `multiple_unsafe_ops_per_block` denied. `layering.rs` fails when any other package lacks `lints.workspace = true` or when the adapter's table drops the two denials. Unsafe stays in small private functions with safe wrappers; no raw handle escapes the crate. Safe crates are preferred where they already fit (tokio named pipes, cpal).

### W3. Platform API shape

The app selects the adapter by `cfg(target_os)`. Rather than introducing traits for one implementation per OS, both adapters export the same names and signatures (`acquire`, `ControlServer`, `Notifier`, `Tray`, `SecretStore`, `read_private_file`, ...) and the app imports `rust_microsip_platform` or `rust_microsip_platform_windows` under cfg. A compile-time API check (a small test module compiled for both targets that names every item) keeps them in sync. Portable code in `crates/platform` (`parse_dial_uri`, `sanitize_caller`, request/ack framing, typed errors) stays there and compiles on both targets; its Linux-only modules move behind `cfg(unix)` (zbus and rustix become `cfg(unix)` dependencies), and the Windows adapter depends on it for the portable parts instead of copying them. Rejected: a trait layer (one implementation per target, dynamic dispatch for no gain).

### W4. Single instance and control channel: named pipes

| | Named pipe + DACL for the user SID | AF_UNIX on Windows | TCP loopback |
|---|---|---|---|
| Peer identity | `GetNamedPipeClientProcessId`/`ServerProcessId` → token SID | no std/tokio support; peer PID ioctl only on recent builds | none |
| Squatting | `first_pipe_instance(true)` fails if the name exists | file in a directory we must protect | port squatting |
| Spec fit | local only, `reject_remote_clients(true)` | local | network socket, forbidden |

Decision: pipes `\\.\pipe\rust-microsip-<SID>-<profile hash>-instance` and `...-control`, created with tokio `ServerOptions`: `first_pipe_instance(true)` (it doubles as the single-instance lock), `reject_remote_clients(true)`, and a protected DACL `D:P(A;;GA;;;<user SID>)`. Every accepted client's process token must carry our SID and must not be restricted or AppContainer; otherwise the instance is disconnected without a response, which is the `SO_PEERCRED` contract. The client verifies the server process SID symmetrically, so the CLI never talks to an impostor. Framing, the 1 MiB limit, the instance request/ack protocol and "exists only while enabled" are unchanged; `ERROR_FILE_NOT_FOUND` on connect maps to "not running". A second launch forwards a `sip:`/`tel:` request and exits before SIP, audio or SQLite are touched, as on Linux.

### W5. Notifications, tray and session end

- **Toasts:** WinRT `ToastNotificationManager` through the `windows` crate, with an XML toast carrying Answer and Reject buttons (`activationType="foreground"`, arguments holding an opaque per-call token, never the number). Activation is handled in-process through the `Activated` event while the app runs; no COM background activator, because an incoming call only exists while the app runs. Unpackaged toasts need an AppUserModelID: the MSI's Start Menu shortcut carries it, and the portable build registers `HKCU\Software\Classes\AppUserModelId\<AUMID>` only when the user enables notifications (removable). Caller text goes through the existing `sanitize_caller`. Toasts are best-effort: Focus Assist or a missing AUMID degrades to the in-window banner plus `FlashWindowEx` on the taskbar; the window remains the authority (LIN-01 parity). Rejected: `notify-rust`/`tauri-winrt-notification` (action-button and activation support not sufficient for Answer/Reject without forking).
- **Tray:** `Shell_NotifyIconW` with a hidden message-only window on its own thread, same `Tray`/`TrayAction` (click shows, menu Show/Quit), re-added on `TaskbarCreated` after an Explorer restart. Rejected: `tray-icon` crate (larger graph for ~200 lines).
- **Session end and close:** SIGTERM/SIGINT have no GUI equivalent. The GUI handles `WM_QUERYENDSESSION`/`WM_ENDSESSION` through the adapter (subclassing the eframe window or the hidden tray window) and runs the existing bounded shutdown (hang up, unregister, release audio within five seconds). The console exe maps `CTRL_C`/`CTRL_BREAK`/`CTRL_CLOSE`/`CTRL_LOGOFF`/`CTRL_SHUTDOWN` through `tokio::signal::windows`.

### W6. Secrets and data protection

- **Credential Manager:** generic credentials, target `rust-microsip:<account reference>`, `CRED_PERSIST_LOCAL_MACHINE` (does not roam), `CredWriteW`/`CredReadW`/`CredDeleteW`, buffers zeroed after use. Same opt-in and reference-only storage as Secret Service; failures fall back to the session-only password with a notice. Rejected: `keyring` crate (extra graph), DPAPI files (Credential Manager is DPAPI-backed and user-visible).
- **Folders:** config in `%APPDATA%\rust-microsip`, database, backups and logs in `%LOCALAPPDATA%\rust-microsip` (SQLite WAL must not roam). `config` reads the two variables Windows sets for every interactive session and rejects non-absolute values; no new dependency. Explicit paths still override.
- **Owner-only DACL:** the adapter exposes `make_private_dir(path)` (protected DACL, full control for the user SID only; SYSTEM and Administrators keep access by privilege, as root does) and `read_private_file` checks owner SID and DACL instead of uid and mode. The app applies it to the config/data directories and the grant export directory before `storage`/`config` open files; failure blocks opening with a message naming the directory, mirroring the 0700 check. `config`/`storage` keep their mode calls under `cfg(unix)`; they gain no adapter dependency.

### W7. Links and autostart (opt-in, removable)

- `sip:` and `tel:` under `HKCU\Software\Classes\<scheme>` (`URL Protocol`, command `"<exe>" "%1"`, quoted path of the running exe), plus a `Capabilities` entry under `HKCU\Software\RegisteredApplications` so the app appears in Settings > Default apps. The app never writes `UserChoice` (hash-protected and the user's choice). Incoming arguments go through the existing strict `parse_dial_uri` and only fill the number.
- Autostart: a value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, off by default, removable from Settings.
- Removal deletes exactly the keys the app wrote; the MSI uninstaller removes them too.

### W8. Audio: cpal WASAPI, shared mode

cpal's WASAPI backend in shared mode; the existing ring/resampler design is unchanged (the engine already resamples to 8 kHz, so the mix format, usually 48 kHz float, is fine). Devices persist by the stable WASAPI endpoint id exposed by cpal, with the friendly name stored for display and fallback; the W1 spike confirms cpal 0.18 exposes it, otherwise a minimal adapter-side lookup maps names to ids. Hotplug follows the Linux rule (new microphone needs confirmation; a missing device falls back with a notice). Microphone privacy (Settings > Privacy > Microphone, "Let desktop apps access") denial surfaces as a typed "microphone blocked by Windows privacy settings" notice, not a silent call. Exclusive mode is rejected: no latency need at 8 kHz G.711, blocks other apps (ringing on the default device), and cpal does not offer it. Communications ducking: default role is console; the `eCommunications` role is evaluated in the spike but not required.

### W9. Two executables

A GUI-subsystem exe cannot write to the launching console; a console exe flashes a window on GUI starts. `apps/rust-microsip` gets two `[[bin]]` targets on Windows sharing `main`: `rust-microsip.exe` (`windows_subsystem = "windows"`) and `rust-microsip-cli.exe` (CLI, `mcp --stdio`, `service`, `--doctor`, `config ...`) with identical arguments, output and exit codes. Linux keeps one ELF. Rejected: `AttachConsole` in one exe (prompt returns early, stdin unusable for MCP). Resources (icon, version info, manifest with per-monitor DPI v2, UTF-8 code page, `longPathAware`, Windows 10 `supportedOS`) are embedded with mingw `windres` from the app build script.

### W10. Packaging and trust

- Portable zip `rust-microsip-<v>-windows-x86_64.zip`: both exes, `LICENSE`, third-party notices, README; fixed member order and timestamps.
- Per-user MSI `rust-microsip-<v>-x86_64.msi` to `%LOCALAPPDATA%\Programs\rust-microsip`, Start Menu shortcut with the AUMID, no elevation, fixed upgrade code, uninstall keeps user data and removes the keys of W7. Built in Docker with `wixl` (msitools from the snapshot). Fallback: WiX v4 on the Windows runner if `wixl` cannot set the shortcut AUMID. Rejected: MSIX (trusted signature required even for sideloading; package identity virtualizes AppData and the registry).
- SBOM and notices: the existing collectors run with `--filter-platform x86_64-pc-windows-gnu`; mingw runtime notices (libgcc exception, winpthreads) come from pinned upstream texts; the publication scanner extracts zip and MSI members (`msiextract`).
- Signing: optional Authenticode with `osslsigncode` in Docker when signing secrets exist, applied after the dist check and excluded from the reproducibility comparison. Unsigned releases state it; SmartScreen shows "Windows protected your PC" until reputation builds, and signing (especially EV or Azure Trusted Signing) shortens that. Defender false positives on new unsigned Rust binaries are possible: no packers or UPX, checksums published, false positives submitted to Microsoft, a known-issue note in the release notes.
- Firewall: the first non-loopback UDP/TCP bind triggers the Windows Defender Firewall prompt. The app never creates rules (needs elevation). Documentation explains the prompt; replies to outbound SIP/RTP pass the stateful filter, so a denied prompt mainly breaks incoming calls on UDP without registration keepalive. `--doctor` reports bound ports and the transport so users can create a rule themselves.

### W11. Testing

- **Docker:** `cargo test --no-run --target x86_64-pc-windows-gnu --message-format=json` lists test exes; they and the app exes are a CI artifact.
- **`windows-latest`:** downloads the artifact and runs every test exe plus `rust-microsip-cli.exe --doctor` and a CLI/MCP process smoke. Rust preinstalled on the runner image is never invoked, so the results bind to the Docker-built binaries. `#[cfg(windows)]` adapter tests: pipe same-user connect, restricted-token rejection, impostor pipe refusal, second instance forwarding, Credential Manager round trip, DACL readback, HKCU link registration/removal in a test hive key, WASAPI enumeration tolerating zero devices. No lab on the runner (Linux containers unavailable). A render smoke uses WARP; Linux snapshots stay the UI reference.
- **Windows Server 2022 VM** (VirtualBox `rustxterm-moba-ref`, shared with rustXterm): coordinate with its owner before each session, never delete, unregister or revert it, never touch rustXterm data; work under a fictional local user created for the session and removed afterwards; record any system change (for example enabling the Windows Audio service) and revert it; shut it down (ACPI) when done. Used for: pipe ACL with a second local user, Credential Manager, DACLs, tray, firewall prompt, MSI install/uninstall, CLI/MCP.
- **Windows 11 desktop VM** (new, evaluation image, TPM/Secure Boot in VirtualBox, created by a script like `scripts/vm/create-desktop-vm.sh`): the desktop-SKU checks the Server SKU cannot prove: toasts with Answer/Reject, Focus Assist fallback, tray, Default apps for `tel:`, microphone privacy, session sign-out during a call, scaling.
- **Lab:** both VMs reach the Asterisk lab only through the host-only profile of the Linux change's task 1.5; nothing is published on the LAN. Fictional accounts only. A human two-way audio check runs on the Windows 11 VM with a real headset (USB passthrough) or a second human endpoint.
- Wine is not evidence (pipes, DACLs, Credential Manager, toasts are not faithful).

### W12. Release gates

The spec scenarios WIN-01..WIN-13 and tasks 7.x form a "Windows" section in `docs/release/vX.Y.Z.md`. `release.yml` builds and attaches Windows artifacts only when that section exists with every box checked; otherwise it publishes Linux artifacts alone. The Linux v0.1.0 gates are not changed by this change.

### W13. Delivery

| Milestone | Delivers | Commit when |
|---|---|---|
| W-A build spike | toolchain stage, cfg fixes, empty adapter, exe runs on the runner | Docker Windows build and cross-compiled tests green; Linux gates unchanged |
| W-B adapter | instance/control pipes, Credential Manager, DACLs, toasts, tray, links, autostart, session end, WASAPI ids | runner adapter tests green |
| W-C packaging + CI | zip, MSI, notices, SBOM, scan, signing hook, CI and release jobs | CI green on both OSes |
| W-D verification | VM receipts, lab calls, review | WIN gates checked with receipts |

Parallel agents with explicit file ownership: adapter (one), cfg fixes in `config`/`storage`/app (one), Docker/CI/packaging (one), each with its own `CARGO_TARGET_DIR`.

## Risks / Trade-offs

- [mingw C build of `ring` or SQLite fails] → first task is the spike; fallback cargo-xwin after an owner licence decision.
- [Unsafe Win32/WinRT bugs] → one crate, `// SAFETY:` per block, small wrappers, runner tests, review in W-B.
- [Toast activation unreliable for unpackaged apps] → window banner and taskbar flash stay authoritative; toast is best-effort by spec.
- [SmartScreen/Defender block unsigned builds] → optional signing, checksums, documented prompts; certificate is an owner decision.
- [Firewall prompt denied] → documentation and `--doctor`; outbound flows keep working.
- [Shared VM conflicts with rustXterm] → coordination before use, fictional user, no reverts, halt when done.
- [No local Windows toolchain] → the Windows CI job is the gate before push.

## Migration Plan

New platform; Linux users unaffected. Settings move between OSes with `config export`/`import`; passwords are re-entered (never exported). Rollback: uninstall the MSI or delete the zip folder; user data in `%APPDATA%`/`%LOCALAPPDATA%\rust-microsip` and `rust-microsip:*` credentials remain until removed by hand (documented).

## Open Questions

- Owner approval of the unsafe policy (W2) and of windows-gnu over MSVC (W1).
- Code-signing: none, OV, EV or Azure Trusted Signing, and who holds it.
- Whether to offer an optional per-machine MSI that creates a firewall rule (rejected by default here).
- Windows 11 evaluation image acceptable for the desktop VM, or a licensed image.
- When to add aarch64 (gnullvm via llvm-mingw, or msvc).
