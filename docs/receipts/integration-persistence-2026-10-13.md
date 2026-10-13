# Linux integration, persistence and AppImage receipt — 2026-10-13

**Result: call history and contacts persist in SQLite and drive the Calls and Contacts tabs; a second launch forwards to the running window; `sip:`/`tel:` links only fill the number; incoming calls raise desktop notifications; the Docker build exports an AppImage that wraps the same ELF. Not a release: real desktops, devices and the remaining gates are open.**

Baseline: `43fb5ee` ([production engine receipt](production-engine-2026-10-13.md)). No new registry packages beyond rusqlite/libsqlite3-sys and their reviewed helpers ([ADR 0012 addendum](../adr/0012-production-dependency-graph.md#addendum-sqlite-persistence-2026-10-13)); `rust-microsip-platform` reuses the zbus 5.19.0 already in the graph with its async-io backend only.

## What was added

- **Storage** (`crates/storage`): schema v1 with transactional migrations, refusal to touch a newer schema, 0700/0600 database, WAL and SHM files, call history (deduplicated per session key, outcomes from the end reason, monotonic durations), 90-day retention, contacts with multiple labelled numbers and favorites, literal (escaped) search, bounded RFC 4180 CSV preview with per-line issues and an all-or-nothing import, online backups that never overwrite.
- **Desktop**: Calls tab with All/Incoming/Outgoing/Missed filters, local search, redial and confirmed deletion; Contacts tab with editor, favorites first, search and CSV import screen; Settings → audio input/output device selection saved to the configuration (applied at the next start). If the database cannot open, an in-memory store keeps the session usable with a visible notice and the file is left untouched.
- **Linux integration** (`crates/platform`): owner election with `flock` in a 0700 `$XDG_RUNTIME_DIR/rust-microsip`, a 0600 Unix socket and a bounded `v1` line protocol with a total per-connection deadline; strict `sip:`/`tel:` parsing (no parameters or headers, no CR/LF, single percent-decoding, host validated then ignored so a link cannot redirect signaling); incoming-call notifications with Answer/Reject accepted only from the notification service's owner and only while that call still rings.
- **Single instance policy**: a second launch forwards its request and exits before touching SIP, audio or the database. If a live owner does not answer, or the runtime directory is unsafe, the launch exits with an error instead of starting a second instance. Without `XDG_RUNTIME_DIR` the app runs in a documented degraded mode with a warning.
- **AppImage** (`packaging/appimage`): type-2 runtime pinned by release tag and SHA-256, reproducible squashfs, original icon, desktop entry, licences for the app, Rust dependencies, toolchain and runtime components; no bundled libraries.

## Evidence

- Workspace: fmt, clippy `-D warnings`, all tests (storage 12, platform 14, app 16 unit + 7 process, plus the earlier suites) and rustdoc pass; the clean-context Docker build (`sh packaging/check-context.sh`) passed including the AppImage stage.
- **Engine scenarios against the lab PBX** (in the [production engine receipt](production-engine-2026-10-13.md)): wrong password without retries, a second incoming call refused with 486 while the active call keeps audio and DND is switched on, reverse-direction calls, and CANCEL racing the 200 OK without leaving a call on either side, over UDP/RTP and TLS/SRTP, with the third party on a TCP account.
- **GUI smoke of the release ELF** (private Xvfb, lab PBX, fictional 1001 UDP account): while the window ran, a second launch with `sip:602@pbx` exited 0; the first window showed `602` in the number field and dialled only after Enter; the PBX reported one active call; SIGTERM exited 0 after unregistering. Capture SHA-256 of the filled number: `deb4e6c7824c0054a985dd5937e167b8a6ebf7644ff096073117d15d0c4fc3c9`.
- **Exported artifacts** (clean-context build): ELF `40a79d1d8ff11d0d883b7d94d0c517035b5941e04b2b99fd9c89e90cff024b72`, AppImage `f14927fb419a20db760f4e706c115db22c4a6e7c9dc3b419f835549d02635641`; `packaging/appimage/verify.sh` confirmed the identical ELF inside, the pinned runtime prefix, the desktop entry, a 256×256 icon and no bundled shared objects. Without FUSE, `--appimage-extract-and-run --version` and extraction followed by `AppRun --version` both ran; a wrong ELF made the verifier fail.
- **Reviews**: two Claude CLI static rounds ([record](../reviews/claude-integration-persistence-2026-10-13.md)); round 1 blocked on a single-instance fail-open path, fixed as described above; round 2 approved.

## Not established

Notifications, tray and links on real GNOME/KDE sessions; device selection and hotplug with real hardware; crash recovery of interrupted calls; opt-in `sip:` handler registration from the UI; LGPL relinking terms for the statically linked libfuse in the AppImage runtime (release review); SO_PEERCRED verification (deferred to the CLI/MCP IPC work); human audio; VM acceptance; publication.
