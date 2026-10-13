# Agent grants receipt: grants, headless service, on-demand audio, tray (2026-10-13)

**Result: a person can issue a scoped, quota-limited, revocable grant in the window; a client holding it acts without a prompt only inside that scope; a single-use bootstrap starts a headless `service` owner with no display; audio devices open only for a call and are released after it. The full lab suite passed 9/9 on commit `ec52f12` on a clean tree. Human audio acceptance (AG-06) is not done; this is not the extension's acceptance.**

Usage and limits: [Controlling rust-microsip](../agent-cli-mcp.md). Earlier stage: [agent control receipt](agent-control-2026-10-13.md).

## What exists

- **Grants** (Settings → Agent access): scopes (reads, call, DTMF, DND, answer), allowed destinations, lifetime and quota. They are bound to the session epoch and stored as the SHA-256 of the capability only (schema v5). The capability is written to a 0600 file in the private runtime directory and never shown or logged. A client reads it with `--grant-file`, opened with `O_NOFOLLOW | O_NONBLOCK`, and every check runs on that descriptor. "Deny all and pause" also suspends grants and fails their queued work.
- **Headless service**: `rust-microsip service --grant-file <bootstrap>` needs a single-use bootstrap exported from the window. It refuses to start without a valid private one, runs without `DISPLAY`, accepts only the bootstrap grant's scope, and turns later window launches away (exit 4).
- **On-demand audio**: devices open when a call's media starts and close after it. They are reference-counted, so overlapping media restarts are safe. The release guard is made on the blocking thread before `start`, so a call that ends while devices are still opening, or a panic in `start`, still releases them. The service logs "microphone active" and "microphone released".
- **Desktop**: the window uses the app icon. A tray icon (`StatusNotifierItem` and `com.canonical.dbusmenu` over zbus, no new dependency) shows the window on click and offers "Show rust-microsip" and "Quit"; Quit asks first during a call.
- **Release tooling**: `release.yml` checks the protected `release` environment (required reviewer, admin bypass off, `v*` tags only), green CI on `main`, the workspace version, and every gate. `check-gates.sh` requires exact per-change labels and has negative controls in `check-gates-selftest.sh`, which CI runs.

## Tests

- Workspace in the pinned container: fmt, clippy `-D warnings`, rustdoc `-D warnings`, 285 tests passing. New tests cover:
  - grants are epoch-bound, quota-limited and revocable;
  - bootstraps are consumed once;
  - deny-all stops grants and their queue;
  - the service refuses a missing, foreign or non-private bootstrap;
  - devices are released after a call, on abort, and on abort while opening;
  - the private file reader refuses a symlink, a 0640 mode, a directory, and an oversized file;
  - tray menu layout, click mapping and RGBA→ARGB conversion;
  - the window icon decodes.
- `make context` (allowlist audit, which now covers every app source file), `make selftest`, `make scan`.

## Lab: `gui-grants` against Asterisk (run `20261013T223831Z-ec52f12`)

Release ELF `6aeb65acf14f75e97f25a4030f43c1e962b80e6a556fe623369893552345bd0d`, image `sha256:299470694b9adb5bc97245a473129f2fa7cf57b42e5b0659b686a972e2911804`, `summary.json` SHA-256 `8511758c6461bacec6931a0f295f17ae8b5da923a9b73fc63cad8323f980b8b7`. All 9 scenarios passed with no uncommitted changes and no lab objects left. ALSA uses a null PCM in the runner, so devices really open and close.

| Step | Result |
| --- | --- |
| Grant issued in the window (AT-SPI clicks) | file mode `600` |
| `history list` with the grant | exit 0 |
| `call dial 602` (outside the grant) | `approval_required`, **0 channels** |
| `call dial 600` (inside) | `accepted` without a prompt, 1 channel; window focus stays on the main frame |
| Hang-up | `accepted`, 0 channels |
| Second dial (quota used) | `approval_required` |
| After revoke | exit 3 |
| Bootstrap exported | file mode `600` |
| Service started with `DISPLAY` empty | mode `service`, `registered` |
| Window launched while the service runs | exit 4 |
| Service: dial 602 / dial 600 / hang-up | `grant_required` / `accepted` (1 channel) / 0 channels |
| Microphone | "active" logged at the call, "released" after it |
| Service stop | exit 0 |
| Bootstrap reused | exit 3, recorded as consumed |
| Capability text in captures and the data directory | **0** occurrences |

## Desktop check (KDE Plasma X11, owner's machine, scratch settings)

`dist/` was built with `make dist` and checked with `make dist-check`. Both the ELF and the AppImage were run with scratch `XDG_*` directories:

- `_NET_WM_ICON` is 256×256;
- the item is listed by `org.kde.StatusNotifierWatcher` and the icon appears in the panel;
- `GetLayout` returns Show, a separator and Quit;
- `Activate` answers;
- menu Quit closes the app (exit 0, AppImage runtime gone).

This is not the task 8.1 check, which needs desktops with and without tray support.

## Independent review

Four Claude CLI rounds ([record](../reviews/claude-agent-grants-2026-10-13.md)).

## Not established

- Answer, reject, DTMF, mute, hold, transfer and DND through CLI and MCP in the lab (task 3.3).
- Ledger crash recovery across epochs with a new grant and no automatic retry, as a lab run (task 2.3).
- Audit deletion confirmed from the window (task 2.5).
- Human audio with headsets in the G4 topology (task 3.4, AG-06).
- Tray and notifications on a desktop without `StatusNotifierItem` support, such as GNOME without the extension (task 8.1).
