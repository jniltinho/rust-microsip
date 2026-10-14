#!/bin/sh
# Per-user MSI from the two Windows executables (add-windows-port W10, task 4.2).
# Runs in the windows-toolchain image (wixl, msibuild, msiinfo, msiextract), from the repo root:
#   packaging/windows/msi.sh RELEASE_DIR OUT_DIR VERSION THIRD-PARTY-NOTICES.txt
# The same version always gets the same ProductCode/PackageCode; a newer version replaces an
# older one (MajorUpgrade). Lists the payload with msiextract and fails on any difference.
set -eu
release=$1 out=$2 version=$3 notices=$4
here=packaging/windows
case $version in ''|*[!0-9A-Za-z.+-]*) echo "invalid version: $version" >&2; exit 1 ;; esac
msi=rust-microsip-$version-x86_64.msi
mkdir -p "$out"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cp "$release/rust-microsip.exe" "$release/rust-microsip-cli.exe" "$work/"
cp LICENSE "$work/LICENSE.txt"
cp NOTICE "$work/NOTICE.txt"
cp "$notices" "$work/THIRD-PARTY-NOTICES.txt"
chmod 0644 "$work"/*
touch -d "@${SOURCE_DATE_EPOCH:-1767225600}" "$work"/*

# wixl writes the build time and a random PackageCode into the summary information: a fixed
# clock (SOURCE_DATE_EPOCH, 2026-01-01 by default) and the UUIDv5 below make rebuilds identical.
stamp=$(date -u -d "@${SOURCE_DATE_EPOCH:-1767225600}" '+%Y-%m-%d %H:%M:%S')
# ProductVersion is numeric: a pre-release suffix is dropped.
uuid() { python3 -c 'import sys, uuid; print(str(uuid.uuid5(uuid.UUID("69C0A8E9-742D-43EA-A651-3ACBB281AA11"), sys.argv[1])).upper())' "$1"; }
faketime -f "@$stamp" wixl --arch x64 -D "Version=${version%%[-+]*}" -D "ProductCode=$(uuid "product:$version")" \
    -D "PackageCode=$(uuid "package:$version")" -D "SourceDir=$work" -o "$work/$msi" "$here/rust-microsip.wxs"
msibuild "$work/$msi" -s "rust-microsip SIP softphone" rust-microsip "x64;1033" "{$(uuid "package:$version")}"
# Start Menu shortcut AppUserModelID (toasts), a table wixl does not author.
cp "$here/MsiShortcutProperty.idt" "$work/"
msibuild "$work/$msi" -i "$work/MsiShortcutProperty.idt"
# Word Count 10: compressed (2) and "elevated privileges are not required" (8); no ALLUSERS.
msiinfo suminfo "$work/$msi" | grep -qx 'Source: 10 (a)' || { echo "MSI is not a per-user package" >&2; exit 1; }
if msiinfo export "$work/$msi" Property | grep -q '^ALLUSERS'; then echo "MSI sets ALLUSERS" >&2; exit 1; fi
msiinfo export "$work/$msi" MsiShortcutProperty | grep -q 'StartMenuShortcut.*rust-microsip' \
    || { echo "MSI lacks the AppUserModelID row" >&2; exit 1; }

# Payload check: exactly the five files, byte-identical to the inputs.
mkdir "$work/x"
(cd "$work/x" && msiextract "../$msi" > /dev/null)
found=$(cd "$work/x" && find . -type f | sed 's|.*/||' | sort | tr '\n' ' ')
[ "$found" = "LICENSE.txt NOTICE.txt THIRD-PARTY-NOTICES.txt rust-microsip-cli.exe rust-microsip.exe " ] \
    || { echo "unexpected MSI payload: $found" >&2; exit 1; }
for f in rust-microsip.exe rust-microsip-cli.exe LICENSE.txt NOTICE.txt THIRD-PARTY-NOTICES.txt; do
    cmp "$work/$f" "$(find "$work/x" -type f -name "$f")"
done
cp "$work/$msi" "$out/$msi"
echo "$out/$msi"
