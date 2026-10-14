# Claude CLI review: link handler entry, desktop VMs and the Wayland bridge (2026-10-13)

Static reviews with `claude -p --tools "" --strict-mcp-config --disable-slash-commands --no-session-persistence`, diff text only. A review is not runtime evidence; see the [desktop VM receipt](../receipts/desktop-vms-2026-10-13.md).

## Link handler entry and VM scripts

| Round | Verdict | Findings and resolution |
| --- | --- | --- |
| 1 | BLOCK | Problems found:<ul><li>an inherited `$APPIMAGE` from another AppImage could become the handler;</li><li>a `(deleted)` executable path was accepted;</li><li>`=` was not rejected in `Exec`;</li><li>the recognizer accepted any absolute `Exec`, so it could delete a user's edit;</li><li>removal followed symlinks and could block on a FIFO;</li><li>an orphan icon was never removed;</li><li>VM scripts had an unsigned `SHA256SUMS`, a predictable `/tmp` state file and an `eval` of another process's environment.</li></ul>Fixed:<ul><li>`$APPIMAGE` is trusted only under `$APPDIR`, and only existing files count;</li><li>`=` is rejected;</li><li>an entry is removed only on an exact match with what this executable would write;</li><li>reads use `O_NOFOLLOW` with a bounded size;</li><li>the icon goes with our entry;</li><li>VM scripts verify signatures with gpgv, keep state in `$XDG_RUNTIME_DIR` and import only listed variables.</li></ul> |
| 2 | BLOCK | The icon was removed even when the entry was kept, and the orphan test passed without exercising anything: both fixed, with assertions. Also fixed: `${XDG_RUNTIME_DIR:?}`, an exact scratch guard, and refusing a key directory inside a Git work tree. |
| 3 | BLOCK | Fedora's clearsigned `CHECKSUM` was parsed outside the signed text: only `gpgv --output` content is parsed now (the hash used for the existing VM was confirmed against the signed text). The provisioning marker is now written only when every step succeeds, `git` is required, and the test skips paths containing `=`. |

## Wayland bridge

| Round | Verdict | Findings and resolution |
| --- | --- | --- |
| 1 | BLOCK | A tray Quit exiting without the window could drop a call (with several calls) or lose history that the window records: removed, so a tray Quit waits for the window. The SIGTERM fallback could race the window's shutdown: the window's teardown now marks the loop alive. A later panic was reported as a missing library: only "could not be loaded" counts, and any other panic propagates. A bridge thread that fails to spawn is now a startup error. Accepted and documented: control requests and tray Show wait for a covered window on Wayland. Already safe: the engine ignores Answer/Reject unless the call is ringing. |
| 2 | PASS | Notes applied: the SIGTERM fallback documents the possible loss of history. The `join` in the bridge's drop cannot block on D-Bus, because notifier calls only enqueue. The release profile keeps `panic = "unwind"`. |
