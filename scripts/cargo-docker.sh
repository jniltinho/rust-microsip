#!/bin/sh
# Run cargo (or any command) in the pinned toolchain image (Dockerfile target
# `toolchain`); no host Rust is used. The checkout is mounted at /work and the
# container runs as your UID with HOME and CARGO_HOME under target/, so nothing
# is written outside the repository. `base/` (local-only references) is masked.
#
#   scripts/cargo-docker.sh cargo test -p rust-microsip-core
#   scripts/cargo-docker.sh bash          # interactive shell
#   RMS_DOCKER_NET=none scripts/cargo-docker.sh cargo test --frozen --workspace
#   RMS_TOOLCHAIN_STAGE=windows-toolchain scripts/cargo-docker.sh \
#       cargo build --target x86_64-pc-windows-gnu --locked   # Windows cross build
#
# Without arguments it runs `cargo build --workspace --locked`.
set -eu
cd "$(dirname "$0")/.."
stage=${RMS_TOOLCHAIN_STAGE:-toolchain}
image=${RMS_TOOLCHAIN_IMAGE:-rust-microsip-$stage:dev}
if ! docker image inspect "$image" > /dev/null 2>&1; then
    docker buildx build --platform linux/amd64 --target "$stage" --load -t "$image" . >&2
fi
mkdir -p target/home target/cargo-home
target_dir=${CARGO_TARGET_DIR:-target/docker}
tty=
[ -t 0 ] && [ -t 1 ] && tty=-it
[ $# -gt 0 ] || set -- cargo build --workspace --locked
exec docker run --rm $tty --network "${RMS_DOCKER_NET:-bridge}" \
    --user "$(id -u):$(id -g)" --cap-drop ALL --security-opt no-new-privileges \
    --mount "type=bind,source=$PWD,target=/work" \
    --mount "type=tmpfs,target=/work/base" \
    -e HOME=/work/target/home -e CARGO_HOME=/work/target/cargo-home \
    -e CARGO_TARGET_DIR="/work/$target_dir" -e CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}" \
    -e CARGO_TERM_COLOR=never -e RUST_LOG -e RUST_BACKTRACE \
    -w /work "$image" "$@"
