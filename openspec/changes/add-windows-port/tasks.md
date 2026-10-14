Every task is checked only with a receipt in `docs/receipts/` bound to a commit (and to artifact hashes where binaries are involved). Runner results, VM checks, human audio and reviews are separate evidence; none inherits another. This change does not block the Linux v0.1.0 release.

## 1. Approval and build spike (W-A)

- [x] 1.1 Review this proposal with the owner and record decisions on the unsafe policy (W2), windows-gnu versus MSVC (W1), signing, and the Windows 11 evaluation image; verify an approval record linked from the proposal. Completed: [approval record](approval.md). The owner's instruction started implementation with the recommended W1/W2 defaults and no signing for now; the Windows 11 image is decided at task 6.2.
- [ ] 1.2 Add a Windows toolchain stage to the `Dockerfile` (mingw-w64 from the pinned snapshot, Windows Rust std in `docker/rust-components.sha256`) and a `make` target; verify `cargo build --target x86_64-pc-windows-gnu --frozen` offline in Docker, including `ring` and bundled SQLite, without any host toolchain (WIN-11).
- [ ] 1.3 Gate Linux-only code with `cfg(unix)` (platform modules, zbus/rustix dependencies, Unix modes in `config`/`storage`/grants, Unix-only tests) and make eframe `wayland`/`x11` target-specific; verify the Linux build, tests and lab suite are unchanged and the Windows build compiles with a stub adapter.
- [ ] 1.4 Spike on the runner: run a cross-compiled exe that opens the window under WARP, enumerates WASAPI devices and prints whether cpal exposes stable endpoint ids; verify a receipt recording the import table (system DLLs only) and the id result (WIN-01, WIN-08).
- [ ] 1.5 Add a licence ADR for the mingw-w64 runtime, the `windows` crate and the MSI tool; verify `packaging/collect-cargo-notices.py` passes for the Windows platform filter with pinned upstream texts.

## 2. Platform adapter (W-B)

- [ ] 2.1 Create `crates/platform-windows` with its own `[lints]` table (unsafe allowed, undocumented and multi-op unsafe blocks denied) and extend `layering.rs` to reject unsafe relaxation elsewhere; verify injected violations in another crate and a missing `// SAFETY:` both fail (WIN-07).
- [ ] 2.2 Add the cross-target API check that names every adapter item used by the app; verify a missing or mismatched item fails compilation for either target.
- [ ] 2.3 Implement the single-instance pipe with SID-scoped name, `first_pipe_instance`, remote rejection, protected DACL and peer SID checks; verify on the runner: forwarding of a `sip:` link, restricted-token rejection, impostor refusal, and exit before SIP/audio/SQLite (WIN-02).
- [ ] 2.4 Implement the control pipe server/client with the same framing, limits and enable rules; verify the existing control tests pass on the runner plus other-user and impostor cases (WIN-10).
- [ ] 2.5 Implement `SecretStore` on Credential Manager with session-only fallback; verify round trip, delete, failure fallback and that no password appears in Debug output, logs or the database (WIN-06).
- [ ] 2.6 Implement `make_private_dir` and `read_private_file` with owner SID and DACL checks, and apply them before config, storage and grant export open files; verify DACL readback, refusal on an unprotectable directory, and denial for a second local user on the Server VM (WIN-06, WIN-09).
- [ ] 2.7 Implement Windows default folders in `config` with absolute-path validation; verify first-run locations and refusal of an empty or relative variable (WIN-09).
- [ ] 2.8 Implement toasts with Answer/Reject, opaque per-call tokens, sanitized caller text, AUMID handling and the banner/taskbar-flash fallback; verify on the Windows 11 VM: answer and reject from the toast act once on the right call, and Focus Assist falls back (WIN-03).
- [ ] 2.9 Implement the tray with `Shell_NotifyIconW` (Show/Quit, `TaskbarCreated` re-add); verify click, menu and an Explorer restart on both VMs (WIN-03).
- [ ] 2.10 Implement opt-in `sip:`/`tel:` registration with `RegisteredApplications` capabilities and opt-in autostart, both removable; verify registration, Default apps listing, strict parsing of malicious links, and exact key removal (WIN-04).
- [ ] 2.11 Implement session end (`WM_QUERYENDSESSION`/`WM_ENDSESSION`) and console control events through the existing bounded shutdown; verify sign-out and shutdown during a lab call end it at the PBX and release the microphone within five seconds (WIN-05).

## 3. Application and audio (W-B)

- [ ] 3.1 Add the second `[[bin]]` (`rust-microsip-cli.exe`, console) and `windows_subsystem` for `rust-microsip.exe`; verify identical CLI output and exit codes to Linux in process tests on the runner and no console window on GUI start (WIN-10).
- [ ] 3.2 Embed icon, version info and manifest (per-monitor DPI v2, UTF-8, `longPathAware`, Windows 10 `supportedOS`) with mingw `windres`; verify the resources in the exe and correct scaling at 100/150/200% on the Windows 11 VM.
- [ ] 3.3 Persist audio devices by WASAPI endpoint id with name fallback and migrate name-only selections safely; verify rename, unplug/replug and confirmation of a new microphone (WIN-08).
- [ ] 3.4 Report microphone privacy denial and device loss as typed notices; verify with the privacy toggle on the Windows 11 VM (WIN-08).
- [ ] 3.5 Run `mcp --stdio`, the CLI and `service` on Windows with grant files read through `read_private_file`; verify the SDK client handshake, protocol-only stdout, approval_required without a grant, and no capability in argv or logs (WIN-10).

## 4. Packaging and trust (W-C)

- [ ] 4.1 Build the portable zip with fixed member order and timestamps; verify contents, `SHA256SUMS` and a repeated build with identical hashes on the same machine (WIN-11).
- [ ] 4.2 Build the per-user MSI with `wixl` (WiX v4 on the runner only if the AUMID shortcut cannot be written); verify install/upgrade/uninstall without elevation and that user data and credentials remain (WIN-12).
- [ ] 4.3 Extend SBOM, notices and the publication scanner to Windows artifacts (zip and MSI members); verify negative controls with a personal path, a fictional secret and a forbidden path (WIN-12).
- [ ] 4.4 Add the optional `osslsigncode` signing step after the dist check, skipped without secrets; verify signed and unsigned paths and that release notes state the unsigned case and SmartScreen prompt.
- [ ] 4.5 Document firewall prompts, Defender false-positive handling, SmartScreen, data locations, removal and rollback in the user guide and `docs/DEVELOPMENT.md`; verify no absolute personal path and `make scan` clean.

## 5. CI and release workflow (W-C)

- [ ] 5.1 Add a CI job that cross-compiles test exes in Docker and a `windows-latest` job that runs them without invoking the runner's Rust; verify an injected Windows-only test failure turns CI red (WIN-13).
- [ ] 5.2 Extend `release.yml` and `check-gates.sh` so Windows artifacts are built and attached only when the Windows gate section is fully checked; verify both a Linux-only release dry run and a gated Windows dry run with negative controls (WIN-13).
- [ ] 5.3 Add a WARP render smoke and the `--doctor` smoke on the runner; verify they fail when the window cannot render or the doctor output contains a secret marker.

## 6. VMs and lab verification (W-D)

- [ ] 6.1 Agree a usage window for the shared Server 2022 VM `rustxterm-moba-ref` with its owner; use a fictional local user, record and revert system changes, never delete, unregister or revert the VM, and shut it down after each session; verify a session log in the receipt.
- [ ] 6.2 Add a script that creates a Windows 11 desktop VM from an evaluation image with a fictional user and local key material; verify creation and teardown without personal data, alongside `scripts/vm/create-desktop-vm.sh`.
- [ ] 6.3 Connect both VMs to the Asterisk lab only through the host-only profile from the Linux change's task 1.5; verify registration over UDP, TCP and TLS, SRTP calls and that no lab port is reachable from the LAN.
- [ ] 6.4 Run the Linux lab scenarios that apply (calls both ways, reject, cancel, DND, DTMF, hold, transfer, network loss, link forwarding, agent control and grants) against the exact MSI build on the Server VM; verify a receipt per scenario bound to the artifact hash.
- [ ] 6.5 Run the desktop checklist on the Windows 11 VM with the exact MSI and zip: toasts, Focus Assist, tray, Default apps, sign-out during a call, scaling, accessibility with Narrator on the essential actions; verify a receipt per artifact hash (WIN-03, WIN-04, WIN-05).
- [ ] 6.6 Run the human five-minute two-way audio check per codec with a headset or a second human endpoint on the Windows 11 VM; verify a separate human receipt (WIN-08).

## 7. Windows release gates

- [ ] 7.1 Gate WIN-01..WIN-13 evidence: every scenario in this change's specs has a receipt; verify the Windows section of `docs/release/vX.Y.Z.md` links each one.
- [ ] 7.2 Run a read-only Claude CLI review with sanitized context focused on the adapter's unsafe code, pipe ACLs and secret handling; verify findings are resolved or recorded, distinguishing review from runtime evidence.
- [ ] 7.3 Verify security and privacy parity: no secrets in arguments, logs, toasts, registry or artifacts; fictional data only; human approval for agent control; scanner clean on all Windows artifacts.
- [ ] 7.4 Publish Windows artifacts only with every Windows gate checked, a green `ci` run on `main` for the tagged commit and the owner's decision; verify downloads and checksums.
