# Reproducing the isolated initial secure-call experiment

This is **not the application or a release build**. Read
[ADR 0011](adr/0011-isolated-initial-tls-sdes-calls.md) and the
[receipt](receipts/g0-secure-call-2026-10-13.md) first. It requires Linux/amd64,
Docker/Compose, the pinned project toolchain/PBX images, Python 3, and the
previously reviewed 100-package dedicated Cargo cache. Missing dependencies fail
offline; these commands do not fetch new packages or approve a changed graph.
Prepare only fictional disposable fixtures using the [lab guide](../docker/asterisk/README.md).
No existing `rust-microsip-lab` resources may be running. Never supply personal
accounts, production credentials, audio devices or local reference directories.

Run from the repository root. Preparation refuses an existing output directory,
symlinks, changed application/probe pins or unexpected input paths. It copies
34 explicit application inputs and four probe inputs, not the repository tree.
The experiment's lock is separate from the application lock.

```sh
python3 tests/probes/prepare_secure_call.py
candidate="$PWD/artifacts/secure-call-work"
cache="$PWD/artifacts/dependency-research/cargo-home"

cargo_in_candidate() {
    docker run --rm --network none --user "$(id -u):$(id -g)" --read-only \
        --cap-drop ALL --security-opt no-new-privileges \
        --memory 3g --cpus 2 --pids-limit 256 \
        --tmpfs /tmp:rw,exec,nosuid,nodev,mode=1777,size=256m \
        --mount "type=bind,source=$candidate,target=/work" \
        --mount "type=bind,source=$cache,target=/cargo" \
        -e CARGO_HOME=/cargo rust-microsip-toolchain:1.99.0 "$@"
}

cargo_in_candidate sh -c '
    cargo metadata --frozen --format-version 1 \
        --filter-platform x86_64-unknown-linux-gnu > secure-metadata.json &&
    cargo tree --frozen --target x86_64-unknown-linux-gnu \
        --edges normal,build,dev --prefix none --format "{p}|{f}" > secure-tree.txt &&
    python3 packaging/collect-cargo-notices.py secure-notices-exact
'
python3 tests/probes/verify_secure_dependencies.py

cargo_in_candidate sh -c '
    cargo fmt --all --check && cargo test --workspace --frozen &&
    cargo clippy --workspace --all-targets --frozen -- -D warnings &&
    cargo build -p rust-microsip-telephony --example secure_call \
        --release --frozen --target x86_64-unknown-linux-gnu
'
python3 tests/probes/verify_secure_dependencies.py
python3 tests/probes/observe_secure_call.py
```

The observer snapshots actual images and ELF, runs only the disposable PBX and
an explicitly mounted probe binary, verifies internal networking/hardening,
validates the four-case matrix plus three real 488 and two typed certificate
negatives, and checks cleanup. Logs, keys, fixtures, executable and caches stay
under ignored `artifacts/`. The runner uses the existing toolchain image rather
than a production runtime image. The Rust notice inventory is not an inventory
of every OS/toolchain package in that container.

For a fresh explicit-copy regression, pass a neutral directory name to both
preparation and dependency verification, and change `candidate` accordingly.
The runtime observer intentionally uses `secure-call-work`; it is not a general
remote-account CLI. Do not point Docker at a repository-wide build context or
mount a home directory. A successful run does not complete G0, SRTCP, secure call
controls, audio-device/human tests, production hardening or release acceptance.
