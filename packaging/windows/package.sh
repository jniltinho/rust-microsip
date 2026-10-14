#!/bin/sh
# Windows packages from the release executables: THIRD-PARTY-NOTICES.txt, the per-user MSI, the
# portable zip and SHA256SUMS (add-windows-port tasks 4.1-4.3). Runs offline in the
# windows-toolchain image, from the repo root:
#   packaging/windows/package.sh RELEASE_DIR OUT_DIR VERSION
set -eu
release=$1 out=$2 version=$3
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cargo tree --locked --offline --target x86_64-pc-windows-gnu -p rust-microsip --prefix none --no-dedupe \
    --format '{p}|{f}' -e normal,build > "$work/tree.txt"
RMS_NOTICE_TARGET=x86_64-pc-windows-gnu python3 -B packaging/collect-cargo-notices.py "$work/cargo" "$work/tree.txt"
python3 -B packaging/windows/notices.py "$work/cargo" "$work/THIRD-PARTY-NOTICES.txt"
sh packaging/windows/msi.sh "$release" "$out" "$version" "$work/THIRD-PARTY-NOTICES.txt"
python3 -B packaging/windows/zip.py "$release" "$out" "$version" "$work/THIRD-PARTY-NOTICES.txt"
