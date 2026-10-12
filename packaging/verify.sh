#!/bin/sh
# Scaffold verification, always executed by Docker without network access.
set -eu
test "$(rustc --version | cut -d ' ' -f 2)" = '1.99.0'
test ! -e base
test ! -e .git
test ! -e .env
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --frozen -- -D warnings
cargo test --workspace --frozen
cargo test --workspace --doc --frozen
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --frozen
# Confirm the minimal lockfile matches actual Cargo generation.
cp Cargo.lock /tmp/scaffold-Cargo.lock
cargo generate-lockfile --offline
cmp Cargo.lock /tmp/scaffold-Cargo.lock
