#!/bin/sh
# Test the allowlist in a synthetic clone; never read or change the real base/.
set -eu
cd "$(dirname "$0")/.."
root=$(pwd)
temporary=$(mktemp -d)
scratch="$temporary/context"
mkdir "$scratch"
trap 'rm -rf "$temporary"' EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
for path in Dockerfile .dockerignore Cargo.toml Cargo.lock rust-toolchain.toml LICENSE NOTICE \
    apps/rust-microsip/Cargo.toml apps/rust-microsip/src/main.rs apps/rust-microsip/src/lib.rs \
    apps/rust-microsip/tests/cli.rs docker/rust-components.sha256 \
    crates/telephony/Cargo.toml crates/telephony/src/lib.rs crates/telephony/tests/registration.rs \
    crates/telephony/examples/g0_registration.rs crates/telephony/examples/g0_calls.rs \
    crates/telephony/src/g0_calls.rs crates/telephony/src/g0_controls.rs crates/telephony/examples/g0_controls.rs \
    crates/telephony/src/g0_hold.rs crates/telephony/examples/g0_hold.rs \
    crates/telephony/src/g0_transfer.rs crates/telephony/examples/g0_transfer.rs \
    crates/media/src/dtmf.rs crates/media/Cargo.toml crates/media/src/lib.rs crates/media/tests/media.rs \
    packaging/verify.sh packaging/export-scaffold.sh \
    packaging/check-g0-dependencies.py packaging/tests/test_g0_dependencies.py \
    packaging/collect-cargo-notices.py packaging/tests/test_cargo_notices.py \
    packaging/reviewed-licenses.json \
    apps/rust-microsip/assets/NotoSans-Regular.ttf apps/rust-microsip/assets/OFL.txt \
    apps/rust-microsip/assets/source.json apps/rust-microsip/assets/icons/rust-microsip.svg \
    packaging/appimage/build-appimage.sh packaging/appimage/verify.sh \
    packaging/appimage/pins.txt packaging/appimage/rust-microsip.desktop \
    crates/core/Cargo.toml crates/audio/Cargo.toml crates/config/Cargo.toml crates/storage/Cargo.toml crates/platform/Cargo.toml \
    $(find apps/rust-microsip/src/ui crates/telephony/src/engine crates/core/src crates/core/tests crates/audio/src \
        crates/config/src crates/storage/src crates/platform/src crates/telephony/examples crates/media/src -maxdepth 1 -type f -name '*.rs' | sort) \
    $(find packaging/third-party-notices packaging/appimage/runtime-licenses -type f | sort); do
    if [ -L "$path" ] || [ ! -f "$path" ]; then
        echo "ERROR: missing or symlinked test source: $path" >&2
        exit 1
    fi
    mkdir -p "$scratch/$(dirname "$path")"
    cp "$path" "$scratch/$path"
done
mkdir -p "$scratch/base" "$scratch/.git" "$scratch/dist" "$scratch/artifacts" "$scratch/tests/probes"
cp tests/probes/srtp_candidate.rs "$scratch/tests/probes/srtp_candidate.rs"
cp tests/probes/srtp_adapter.rs "$scratch/tests/probes/srtp_adapter.rs"
for name in secure_call_inputs.json rtcp.rs sdes.rs secure_controls.rs secure_call.rs secure_call_example.rs prepare_secure_call.py observe_secure_call.py verify_secure_dependencies.py; do
    cp "tests/probes/$name" "$scratch/tests/probes/$name"
done
for path in base/private.txt .env .env.local profile.sqlite3 sample.AppImage .git/private.txt dist/private.txt artifacts/private.txt apps/rust-microsip/src/private.env crates/telephony/src/private.env crates/telephony/examples/private.env tests/probes/private.env; do
    printf '%s\n' 'synthetic-private-context-marker' > "$scratch/$path"
done
docker buildx build --platform linux/amd64 --target context-audit "$scratch"
# Negative control: the audit MUST detect a permissive filter, not accept it.
cp "$scratch/.dockerignore" "$temporary/dockerignore.safe"
printf '%s\n' '' > "$scratch/.dockerignore"
if docker buildx build --progress plain --platform linux/amd64 --target context-audit "$scratch" > "$temporary/negative.log" 2>&1; then
    echo 'ERROR: context containing synthetic private data was not rejected.' >&2
    exit 1
fi
if ! grep -Eq '^#[0-9]+ [0-9.]+ CONTEXT_REJECTED:' "$temporary/negative.log"; then
    echo 'ERROR: negative control failed for a reason other than the privacy audit.' >&2
    cat "$temporary/negative.log" >&2
    exit 1
fi
grep -E '^#[0-9]+ [0-9.]+ CONTEXT_REJECTED:' "$temporary/negative.log"
for sentinel in base/private.txt .env tests/probes/private.env tests/probes/srtp_candidate.rs tests/probes/srtp_adapter.rs tests/probes/rtcp.rs tests/probes/sdes.rs tests/probes/secure_controls.rs tests/probes/secure_call.rs tests/probes/secure_call_example.rs tests/probes/prepare_secure_call.py tests/probes/observe_secure_call.py tests/probes/verify_secure_dependencies.py tests/probes/secure_call_inputs.json; do
    if ! grep -E '^#[0-9]+ [0-9.]+ CONTEXT_REJECTED:' "$temporary/negative.log" | grep -Fq "'$sentinel'"; then
        echo "ERROR: sentinel not identified by the audit: $sentinel" >&2
        exit 1
    fi
done
mv "$temporary/dockerignore.safe" "$scratch/.dockerignore"
rm -rf "$scratch/base" "$scratch/.git" "$scratch/dist" "$scratch/artifacts" "$scratch/tests"
rm -f "$scratch/.env" "$scratch/.env.local" "$scratch/profile.sqlite3" "$scratch/sample.AppImage" "$scratch/apps/rust-microsip/src/private.env"
rm -f "$scratch/crates/telephony/src/private.env" "$scratch/crates/telephony/examples/private.env"
# Build without base/, including offline verification; no host Cargo required.
docker buildx build --platform linux/amd64 --target artifacts \
    --output "type=local,dest=$root/dist" "$scratch"
(cd "$root/dist" && sha256sum --check SHA256SUMS)
