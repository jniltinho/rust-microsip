#!/bin/sh
# DIST-03: build the exported artifacts from a fresh clone of a committed
# revision, never from the working tree, so base/, artifacts/, dist/ and
# uncommitted files cannot leak into the build or the result.
# Usage: sh packaging/release/clean-clone-build.sh [revision] [empty-dest] [reference-SHA256SUMS]
set -eu
repo=$(cd "$(dirname "$0")/../.." && pwd)
commit=$(git -C "$repo" rev-parse --verify "${1:-HEAD}^{commit}")
dest=${2:-$(mktemp -d)}
reference=${3:+$(realpath "$3")}
mkdir -p "$dest"
dest=$(cd "$dest" && pwd)
[ -z "$(ls -A "$dest")" ] || { echo "ERROR: destination is not empty: $dest" >&2; exit 1; }
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

git clone --quiet --no-local --no-checkout "$repo" "$work/src"
git -C "$work/src" checkout --quiet --detach "$commit"
for local_only in base artifacts dist target .env; do
    [ ! -e "$work/src/$local_only" ] || { echo "ERROR: clone contains $local_only" >&2; exit 1; }
done
# The build must not depend on a host Rust toolchain: hide rustup/cargo dirs.
PATH=$(printf '%s' "$PATH" | tr ':' '\n' | grep -v -e '/\.cargo/' -e '/\.rustup/' | paste -sd: -)
export PATH
if command -v cargo > /dev/null 2>&1 || command -v rustc > /dev/null 2>&1; then
    echo "ERROR: a host Rust toolchain is still on PATH" >&2
    exit 1
fi

# NO_CACHE=1 rebuilds every stage, as on a fresh Docker machine.
docker buildx build ${NO_CACHE:+--no-cache} --platform linux/amd64 --target artifacts \
    --output "type=local,dest=$dest" "$work/src"

cd "$dest"
sha256sum --check --strict --quiet SHA256SUMS
for required in rust-microsip SHA256SUMS LICENSE NOTICE build-manifest.json \
    scaffold-sbom.cdx.json cargo-licenses/inventory.json toolchain-licenses; do
    [ -e "$required" ] || { echo "ERROR: missing $required" >&2; exit 1; }
done
ls ./*.AppImage > /dev/null
# Every exported file must be covered by SHA256SUMS (origin identifiable).
find . -type f ! -name SHA256SUMS | sort > "$work/exported"
awk '{print $2}' SHA256SUMS | sort > "$work/listed"
cmp -s "$work/exported" "$work/listed" || {
    echo "ERROR: exported files and SHA256SUMS differ:" >&2
    diff "$work/listed" "$work/exported" >&2 || true
    exit 1
}

echo "commit: $commit"
echo "dest: $dest"
grep -vE '\./(cargo|toolchain)-licenses/' SHA256SUMS
if [ -n "$reference" ]; then
    if diff "$reference" SHA256SUMS > "$work/reference.diff"; then
        echo "reference: identical SHA256SUMS"
    else
        echo "reference: differences (- reference, + clean clone):"
        grep -E '^[<>]' "$work/reference.diff" | sed 's/^</-/; s/^>/+/'
    fi
fi
