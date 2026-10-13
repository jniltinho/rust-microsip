# Configuration, secrets, recovery and release engineering receipt — 2026-10-13

**Result: configuration now resolves through a SQLite layer that stores only user edits; passwords can be kept in the system keyring by opt-in; interrupted calls are recovered into history; the engine survives a network outage and renews registrations; CI, a publication scanner and a reproducible clean-clone build exist. Not a release.**

Baseline: `1145bcd` ([integration and persistence receipt](integration-persistence-2026-10-13.md)). No new registry packages.

## Configuration and secrets

- Storage schema v2 adds `accounts` (one enabled account by a partial unique index, references only — no password column) and `preferences`; v3 adds an open-call journal. Each migration runs in one transaction after an online-backup copy (`<db>.v<N>.bak`, 0600, never overwritten). Tests cover upgrades, an injected mid-migration failure (database left at the old version, backup present, next open completes), a locked database (`Busy`, no data loss), a simulated full disk (`Full`, rollback), corrupted and future-version files left untouched.
- `rust-microsip-config` resolves flags > environment > explicit `.env` > TOML > SQLite > defaults with per-field provenance (32 precedence combinations tested). Saving from the window writes only the fields the user changed; a field set by a higher layer is refused with its source named, so overrides are never persisted. `config import FILE` previews a sanitized TOML and `--apply` replaces the stored settings atomically; the source file is never modified and a literal password in it is ignored.
- Opt-in "Remember password in the system keyring": the password goes to Secret Service on a background thread; the database stores only `user@domain` as a reference once the keyring confirms. At start, when no other source provides a password, it is read in the background and registration follows. Unchecking removes the item. The secret store passed a real round trip against gnome-keyring in a disposable `dbus-run-session` with fictional data.

## Calls, links and desktop behaviour

- Crash recovery: a call is journaled when it starts and completed when it ends; rows left open by a killed process are marked `Interrupted` at the next start (simulated kill test).
- Closing the window during a call asks "Hang up and quit?"; SIGTERM/SIGINT still shut down without a prompt within the bounded engine shutdown.
- Opt-in "Handle sip:/tel: links" writes a 0644 desktop entry atomically under `$XDG_DATA_HOME/applications` and shows the `xdg-mime` command to make it default; nothing is executed automatically. "Remove" deletes it.
- Settings → Audio → Test plays a short tone and shows the microphone level without a call (disabled while a call owns the devices).

## Engine: renewal and network loss

`crates/telephony/examples/engine_network.rs` against the lab PBX (log SHA-256 `61708a2e40b472a90e450148eeb1bc54c4aa0f4584ffbb78e57b5e427382a850`, ELF `e52765595869b4e289fd88583f341ac68df8ca3da64479dc9a8accb62a028754`):

| Check | Result |
| --- | --- |
| Registration with `expires=60`, PBX contact after 70 s | present (renewed) |
| Network cut for 40 s during an echo call | call ended `Failed` 30 s after the cut (RTP inactivity); exactly one call, no automatic redial |
| After the network returned | the in-flight REGISTER completed, a prompt re-registration followed, the PBX listed the contact and **zero channels**: the BYE deferred until the registrar answered was delivered |
| Shutdown | unregistered (contact removed) |

Two engine changes came from these runs: BYEs after RTP inactivity wait for a registrar answer (the SDK marks a dialog ended even when its BYE is lost), and a REGISTER that only completes after retransmissions triggers a re-registration two seconds later. An engine-side trace showed the earlier "missing contact" was the test's own unregister, not a lost binding; the test now waits for the host check.

The full engine lab (log SHA-256 `eae5daa874e3df15fac8f7ff01a63cfd733a49f758400386b1866d66df7bcb4b`) and the GUI smoke of the release ELF passed again after these changes.

## Release engineering

- `.github/workflows/ci.yml`: Docker `verify` and `artifacts` targets, checksum and AppImage verification, publication scans and the clean-context check; `actions/checkout` pinned by commit, `contents: read`, no secrets. It has not run on GitHub yet.
- `packaging/release/scan-publication.sh` checks tracked/staged content and artifact directories (including extracted AppImages) for forbidden paths, private keys, token formats, personal home paths and the lab's fictional passwords without printing values; it passed on HEAD and on the exported artifacts.
- `packaging/release/ci-selftest.sh` proves that a staged `base/` path, a `.env`, a lab password, a private key, a token, a personal path and an injected rustfmt violation each fail the pipeline, on a `git archive` copy.
- `packaging/release/clean-clone-build.sh` built `1145bcd` from a clean local clone without host Rust; a no-cache rebuild produced an identical `SHA256SUMS` (same-machine reproducibility).
- `docs/release/v0.1.0-draft.md` is a draft with scope, limits, verification and rollback steps and the remaining gate checklist — not a release.

## Independent review

Two Claude CLI static rounds ([record](../reviews/claude-config-recovery-release-2026-10-13.md)). Round 1 blocked on deferred BYEs lost when the stack was torn down, duplicate refresh chains after inactivity probes, and scanner gaps (binary/UTF-16 blobs, history, the app's own TOML passwords). All were fixed — BYEs drained before shutdown, a single sequenced refresh timer, one fast retry per slow REGISTER, a refresh chain that cannot die silently, a bounded shutdown budget; the scanner reads binary and UTF-16 content, every historical blob and path, literal TOML passwords in all common forms (also inside artifacts and AppImages) and secret-like file names — plus the app notes (fail-closed saving without provenance, keyring reference bound to server and transport, recovery only with the single-instance lock, no D-Bus or device probing on the UI thread, second-signal exit, guarded link-handler removal). Round 2 approved. The engine lab, the network-loss test and the GUI smoke were rerun after the fixes.

## Not established

Real keyring prompts and KWallet, real audio devices for the local test, notification/tray on real desktops, NAT/STUN, VM acceptance, human audio, `cargo deny/audit` and coverage-guided fuzzing in CI, the first GitHub Actions run, the CLI/MCP extension and publication.
