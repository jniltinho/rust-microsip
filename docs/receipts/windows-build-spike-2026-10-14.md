# Windows build spike receipt, milestone W-A (2026-10-14)

**Result: the workspace cross-compiles for `x86_64-pc-windows-gnu` in Docker, offline and `--frozen`, with no host toolchain. It produces a GUI exe and a console exe that import only Windows system DLLs. The Linux build, its 292 tests and the 9-scenario lab suite are unchanged. No real Windows run has been done yet; the shared VM was busy. Desktop integration on Windows is a set of stand-ins.**

Change: [add-windows-port](../../openspec/changes/add-windows-port/proposal.md) ([approval](../../openspec/changes/add-windows-port/approval.md)). Commit `2c70fc9`.

## Toolchain (task 1.2)

- Dockerfile stage `windows-toolchain`: mingw-w64 GCC 13 and binutils 2.41.90 from the same Ubuntu snapshot (`20261001T000000Z`), plus `rust-std-1.99.0-x86_64-pc-windows-gnu`. Its SHA-256 in `docker/rust-components-windows.sha256` matches the `.sha256` published by static.rust-lang.org.
- Link flags `-static` and `--no-insert-timestamp`; a build script embeds an 8 MiB main-thread stack and, through windres, the icon, version information and manifest (PerMonitorV2 DPI, UTF-8 code page, long paths, Windows 10+, asInvoker).
- With the container network off (`RMS_DOCKER_NET=none`), `cargo build --workspace --release --target x86_64-pc-windows-gnu --frozen` succeeded. That covers ring and the bundled SQLite.
- Imports of `rust-microsip.exe`, all of them Windows system DLLs: advapi32, api-ms-win-core-synch-l1-2-0, api-ms-win-core-winrt-error-l1-1-0, bcrypt, bcryptprimitives, combase, dwmapi, dxgi, gdi32, imm32, iphlpapi, kernel32, mmdevapi, msvcrt, ntdll, ole32, oleaut32, opengl32, shell32, uiautomationcore, user32, uxtheme and ws2_32.
- Subsystems: `rust-microsip.exe` is Windows GUI (2) and `rust-microsip-cli.exe` is console (3).
- `make windows` writes both exes, a deterministic zip (fixed member order and timestamps) and `SHA256SUMS` to `dist/windows/`.
  - `rust-microsip.exe`: `e7a7d9e0128d2902488f7b8f597c1b094b57da25d01764bdb49e7486c7f3b2de`
  - `rust-microsip-cli.exe`: `90700dd9857214446201cc10216426333cc38e200eb8024df6d73b32b28eabe4`

## Gating (task 1.3)

- `crates/platform`:
  - The protocol types (`ipc.rs`) and caller sanitizing (`caller.rs`) are shared by both platforms.
  - The Unix modules (socket instance and control, D-Bus notifications, StatusNotifierItem, Secret Service, rustix file checks) sit behind `cfg(unix)`; `zbus` and `rustix` became Unix-only dependencies.
  - `windows.rs` provides the same API, with everything reporting "unavailable": the app runs without single-instance protection and shows a warning, there is no control channel, no toasts and no tray, and passwords last only for the session. Milestone W-B replaces it with `crates/platform-windows`.
- `config` and `storage`: Unix owner and mode checks are behind `cfg(unix)`. On Windows, configuration goes in `%APPDATA%\rust-microsip` and data in `%LOCALAPPDATA%\rust-microsip`. The owner-only DACL check is task 2.6, so grant files are **refused** on Windows until then.
- Signals: SIGTERM/SIGINT on Unix and Ctrl+C on Windows go through one module. Session end is task 2.11.
- Unix-only tests are `cfg(unix)`: the config and storage modules, two unit tests that check modes, and the CLI/MCP process tests. The G0 lab examples are not built for Windows.
- Checks:
  - **Windows**: `cargo clippy --workspace --lib --bins --tests --target x86_64-pc-windows-gnu -D warnings` is clean, rustdoc is clean, and 16 test executables build.
  - **Linux**: fmt, clippy `--all-targets` and rustdoc are clean, with 292 tests passing.
  - **Context audit**: `make context` passes.
  - **Lab suite**: run `20261014T014507Z-2c70fc9` passed 9/9 on a clean tree with no lab objects left.

## Smoke under Wine (not evidence)

Wine 9 with a scratch prefix:
- `rust-microsip-cli.exe --version` prints `rust-microsip 0.1.0-rc.1`.
- `--doctor` reads `%APPDATA%` and reports the missing account.
- `rust-microsip.exe` opens the window, with the embedded icon.

Wine does not prove pipes, DACLs, Credential Manager or toasts (design W11).

## Not established

- A run on real Windows (task 1.4): the shared Server 2022 VM was busy, and runs there are coordinated with its owner first.
- Every adapter feature (W-B), packaging beyond the test zip (W-C), CI on `windows-latest` (5.1), and all VM and lab checks (W-D).
