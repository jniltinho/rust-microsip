#!/bin/sh
# Check an AppImage against its ELF without FUSE.
# Usage: sh packaging/appimage/verify.sh <AppImage> <ELF>
set -eu
pins=$(cd "$(dirname "$0")" && pwd)/pins.txt
image=$(realpath "$1")
elf=$(realpath "$2")
fail() { echo "FAIL: $*" >&2; exit 1; }
sha() { sha256sum "$1" | cut -d' ' -f1; }
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"

offset=$("$image" --appimage-offset)
pinned=$(awk '$2 == "runtime-x86_64" { print $1 }' "$pins")
[ "$(head -c "$offset" "$image" | sha256sum | cut -d' ' -f1)" = "$pinned" ] || fail "runtime prefix is not the pinned runtime"
"$image" --appimage-extract > /dev/null
root=$work/squashfs-root

[ "$(sha "$root/usr/bin/rust-microsip")" = "$(sha "$elf")" ] || fail "bundled ELF differs from exported ELF"
[ "$(readlink "$root/AppRun")" = usr/bin/rust-microsip ] || fail "AppRun does not point to the ELF"

desktop=$root/rust-microsip.desktop
for field in Type=Application Name=rust-microsip Exec=rust-microsip Icon=rust-microsip \
    'Categories=Network;Telephony;' StartupWMClass=rust-microsip Terminal=false; do
    grep -qxF "$field" "$desktop" || fail "desktop file lacks $field"
done
cmp -s "$desktop" "$root/usr/share/applications/rust-microsip.desktop" || fail "desktop copies differ"
# PNG signature, then IHDR width and height (256 x 256).
[ "$(od -An -tx1 -N8 "$root/rust-microsip.png" | tr -d ' \n')" = 89504e470d0a1a0a ] || fail "icon is not PNG"
[ "$(od -An -tx1 -j16 -N8 "$root/rust-microsip.png" | tr -d ' \n')" = 0000010000000100 ] || fail "icon is not 256x256"
[ -s "$root/usr/share/icons/hicolor/scalable/apps/rust-microsip.svg" ] || fail "SVG icon missing"

[ -z "$(find "$root" \( -name '*.so' -o -name '*.so.*' \) -print)" ] || fail "bundled shared libraries found"
elves=$(find "$root" -type f -exec sh -c '[ "$(od -An -tx1 -N4 "$1" | tr -d " \n")" = 7f454c46 ] && echo "$1"' _ {} \;)
[ "$elves" = "$root/usr/bin/rust-microsip" ] || fail "unexpected ELF files: $elves"

doc=$root/usr/share/doc/rust-microsip
for file in LICENSE NOTICE NotoSans-OFL.txt cargo-licenses/inventory.json \
    toolchain-licenses/generated-notices/COPYRIGHT-library.html \
    appimage-runtime-licenses/type2-runtime-LICENSE appimage-runtime-licenses/libfuse-3.15.0-LGPL2.txt; do
    [ -s "$doc/$file" ] || fail "missing license file $file"
done
echo "OK: $(basename "$image") offset=$offset elf_sha256=$(sha "$elf")"
