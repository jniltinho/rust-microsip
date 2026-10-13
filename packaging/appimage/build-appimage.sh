#!/bin/sh
# Wrap the exported ELF (the primary artifact) in a convenience AppImage.
# Runs offline after the pinned runtime was downloaded and checked. Bundles no
# libraries: the host provides glibc and libasound.so.2, exactly as for the ELF.
set -eu
here=packaging/appimage
runtime=/opt/appimage/runtime-x86_64
grep -v '^#' "$here/pins.txt" | while read -r sum name _url; do
    case $name in
        runtime-x86_64) file=$runtime ;;
        *) file=$here/$name ;;
    esac
    printf '%s  %s\n' "$sum" "$file" | sha256sum --check --strict --quiet -
done

version=$(/out/rust-microsip --version | awk '{print $2}')
umask 022
temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT
appdir=$temporary/AppDir
doc=$appdir/usr/share/doc/rust-microsip
icons=$appdir/usr/share/icons/hicolor
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" "$icons/256x256/apps" "$icons/scalable/apps" "$doc"

cp /out/rust-microsip "$appdir/usr/bin/rust-microsip"
ln -s usr/bin/rust-microsip "$appdir/AppRun"
desktop-file-validate "$here/rust-microsip.desktop"
cp "$here/rust-microsip.desktop" "$appdir/rust-microsip.desktop"
cp "$here/rust-microsip.desktop" "$appdir/usr/share/applications/rust-microsip.desktop"
svg=apps/rust-microsip/assets/icons/rust-microsip.svg
cp "$svg" "$icons/scalable/apps/rust-microsip.svg"
rsvg-convert --width 256 --height 256 "$svg" --output "$icons/256x256/apps/rust-microsip.png"
cp "$icons/256x256/apps/rust-microsip.png" "$appdir/rust-microsip.png"
ln -s rust-microsip.png "$appdir/.DirIcon"
cp -R /out/LICENSE /out/NOTICE /out/cargo-licenses /out/toolchain-licenses "$doc/"
cp apps/rust-microsip/assets/OFL.txt "$doc/NotoSans-OFL.txt"
cp -R "$here/runtime-licenses" "$doc/appimage-runtime-licenses"

# Fixed owner, timestamps and no xattrs keep the image independent of the build host.
mksquashfs "$appdir" "$temporary/image.squashfs" -noappend -all-root -no-xattrs \
    -comp zstd -mkfs-time 0 -all-time 0 -quiet -no-progress
image=/out/rust-microsip-$version-x86_64.AppImage
cat "$runtime" "$temporary/image.squashfs" > "$image"
chmod 0755 "$image"

(cd /out && rm -f SHA256SUMS && find . -type f ! -name SHA256SUMS -print0 | LC_ALL=C sort -z | xargs -0 sha256sum > SHA256SUMS)
