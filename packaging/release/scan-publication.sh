#!/bin/sh
# DIST-07 pre-push/pre-release audit. Scans what a push publishes: the Git
# index (tracked + staged content), every blob and path reachable from any
# ref in history and, optionally, artifact directories. Content is matched
# byte-wise (binary files included) and again with NUL bytes removed, so
# UTF-16 text is covered too. Findings print only a path, a category and a
# match count, never the match.
#
# Usage: sh packaging/release/scan-publication.sh [artifact-dir ...]
# Env:   LAB_DIR        generated Asterisk lab directory whose fictional
#                       passwords must never be published (default:
#                       <repo>/artifacts/asterisk-lab; skipped when absent).
#        MAX_BLOB_BYTES Git blobs larger than this are skipped with a
#                       warning (default: 64 MiB).
set -eu
repo=$(git rev-parse --show-toplevel)
lab=${LAB_DIR:-$repo/artifacts/asterisk-lab}
max_blob=${MAX_BLOB_BYTES:-67108864}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
failures=0
tab=$(printf '\t')
fail() { echo "FAIL [$1] $2"; failures=$((failures + 1)); }

# Forbidden names: local references, env files, databases, AppImages, keys,
# captures, credential stores, generated lab credentials and build output.
forbidden_path='(^|/)(base|artifacts|dist|target)(/|$)|(^|/)\.env($|\.)|\.(sqlite3?|db|db-wal|db-shm|sqlite3?-(wal|shm|journal))$|\.AppImage$|\.(pem|key|p12|pfx|jks|pcapng?|csr|kdbx|ppk)$|(^|/)(credentials\.json|id_(rsa|ecdsa|ed25519)|\.netrc|\.pgpass|\.git-credentials|\.pypirc)$'
allowed_path='(^|/)\.env\.example$'
# Repository files whose basename mentions a password or secret (checked
# case-insensitively) are app secret stores unless they are known sources.
secret_name='(^|/)[^/]*(password|secret)[^/]*$'
secret_name_allowed='^crates/platform/src/secrets\.rs$'
# Content patterns are written so this file never matches itself; it is
# scanned like any other file.
# Unsplit PEM header (the text BEGIN ... PRIVATE KEY between five dashes) or
# a PuTTY private key file header.
key_pattern='-{5}BEGIN ([A-Z0-9]+ )*PRIVATE KEY-{5}|PuTTY-User-Key-File-[0-9]+:'
# The npm token alternative skips ${VAR} references.
token_pattern='gh[pousr]_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{60,}|AKIA[0-9A-Z]{16}|xox[abprs]-[A-Za-z0-9-]{10,}|sk-ant-[A-Za-z0-9_-]{20,}|glpat-[A-Za-z0-9_-]{20}|AIza[0-9A-Za-z_-]{35}|sk_live_[0-9A-Za-z]{16,}|_auth[T]oken[[:space:]]*=[[:space:]]*[^[:space:]$]'
# A literal password in a TOML file (the app config format).
# Basic and literal strings, dotted keys and inline tables (`a.password = 'x'`,
# `account = { password = "x" }`).
toml_password='(^|[[:space:]{,.])password[[:space:]]*=[[:space:]]*("[^"]+"|'"'"'[^'"'"']+'"'"')'
# Documented examples are exempt only when they live under docs/ or carry
# "example" in their name AND contain this explicit marker line.
toml_example_marker='publication-scan: fictional-example'
# A path start (also after file://, a flag such as -L or -I, '=' or ':'),
# not a URL segment such as crates.io/crates/home/<version>.
home_pattern='(^|[^A-Za-z0-9._/-]|:/{2}|(^|[^A-Za-z0-9])-[A-Za-z]{1,2})/(home|Users)/[A-Za-z0-9._-]+'
# Container/CI accounts and documentation placeholders, not personal homes.
home_allowed='/(home|Users)/(builder|runner|user|username|example|USER)$'
# Owner decision: two early commits published an absolute personal home path
# in README.md and docs/reference-sources.md. The repository is public and the
# owner chose not to rewrite history. Only these exact historical blobs are
# exempt from the personal-home check, and only while they are absent from
# the index, so the current tree must stay clean.
history_home_allowed='a618a98cecb802635c863a83e804e47ff55737eb ddad2829bbbc868b5fa0293fa51c5f51a501ffe1 373601a9aaad5eb778b993217f303b8fb61dc672'

# Fictional lab passwords: fixed strings in a private temporary file only.
: > "$work/lab-values"
chmod 0600 "$work/lab-values"
if [ -d "$lab" ]; then
    find "$lab" -name credentials.json -type f -exec python3 -I -c '
import json, sys
for name in sys.argv[1:]:
    for value in json.load(open(name)).values():
        if isinstance(value, str) and len(value) >= 6:
            print(value)' {} + | sort -u > "$work/lab-values"
    echo "lab secrets: $(wc -l < "$work/lab-values") fictional value(s) loaded (not printed)"
else
    echo "lab secrets: no lab directory, value check skipped"
fi

home_count() { grep -a -o -E -e "$home_pattern" | grep -a -v -c -E "$home_allowed" || true; }

# Copies every file of $1 that contains a NUL byte to $2 (same relative path)
# with the NUL bytes removed, so UTF-16 encoded ASCII becomes matchable.
strip_nul() {
    python3 -I -c '
import os, sys
src, dst = sys.argv[1:]
for root, _, files in os.walk(src):
    for name in files:
        path = os.path.join(root, name)
        if os.path.islink(path):
            continue
        with open(path, "rb") as handle:
            data = handle.read()
        if b"\0" in data:
            out = os.path.join(dst, os.path.relpath(path, src))
            os.makedirs(os.path.dirname(out), exist_ok=True)
            with open(out, "wb") as handle:
                handle.write(data.replace(b"\0", b""))' "$1" "$2"
}

# Prints "category<TAB>relative-path<TAB>count" for every content finding in
# directory $1 and in its NUL-stripped copy (one line per category and file).
content_hits() {
    rm -rf "$work/stripped"
    mkdir "$work/stripped"
    strip_nul "$1" "$work/stripped"
    for dir in "$1" "$work/stripped"; do
        grep_dir private-key "$dir" -E -e "$key_pattern"
        grep_dir token "$dir" -E -e "$token_pattern"
        [ ! -s "$work/lab-values" ] || grep_dir lab-secret "$dir" -F -f "$work/lab-values"
        grep -r -a -l -E -e "$home_pattern" "$dir" | while IFS= read -r path; do
            count=$(home_count < "$path")
            [ "$count" -eq 0 ] || printf 'personal-home\t%s\t%s\n' "${path#"$dir"/}" "$count"
        done || true
    done | awk -F '\t' '!seen[$1 FS $2]++'
}
grep_dir() { # category, directory, grep arguments...
    category=$1; dir=$2; shift 2
    grep -r -a -c "$@" "$dir" | grep -v ':0$' | while IFS= read -r hit; do
        rel=${hit#"$dir"/}
        printf '%s\t%s\t%s\n' "$category" "${rel%:*}" "${hit##*:}"
    done || true
}

# --- Git repository: index and full history ---
cd "$repo"
if [ "$(git rev-parse --is-shallow-repository)" = true ]; then
    echo "WARN shallow clone: history is only partially scanned (use fetch-depth: 0)"
fi
# Blob inventory "id<TAB>label": index entries first, then history-only blobs.
git ls-files -s -z | tr '\0' '\n' | awk -F '\t' '{
    split($1, meta, " "); if (meta[1] != "160000") print meta[2] "\tindex:" $2 }' > "$work/inventory"
cut -f1 "$work/inventory" | sort -u > "$work/index-ids"
git rev-list --objects --all |
    git cat-file --batch-check='%(objecttype) %(objectname) %(rest)' |
    awk '$1 == "blob" { id = $2; sub(/^[^ ]+ [^ ]+ ?/, ""); print id "\thistory:" $0 }' |
    awk -F '\t' 'NR == FNR { indexed[$1] = 1; next } !($1 in indexed) && !seen[$1]++' \
        "$work/index-ids" - >> "$work/inventory"
cut -f1 "$work/inventory" | sort -u |
    git cat-file --batch-check='%(objectname) %(objectsize)' > "$work/sizes"
awk -v max="$max_blob" '$2 > max { print $1 }' "$work/sizes" > "$work/huge"
while IFS= read -r id; do
    echo "WARN [blob-too-large] $(grep -m1 "^$id" "$work/inventory" | cut -f2) skipped (> $max_blob bytes)"
done < "$work/huge"
mkdir "$work/blobs"
awk -v max="$max_blob" '$2 <= max { print $1 }' "$work/sizes" | git cat-file --batch |
    python3 -I -c '
import os, sys
out, stream = sys.argv[1], sys.stdin.buffer
while True:
    header = stream.readline()
    if not header:
        break
    oid, kind, size = header.split()
    with open(os.path.join(out, oid.decode()), "wb") as handle:
        handle.write(stream.read(int(size)))
    stream.read(1)' "$work/blobs"

# Paths: every index entry plus every path added or changed by any commit.
cut -f2- "$work/inventory" | sed -n 's/^index://p' > "$work/index-paths"
git log --all -m --no-renames --format= --name-only -z | tr '\0' '\n' | sed '/^$/d' | sort -u |
    awk 'NR == FNR { indexed[$0] = 1; next } !($0 in indexed)' "$work/index-paths" - > "$work/history-paths"
for scope in index history; do
    { grep -E "$forbidden_path" "$work/$scope-paths" | grep -v -E "$allowed_path" || true; } > "$work/bad"
    while IFS= read -r path; do fail forbidden-path "$scope:$path"; done < "$work/bad"
    { grep -i -E "$secret_name" "$work/$scope-paths" | grep -v -E "$secret_name_allowed" || true; } > "$work/bad"
    while IFS= read -r path; do fail secret-file-name "$scope:$path"; done < "$work/bad"
done
git ls-files -s | awk '$1 == "160000" { print $4 }' > "$work/submodules"
while IFS= read -r path; do fail submodule "$path"; done < "$work/submodules"
[ ! -e .gitmodules ] || ! grep -q -E '(^|/)base' .gitmodules || fail submodule .gitmodules
git ls-files -s | awk '$1 == "120000" { print $2, $4 }' | while read -r blob path; do
    if git cat-file blob "$blob" | grep -q -E "(^|/)base(/|$)|$home_pattern"; then
        echo "FAIL [symlink-target] $path"
    fi
done > "$work/links"
[ ! -s "$work/links" ] || { cat "$work/links"; failures=$((failures + $(wc -l < "$work/links"))); }

# Content of every scanned blob, reported under each of its labels.
content_hits "$work/blobs" > "$work/hits"
allowed_history=0
while IFS=$tab read -r category id count; do
    if [ "$category" = personal-home ] && ! grep -q -x "$id" "$work/index-ids"; then
        case " $history_home_allowed " in *" $id "*) allowed_history=$((allowed_history + 1)); continue ;; esac
    fi
    grep "^$id$tab" "$work/inventory" | cut -f2- > "$work/labels"
    while IFS= read -r label; do fail "$category" "$label:$count"; done < "$work/labels"
done < "$work/hits"
[ "$allowed_history" -eq 0 ] ||
    echo "personal-home: $allowed_history owner-approved historical blob(s) allowlisted"
# Literal passwords in TOML files, per path.
grep -E "$tab(index|history):.*\.toml$" "$work/inventory" > "$work/toml" || true
while IFS=$tab read -r id label; do
    blob=$work/blobs/$id
    [ -f "$blob" ] || continue
    count=$(grep -a -c -E -e "$toml_password" "$blob" || true)
    [ "$count" -ne 0 ] || continue
    if printf '%s\n' "$label" | grep -q -E '^(index|history):(docs/.*|.*example[^/]*)$' &&
        grep -q -F -e "$toml_example_marker" "$blob"; then
        continue
    fi
    fail app-password "$label:$count"
done < "$work/toml"
echo "repository: $(wc -l < "$work/index-paths") index path(s), $(wc -l < "$work/history-paths") history-only path(s), $(ls "$work/blobs" | wc -l) unique blob(s) scanned"

# --- Artifact directories (AppImages are unpacked without executing them) ---
scan_tree() { # label, directory
    find "$2" -type f | sed "s|^$2/||" > "$work/files"
    grep -E "$forbidden_path" "$work/files" | grep -v -E "$allowed_path|\.AppImage$" > "$work/bad" || true
    while IFS= read -r path; do fail forbidden-path "$1/$path"; done < "$work/bad"
    # Shipped TOML must never carry a literal password; names hint at secrets.
    grep -E '\.toml$' "$work/files" | while IFS= read -r path; do
        if grep -a -q -E -e "$toml_password" "$2/$path"; then echo "$path"; fi
    done > "$work/bad"
    while IFS= read -r path; do fail app-password "$1/$path"; done < "$work/bad"
    grep -i -E "$secret_name" "$work/files" > "$work/bad" || true
    while IFS= read -r path; do fail secret-file-name "$1/$path"; done < "$work/bad"
    content_hits "$2" > "$work/hits"
    while IFS=$tab read -r category path count; do
        fail "$category" "$1:$path:$count"
    done < "$work/hits"
    echo "$1: $(wc -l < "$work/files") file(s) scanned"
}
for label in "$@"; do
    artifacts=$(cd "$label" && pwd)
    scan_tree "$label" "$artifacts"
    find "$artifacts" -type f -name '*.AppImage' | while IFS= read -r image; do
        # The squashfs payload starts right after the runtime ELF's section headers.
        offset=$(python3 -I -c '
import struct, sys
header = open(sys.argv[1], "rb").read(64)
assert header[:4] == b"\x7fELF" and header[4] == 2, "not a 64-bit ELF runtime"
shoff, = struct.unpack_from("<Q", header, 0x28)
size, count = struct.unpack_from("<HH", header, 0x3A)
print(shoff + size * count)' "$image")
        rm -rf "$work/appimage"
        unsquashfs -q -n -o "$offset" -d "$work/appimage" "$image" > /dev/null
        scan_tree "$label/${image#"$artifacts"/}" "$work/appimage"
    done > "$work/appimage.log"
    cat "$work/appimage.log"
    failures=$((failures + $(grep -c '^FAIL' "$work/appimage.log" || true)))
done

if [ "$failures" -ne 0 ]; then
    echo "PUBLICATION SCAN FAILED: $failures finding(s)"
    exit 1
fi
echo "PUBLICATION SCAN PASSED"
