# ADR 0012: Production dependency graph and notice policy

Date: 2026-10-13. Status: **accepted engineering boundary for the merged production
workspace; not a legal certification, vulnerability audit or release approval.**

## Context

The production workspace (`Cargo.toml`, `Cargo.lock`, 450 lock entries) merges:

- the reviewed secure SIP/SRTP graph ([ADR 0004](0004-experimental-sip-adapter.md),
  [ADR 0010](0010-isolated-srtp-packet-adapter.md), [ADR 0011](0011-isolated-initial-tls-sdes-calls.md));
- the reviewed eframe/wgpu native UI graph (G0 native UI dependency review, 2026-10-13);
- audio and configuration crates: cpal 0.18.2, alsa 0.11.0, alsa-sys 0.4.0,
  dasp_sample 0.11.0, toml 1.1.8, toml_writer 1.1.3 and serde_spanned 1.1.2.

For `x86_64-unknown-linux-gnu`, `cargo tree -e normal,build[,dev]` selects 300 identities:
6 own workspace members and 294 registry packages. `cargo metadata` resolves 17 more
registry packages (optional-feature superset: `egui_glow`, `glutin*`, `gl_generator`,
`khronos_api`, `tiff`, `zune-*`, `zlib-rs`, `xml-rs`, `jobserver`, `fax`, `weezl`,
`quick-error`, `glifo`) that are not built. They are listed as
`metadata_only_not_built` and are **not** approved; any feature change that selects
them requires a new review.

## Decision

### Notice collection

`packaging/collect-cargo-notices.py` now takes the `cargo tree` normal/build/dev
selection and inventories the intersection with `cargo metadata`. A tree identity
missing from metadata is rejected.

- **License expressions.** The allowlist is unchanged. An expression outside it is
  accepted only on an exact match of `(name, version, archive_sha256,
  declared_expression)` in `packaging/reviewed-licenses.json`, which also records the
  chosen license and the reason. No wildcards or version ranges exist. 35 exceptions
  are recorded (33 from the UI review, plus cpal and alsa).
- **Supplemental notices.** Archives without notice files (15 packages: AccessKit x5,
  egui family x7, profiling, spirv, dasp_sample) use commit-pinned upstream files under
  `packaging/third-party-notices/<name>-<version>/`. `manifest.json` records the exact
  archive checksum, raw URL at a 40-hex commit and the file sha256. The collector uses
  them only for that exact archive, verifies every hash and rejects collisions with
  packaged notices. The UI-review files were re-verified against their recorded hashes
  before copying. The dasp_sample files were fetched from
  `rustaudio/sample@97c3bb9b2363c0b46ac1633858bf1054fd02a980` (the commit in its
  `.cargo_vcs_info.json`). Its upstream `LICENSE-APACHE` is only a short Apache header,
  so the MIT option (complete text) is the license path used.
- **Nested license directories.** Files under any `LICENSES/` or `LICENCES/` directory
  are retained, not only files whose basename looks like a notice. This retains the
  wayland-protocols-plasma `LICENSES/{LGPL-2.1-or-later,MIT-CMU,MIT,BSD-3-Clause,CC0-1.0}.txt`
  set along with `COPYING.LIB`.

### License choices (summary)

| Basis | Packages | Path |
| --- | ---: | --- |
| Allowlisted expression | 258 | Declared expression; complete packaged texts retained |
| Legacy reviewed (`version_check` 0.9.5) | 1 | Both texts retained |
| Exception, MIT option | 15 | e.g. `rustix`, `linux-raw-sys`, `zerocopy`, `bytemuck`, `adler2` |
| Exception, Apache-2.0 | 10 | Apache-only crates (`winit`, `cpal`, `spirv`, ...), `self_cell` (Apache-2.0 option, not GPL-2.0-only), `moxcms`/`pxfm` |
| Exception, Apache-2.0 AND MIT | 9 | Legacy slash expressions (`alsa`, `downcast-rs`, ...) and `dpi` (with libm MIT notice) |
| Exception, Zlib | 1 | `foldhash` |

Apache-2.0 material flows only in the Apache-2.0 to GPL-3.0-or-later direction (see
[ADR 0003](0003-g0-license-and-native-boundaries.md)); NOTICE files are retained when
packaged. The full per-package list is `docs/inventory/production-graph.json`.

### Runtime

- Tokio is used only by the SIP/RTP worker (rsipstack, rustrtc, own telephony).
- AccessKit's AT-SPI bridge uses zbus with `async-io` on its own executor. The selected
  zbus has `async-io` and not `tokio`; `packaging/check-g0-dependencies.py` keeps
  rejecting a zbus `tokio`+`async-io` union and passes on the merged tree.
- Any future own D-Bus use (notifications, IPC) must reuse the same async-io zbus and
  must not enable its `tokio` feature.
- Audio callbacks run on cpal/ALSA threads and do not touch the Tokio runtime.

### Native dependencies

`alsa-sys` declares `links = "alsa"` and links the system `libasound.so.2` found via
pkg-config (`libasound2-dev` in the toolchain image). `libasound.so.2` is an allowed
`DT_NEEDED` entry (LGPL-2.1-or-later system library, dynamically linked, not
redistributed by this project). Graphics, windowing and accessibility libraries keep
the dynamic-loading boundaries recorded in the UI review.

## Not claimed / open

- No legal certification, no vulnerability or advisory audit, no full-source review.
- No ELF `DT_NEEDED` verification of a release binary yet.
- Embedded copyright headers in source files (for example the protocol XML
  `<copyright>` blocks and libm preambles) are not extracted by the collector.
  `wayland-protocols-misc` compiles `server-decoration.xml` (LGPL-2.1-or-later,
  copyright Martin Gräßlin) but ships no license text; the identical LGPL-2.1-or-later
  text is retained only via wayland-protocols-plasma. Extracting XML copyright blocks
  (or a pinned upstream supplement for misc) remains a release gate.
- The Docker context allowlist (`.dockerignore`, the `context-audit` stage and
  `packaging/check-context.sh`) does not yet admit the new data files or the new
  crates; the Docker `verify` stage needs them before it can run this collector.

## Addendum: SQLite persistence (2026-10-13)

`rust-microsip-storage` adds rusqlite 0.40.2 (MIT) with `bundled` and `backup`,
libsqlite3-sys 0.38.2 (MIT, compiles the bundled SQLite amalgamation, which is in the
public domain per its source header and <https://sqlite.org/copyright.html>),
fallible-iterator 0.3.0, fallible-streaming-iterator 0.1.9 and the build-only vcpkg
0.2.15. The last three declare the legacy `MIT/Apache-2.0` spelling; exact-archive
exceptions record the conservative both-license path with both complete texts
retained. No extension loading, SQLCipher or external server is enabled; no
additional runtime shared library is needed (SQLite is linked statically).
