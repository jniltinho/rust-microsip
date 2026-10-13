# Hardening receipt: layering, fuzzing, supervision, themes and scaling (2026-10-13)

**Result: an automated layering rule, bounded deterministic fuzzing of the main outside-input parsers, a paused-clock timer test, engine supervision, a fix for lost final events, a RustSec/source check in CI, and runtime checks of theme switching during a call, exit with a frozen PBX and 100/150/200% scaling. Not a release.**

Baseline: `6324e2f` ([config/recovery/release receipt](config-recovery-release-2026-10-13.md)). No new crates: `serde_json` (already locked) became a dev-dependency of `rust-microsip-core`, and the telephony tests enable Tokio's `test-util` feature. `deny.toml` configures the CI-only cargo-deny action.

## First GitHub Actions run

Run `38423434363` on `6324e2f` succeeded: publication scan with negative controls, Docker `verify` (fmt, clippy `-D warnings`, tests, doctests, rustdoc), `artifacts`, checksums, AppImage against the ELF without FUSE, artifact scan and the Docker context allowlist with its negative control.

## Code and tests

| Area | Change | Check |
| --- | --- | --- |
| Layering | `crates/core/tests/layering.rs` reads `cargo metadata --no-deps --offline --locked`: each workspace package's normal dependencies (by real package name, including renamed and target-specific ones) are allowlisted — the domain may use only `thiserror` and `secrecy`; SQL, SIP, audio and D-Bus each stay in one crate — and every package needs a rule. The UI module (recursively) may not name `rusqlite`, `rsipstack`, `rustls`, `tokio`, `cpal`, `zbus`, `net::`, socket types, the engine or media crates | 3 tests; the negative control injects a renamed, a target-specific and an unruled package plus a UI socket import |
| Fake clock | Registration refresh/backoff are pure functions (`refresh_delay`, `retry_delay`); the actor's refresh timer runs on Tokio's paused clock in tests; SRTP replay and rekey windows take an explicit `Instant` | `registration_timing_follows_the_policy` (80% refresh, 2 s fast retry, 10→300 s backoff, attempt limit, no retry on bad credentials); `refresh_timer_fires_on_the_fake_clock_and_rearming_supersedes` (fires at exactly 48 s of virtual time; a re-armed timer carries the newer sequence) |
| Fuzzing | Deterministic, bounded mutation fuzzing (text parsers receive lossily decoded strings; SIP headers, digest, `Refer-To`, TOML import, the single-instance socket protocol and the SIP stack itself are not covered) (xorshift seeds, so a failure reproduces): dial strings, `sip:`/`tel:` links, CSV import, `.env`, command-line arguments (20 000 inputs each), SDP offers under both media policies (20 000), RTP and RFC 4733 parsing (50 000) | no panic; runs in `cargo test` and therefore in CI |
| Supervision | The engine actor runs under `catch_unwind` (default `panic = "unwind"`); a panic sends `Notice::EngineFailed` (waiting at most 1 s for queue space) and wakes the UI; a panic in a spawned per-call task ends only that task. A closed command queue reports `EngineFailed` instead of `Overloaded`, except after a normal stop. The status bar keeps "Engine stopped; restart" until restart | `supervisor_reports_a_panicking_engine` (injected panic), `engine_failure_is_sticky_in_the_status` |
| Final events | **Defect found by the new saturation test:** on shutdown the actor flushed once and dropped the rest of the backlog, so `Stopped` could be lost behind a slow UI. It now drains the backlog for up to 1 s, reserving queue space before taking each event (the drain only waits when the event queue is full) | `command_flood_never_blocks_and_stopped_is_delivered`: 20 000 commands sent without blocking (bound 10 s for shared CI runners; about 0.2 s locally) with nobody reading events, `Stopped` is the last event delivered (run 3 times locally) |
| Contrast | WCAG 2.x ratio test of the text, ok, warn and error colors on the panel and window backgrounds of both themes | all ≥ 4.5:1 |
| Themes | A theme change during a call sends no engine command and keeps the call | `switching_theme_during_a_call_leaves_the_call_alone` |
| CI | New `supply-chain` job: `cargo deny check advisories sources` (RustSec vulnerabilities, unmaintained and yanked crates; crates.io only), action pinned by commit | first run pending at the time of writing; `Cargo.lock` has no git sources; the action builds its image from the pinned commit's Dockerfile, whose base image and cargo-deny binary are fetched by tag, not by digest; advisories reported on the first run are to be resolved one by one, not by relaxing `deny.toml` |

`cargo fmt --all --check`, `cargo clippy --frozen --workspace --all-targets -- -D warnings` and `cargo test --frozen --workspace` passed in the pinned toolchain container after the batch, and `sh packaging/check-context.sh` (clean-context Docker build including the AppImage stage) passed.

## Runtime checks (release ELF, private Xvfb, lab PBX, fictional 1001 account)

| Scenario | Result |
| --- | --- |
| Dark theme chosen in Settings during an echo call to 600, then back to the dialpad | PBX showed one active channel at each step; the call timer kept running; capture `app-in-call-dark.png` SHA-256 `0522577fe5fa3c14c991d1c1c02c7461cbc5b2c4c69bca530d80bb896acb695f` |
| Relaunch after the switch | dark theme restored from the database (unversioned capture, SHA-256 of the PNG `eabc311f277800d88861856d175f8169e6ede15e0afc3b4f19d3e6a707f30975`) |
| No theme service (no D-Bus or portal in the container) with "System" | light theme, no error (unversioned capture, SHA-256 of the PNG `d7639a9cdb45390830ce255a2f91f42280dc7be97b284f116f3afaaf01ebf96d`) |
| SIGTERM during a call while the PBX container is paused (`docker pause`) | process exited after 2.5 s with the documented signal status 143 (within the app's 4 s engine-shutdown wait); no process left; lab torn down with no leftover containers |
| 1024x768 screen at 100/150/200% (`WINIT_X11_SCALE_FACTOR`) | 300x460, 450x690 and 600x768 physical; at 200% the requested 920 px height was clamped to the screen and the main panel scrolls, so every control stays reachable (Enter dials, Escape hangs up) |

An apparent dropped call during this work was a harness race (the host counted PBX channels while the app was already being terminated); a 40 s echo call held one channel throughout.

## Mute keeps reception (lab)

`engine_lab` now also measures the muted side while muted. Full run, both profiles passed (log SHA-256 `142b16a39d0fbc36ee79eee1b56d7081463a89ec1fa5436b1337592da916279b`, example ELF `0e914aaceece6a8b359c976cb5ece551032bbcf175330c8280e89f8a9b5372d1`):

| Profile | Peer while muted (RMS) | Muted side hears the peer |
| --- | --- | --- |
| UDP/RTP | 8 | 81 frames, RMS 5675, tone ratio 1662 |
| TLS/SRTP | 8 | 80 frames, RMS 5675, tone ratio 1670 |

The local audio test on real devices (the other half of AUD-04) is still open.

## Independent review

Four Claude CLI static rounds ([record](../reviews/claude-hardening-2026-10-13.md)); round 4 approved. Round 2 contained findings about code that does not exist and was checked against the tree.

## Open points

- Worst-case shutdown is the engine's own shutdown (hang-up and unregister, 1 s each) plus up to 1 s of event drain plus the runtime's 2 s, which can exceed the app's 4 s wait; the drain only waits when the event queue is full. Task 8.4 stays open.

## Not established

Audio device release on real hardware (the containers have no sound card), screen-reader/AT-SPI labels, the first `supply-chain` CI run, coverage-guided fuzzing, real desktops and VMs, human audio and publication.
