# SRTP pre-adoption diagnostic receipt — 2026-10-13 UTC

**Result: inventory/evaluation milestone, not secure-media approval.**
Baseline: `da7a470ab923637c78606885f8d86f20b396c63a`.
The application's Cargo manifests and lockfile are unchanged. No PBX, account,
network call, microphone, desktop, personal configuration, or installed AppImage
was used by the candidate probe. No new release/tag is justified.

## Inputs and pre-compilation review

- [ADR 0009](../adr/0009-srtp-candidate-evaluation-boundary.md) records the boundary
  and unresolved adapter obligations.
- [Candidate lock](../inventory/g0-srtp-candidate.Cargo.lock): an isolated copy's
  lock, **not** the application's `Cargo.lock`.
- [Candidate inventory](../inventory/g0-srtp-candidate-cargo.json): 100 selected
  registry packages, 195 retained notice files; 27 packages added to the previous
  73-package inventory. All package identities agree with the actual Linux
  normal/build/dev tree. Metadata and tree features are recorded separately.
- [Source comparison](../inventory/g0-srtp-candidate-source.json): all 4,548 regular
  archive files in those 100 packages matched the cached source byte-for-byte.
  Archives were checksum-verified, traversed with size/member/path bounds, and
  read without extracting paths. The limited native/legal-marker scan is **not**
  an exhaustive legal, vulnerability, cryptographic, or source-security audit.
- Complete available notices were retained locally before compilation. Notice
  texts already present in the previous inventory were compared by hash; all 24
  new distinct texts were read. An additional archive scan found no regular
  notice files discoverable only by a `LICENSES`/`LICENCES`/`COPYING` directory
  component and omitted by the collector's basename rules, in this exact graph.
  This does not generalize notice completeness to future packages.
  The three added build scripts and selected
  dependency edges/native boundaries were inspected. No additional native crypto
  provider or blocked codec was selected. Third-party unsafe/intrinsics remain.
- `rustrtc =0.4.7`, `default-features=false`, empty feature union. The evaluated
  data plane is `SrtpContext`, not its high-level peer/DTLS/SDES generator.
- Existing Rust 1.99.0 Ubuntu 24.04 toolchain image, built from the pinned
  repository Dockerfile; no host Cargo. Candidate execution used no network,
  read-only root, host UID/GID, dropped capabilities, no-new-privileges,
  3 GiB/2 CPU/256-process limits, and a bounded executable `/tmp` tmpfs.
  Only the explicit isolated copy and dedicated registry cache were mounted.

The [machine-readable receipt](g0-srtp-candidate-2026-10-13.json) binds the
candidate manifest/lock/probe, inventory, source comparison, collector, and
verification evidence by SHA-256. This is implementer-recorded evidence, not an
independent build or cryptographic attestation. The 34 copied application build
inputs plus one explicitly copied probe were used; no recursive reference/HOME
copy took place.

## Actual execution

The [own isolated probe](../../tests/probes/srtp_candidate.rs) compiled, passed
formatting and clippy with warnings denied, then returned **exit 101**:

| Test | Observed result |
| --- | --- |
| Ciphertext differs from the fictional payload; ten-byte tag; payload roundtrip | PASS |
| Modified ciphertext fails with `AuthenticationFailed`; original still succeeds | PASS |
| Wrong key fails with `AuthenticationFailed` | PASS |
| Authenticated packets reorder across 65535 → 0 and recover the payload | PASS |
| A second copy of an authenticated packet must be rejected | **FAIL: duplicate accepted** |

Exactly **4 passed, 1 failed, 0 ignored**. The failure was the intended replay
assertion, not a parse error, missing dependency, or compilation failure. The
test remains an unsatisfied acceptance condition; it was not inverted or marked
ignored to manufacture a green SRTP result. This code is deliberately outside
Cargo's application test targets and Docker build context. No adaptation is
present yet. Self-roundtrips are not independent RFC test vectors, PBX SRTP
interoperability, human audio, or an authenticated signaling/SDES negotiation.

## Packaging regression checks

- **21** Python packaging tests pass. The newly added counterfeit-`rustrtc` test
  first failed against the old collector because the counterfeit was accepted;
  it passes after adding the independent archive pin. Legacy spelling is accepted
  only for pinned `version_check 0.9.5` with both notice names; other
  identities/versions/missing notices and counterfeit provenance are rejected.
- The normal Docker `verify` target passes: dependency deny checks, complete
  archive-notice collection, fmt, clippy, **10 CLI / 12 media / 28 telephony /
  6 registration integration tests**, doctests and rustdoc warnings-as-errors,
  and unchanged offline lock regeneration. This is the **existing application
  workspace**, not a green candidate replay test.
- `packaging/check-context.sh` passes in an explicit synthetic clone, including
  a deliberately permissive-filter negative control. The distinctive
  `CONTEXT_REJECTED` result identifies the synthetic reference/env sentinels and
  both `tests/probes/private.env` and `tests/probes/srtp_candidate.rs`. Thus the
  default context excludes the isolated research probe as well as private data.
  The safe clone also builds the scaffold export and verifies its checksums.
  No real local reference directory is read or modified.
- Separate sanitized read-only [Claude review, round 1](../reviews/claude-srtp-candidate-round1-2026-10-13.md)
  returned **APPROVE for the pre-adoption evaluation only**. Its documentary
  clarifications were applied; [round 2](../reviews/claude-srtp-candidate-round2-2026-10-13.md)
  returned **APPROVE for the final documentary clarifications only**. Neither
  review replaces the failed runtime
  requirement or approves the future adapter.

## Reproduce the isolated probe

Run only in a disposable checkout of this receipt's commit. The following
creates a new ignored copy and does not change the application workspace:

```sh
docker buildx build --platform linux/amd64 --target toolchain --load \
  --tag rust-microsip-toolchain:1.99.0 .
python3 - <<'PY'
import json
import hashlib
from pathlib import Path
import shutil

root = Path.cwd()
work = root / 'artifacts/srtp-candidate-reproduction'
work.mkdir(parents=True, mode=0o700, exist_ok=False)
names = json.loads((root / 'docs/receipts/g0-rust-transfer-2026-10-12.json').read_text())['application_build_input_names']
assert len(names) == 34
receipt = json.loads((root / 'docs/receipts/g0-srtp-candidate-2026-10-13.json').read_text())
assert set(names) == set(receipt['application_build_inputs_sha256'])
for name in names:
    path = Path(name)
    assert not path.is_absolute() and '..' not in path.parts and 'base' not in path.parts
    source = root / path
    assert source.is_file() and not source.is_symlink()
    assert hashlib.sha256(source.read_bytes()).hexdigest() == receipt['application_build_inputs_sha256'][name]
    target = work / path
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, target)
manifest = work / 'crates/media/Cargo.toml'
text = manifest.read_text()
assert text.count('[dev-dependencies]') == 1 and 'rustrtc' not in text
manifest.write_text(text.replace('[dev-dependencies]', '[dependencies]\nrustrtc = { version = "=0.4.7", default-features = false }\n\n[dev-dependencies]'))
shutil.copyfile(root / 'docs/inventory/g0-srtp-candidate.Cargo.lock', work / 'Cargo.lock')
shutil.copyfile(root / 'tests/probes/srtp_candidate.rs', work / 'crates/media/tests/srtp_candidate.rs')
for name in ('Cargo.lock', 'crates/media/Cargo.toml', 'crates/media/tests/srtp_candidate.rs'):
    assert hashlib.sha256((work / name).read_bytes()).hexdigest() == receipt['candidate_copied_inputs_sha256'][name]
PY
```

Use a dedicated ignored Cargo cache, never the personal Cargo/HOME directory.
Fetch the pinned lock in Docker with `cargo fetch --locked --target
x86_64-unknown-linux-gnu`; fetch/metadata do not compile dependencies. Before
compiling, run the existing deny checker against the actual target tree, collect
complete notices with `packaging/collect-cargo-notices.py`, compare package
checksums/features with this candidate inventory, and repeat the source/cache
and native/license review. A changed graph is not covered by this receipt.

In that isolated copy and toolchain, with the dedicated cache mounted and network
disabled, the diagnostic commands are:

```sh
cargo fmt --all -- --check
cargo clippy -p rust-microsip-media --test srtp_candidate --frozen -- -D warnings
cargo test -p rust-microsip-media --test srtp_candidate --frozen
```

The published preparation snippet and all three diagnostic commands were also
executed from a newly prepared copy, reproducing exactly 4/1/0 and exit 101; the
machine receipt records that separate implementer rerun and its log hash.

The final command is expected to fail **only** the duplicate-packet assertion
for this unadapted candidate. Do not run it as a root-workspace release gate or
treat its exit 101 as successful secure-media acceptance. No known-failing test
has been added to the normal application suite.

## Remaining work

Implement and verify the narrow replay/resource/secret boundary before candidate
adoption; then prove actual mandatory SDES/SRTP against the pinned Asterisk lab,
including downgrade and other negative cases. No secure packet adapter,
negotiation, secure call, or fallback behavior was added in this milestone.
OpenSpec remains **4/54**; tasks **1.7 and 5.4** and all G0/release gates stay open.
