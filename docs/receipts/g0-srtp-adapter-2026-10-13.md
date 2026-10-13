# Isolated SRTP/SRTCP packet-adapter receipt — 2026-10-13 UTC

**Result: verified per-SSRC packet primitive, not a secure call or release.**
Baseline: `3d4e5558bb09d9cf25f4112c2fe208b6b40efdab`.
The application workspace remains unchanged; the module is an explicit isolated
copy under [ADR 0010](../adr/0010-isolated-srtp-packet-adapter.md).

## Pre-compilation gate

The exact no-default `rustrtc 0.4.7` candidate and existing-inventory
`thiserror 2.0.21` / `zeroize 1.9.1` were selected only in the copied media
manifest. Actual normal/build/dev tree deny checks pass. All 100 selected
registry versions, archive checksums, metadata features and 195 preserved notice
hashes match the [previous inventory](../inventory/g0-srtp-candidate-cargo.json).
The source/cache comparison was repeated for all 4,548 archive files before
compilation. No new package, native provider, codec, or candidate feature was
introduced. This is the reviewed evaluation boundary, not application adoption.

The [isolated lock](../inventory/g0-srtp-adapter.Cargo.lock) and
[machine receipt](g0-srtp-adapter-2026-10-13.json) identify the exact own source,
candidate manifest/library changes, lock, tests and safe result hashes. Evidence
is implementer-recorded, not independent cryptographic/build attestation.

## Actual checks

- **TDD RED:** the initial wire-through implementation returned a second clear
  packet where the specific Replay error was required: one intended assertion
  failure, test exit 101. This was not a compilation error or an ignored test.
- **29 adapter tests pass**, including:
  - exact replay/too-old errors and the 63/64-packet window boundary;
  - tampered ciphertext/tag/header and forged rollover without state poisoning;
  - RTP reordering across rollover, plus a forward gap across wrap;
  - 1,000 failed first-packet SSRCs without source learning/retained sessions;
  - authenticated-other-source rejection for a bound per-SSRC context;
  - separate SRTCP index/window, tag/index/E-bit failures, and authenticated
    malformed control both before and after source binding;
  - transmit duplicate/backward-index rejection and both shared-key lifetime
    gates closing both send paths (boundary state injection, not 2^48 packets);
  - fixed profile overhead, CSRC/extension/padding roundtrip, header-field
    authentication, valid-header oversize rejection and maximum-size packets;
  - exact key length, redacted Debug, and receive-only provider transmit failure;
  - **30,000** seeded bitmap operations against an independent bounded-set
    state model. This models replay bookkeeping, not the cipher or network.
- **Three non-Clone compile-fail doctests pass** for master material, sender and
  receiver. They are compile-time API checks, not runtime call tests.
- **Three deliberate code mutants fail their exact assertions with exit 101**:
  replay rejection removed, replay committed before authentication, and source
  bound before authentication. No arbitrary build failure counts as detection.
  The correct source was restored and the full suite rerun afterward.
- Full isolated-workspace fmt/clippy/tests/doctests/rustdoc checks pass; normal
  tests are **10 CLI / 12 existing media / 29 adapter / 28 telephony /
  6 registration integration**. No new application feature was enabled.
- The synthetic-clone Docker context audit excludes both research modules and
  private env/reference sentinels. Its deliberately permissive filter produces
  the distinctive `CONTEXT_REJECTED` marker naming the intended sentinels. The
  safe clone builds the existing scaffold export and verifies its checksums.
- Separate sanitized, tools-disabled [Claude code review, round 1](../reviews/claude-srtp-adapter-round1-2026-10-13.md)
  returned **APPROVE for isolated packet-adapter evaluation only**. Suggested
  tests were added and executed; [round 2](../reviews/claude-srtp-adapter-round2-2026-10-13.md)
  returned **APPROVE for isolated per-SSRC packet-primitive evaluation only**.
  Review cannot replace runtime
  evidence, PBX interoperability or release acceptance.

All execution used the pinned Rust 1.99.0 Ubuntu 24.04 Docker toolchain, not host
Cargo. Candidate runs had no network/devices/desktop, a read-only root, invoking
UID/GID, dropped capabilities, no-new-privileges, 3 GiB/2 CPU/256-process limits,
and a bounded `/tmp` tmpfs. Only explicit own copied files and the dedicated
registry cache were mounted. No PBX call, personal configuration, installed
AppImage, production account, microphone or real voice was used.

## Reproduction

Use a disposable checkout of this receipt's commit and a fresh ignored work
directory. Copy only the 34 application build inputs listed and hash-bound in
the machine receipt; never recursively copy the checkout or a reference/HOME
directory. In that copy only:

1. Insert the following before `[dev-dependencies]` in `crates/media/Cargo.toml`:

   ```toml
   [dependencies]
   rustrtc = { version = "=0.4.7", default-features = false }
   thiserror = "=2.0.21"
   zeroize = "=1.9.1"
   ```

2. Copy `docs/inventory/g0-srtp-adapter.Cargo.lock` to the copy's `Cargo.lock`.
3. Copy `tests/probes/srtp_adapter.rs` to `crates/media/src/srtp_adapter.rs` and
   append the following to the copy's media `src/lib.rs`:

   ```rust
   /// Experimental isolated secure packet adapter.
   pub mod srtp_adapter;
   ```

4. Verify all 35 resulting input hashes against `candidate_copied_inputs_sha256`.
   Use the pinned Docker toolchain and a dedicated ignored Cargo cache, not the
   personal Cargo directory. Locked fetch/metadata do not compile dependencies.
   Repeat deny checks, complete notice collection/comparison, source/cache and
   native/license review before compiling. A changed graph is not covered here.
5. Inside the isolated Docker copy with network disabled, run:

   ```sh
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets --frozen -- -D warnings
   cargo test --workspace --all-targets --frozen
   cargo test --workspace --doc --frozen
   RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --frozen
   ```

The public preparation steps were repeated in a new explicit copy, with all 35
input hashes matching. That copy also passed fmt/clippy, all 29 adapter tests and
the three non-Clone doctests; its separate log hash is in the machine receipt.
The source/cache comparison still matched all 4,548 files after compilation.

The expected full suite is green only for this **adapted** packet primitive.
The separate [unadapted diagnostic](g0-srtp-candidate-2026-10-13.md) remains a
known failing acceptance test; do not put it into this green isolated suite or
misrepresent it as fixed in the upstream library.

## Still required

Actual TLS/SDES negotiation and encrypted Asterisk PCMA/PCMU calls in both
directions; downgrade/invalid crypto/key/auth/replay negatives; verified key and
bounded SSRC lifecycles for real secure call controls; teardown/leak scanning;
complete G0 desktop/device/IPC proof; exact-artifact human/VM acceptance; all
remaining production, CLI/MCP and release gates. Per-SSRC object reconstruction
under the same key is not an acceptable replay-reset workaround.

No task checkbox changed: **4/54**, with **1.7 and 5.4 open**. No usable softphone
or secure call/profile is delivered by these packet tests.
