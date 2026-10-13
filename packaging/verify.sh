#!/bin/sh
# Scaffold verification, always executed by Docker without network access.
set -eu
test "$(rustc --version | cut -d ' ' -f 2)" = '1.99.0'
test ! -e base
test ! -e .git
test ! -e .env
python3 -B -m unittest discover -s packaging/tests
# Use the build-selected target/features, not Cargo metadata's optional superset.
cargo tree --frozen --target x86_64-unknown-linux-gnu --prefix none --no-dedupe --format '{p}|{f}' -e normal,build > /tmp/g0-selected-tree.txt
python3 -B packaging/check-g0-dependencies.py < /tmp/g0-selected-tree.txt
cargo tree --frozen --target x86_64-unknown-linux-gnu --prefix none --no-dedupe --format '{p}|{f}' -e normal,build,dev > /tmp/g0-example-tree.txt
python3 -B packaging/check-g0-dependencies.py < /tmp/g0-example-tree.txt
python3 -B packaging/collect-cargo-notices.py /tmp/g0-cargo-notices /tmp/g0-example-tree.txt
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --frozen -- -D warnings
cargo test --workspace --frozen
cargo test --workspace --doc --frozen
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --frozen
# Confirm the lockfile matches actual offline Cargo generation.
cp Cargo.lock /tmp/scaffold-Cargo.lock
cargo generate-lockfile --offline
cmp Cargo.lock /tmp/scaffold-Cargo.lock
