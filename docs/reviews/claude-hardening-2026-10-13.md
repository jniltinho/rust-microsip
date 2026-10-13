# Claude CLI review: hardening batch (2026-10-13)

Static reviews with `claude -p --tools "" --strict-mcp-config --disable-slash-commands --no-session-persistence`, sanitized diff only, no repository access. A review is not runtime evidence; see the [hardening receipt](../receipts/hardening-2026-10-13.md).

| Round | Verdict | Findings and resolution |
| --- | --- | --- |
| 1 | BLOCK | Layering manifest parsing failed open (target-specific, table-form, renamed packages, member discovery) → replaced by `cargo metadata` with a negative control per form and a recursive UI scan. "Fake clock" was only pure functions → Tokio paused-clock test of the refresh timer. Overstated fuzzing scope → parsers listed, exclusions recorded. Supervision scope (unwind only, per-call tasks) → documented. Unbounded `blocking_send` after a panic → `try_send` with a 1 s deadline. Drain popped before reserving → reserve first. `EngineFailed` shown after a normal stop → ignored after `Stopped`. Tight 2 s flood bound. Draft checklist vs roadmap mismatch for 3.3/4.6 → real-desktop checks moved to the release row. |
| 2 | BLOCK | Two findings described code that does not exist (`let staged = 0;`, placeholder functions, a missing `retry_delay`); the author checked the working tree and index with `grep` and they were void. Valid minors fixed: Shutdown retried in the flood test, `EngineFailed` also ignored while quitting, CI comment lists the fuzzed parsers, cargo-deny image-tag caveat and the shutdown worst case recorded as an open 8.4 point. |
| 3 | BLOCK | The flood-test bound was still 2 s in code while the receipt said 10 s (an earlier edit had not applied after formatting) → set to 10 s. |
| 4 | APPROVE | Wording "repeated 3 times" changed to "run 3 times locally". |
