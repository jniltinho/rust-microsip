# syntax=docker/dockerfile:1.19.0@sha256:b6afd42430b15f2d2a4c5a02b919e98a525b785b1aaff16747d2f623364e39b6
FROM curlimages/curl:8.17.0@sha256:43ebaa53d3806db6b1ce4353b6b26ae638ec1c167ee351524b05690f988bb20d AS bootstrap-ca

FROM ubuntu:24.04@sha256:f610ab94648195aa356059f5b41d6085c9d4d903c072430cdd1af7bdb646106b AS toolchain

# The snapshot also pins transitive builder tool versions.
# Ubuntu base has no CAs: bootstrap by digest without disabling TLS validation.
COPY --from=bootstrap-ca /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
RUN printf '%s\n' \
        'Types: deb' \
        'URIs: https://snapshot.ubuntu.com/ubuntu/20261001T000000Z/' \
        'Suites: noble noble-updates noble-security' \
        'Components: main universe' \
        'Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg' \
        'Check-Valid-Until: no' > /etc/apt/sources.list.d/ubuntu.sources \
    && apt-get update --error-on=any \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        ca-certificates curl xz-utils build-essential binutils python3 \
        pkgconf libasound2-dev \
    && rm -rf /var/lib/apt/lists/*
COPY docker/rust-components.sha256 /tmp/rust-components.sha256
RUN set -eu; cd /tmp; \
    while read -r checksum archive; do \
        curl --fail --show-error --silent --location --retry 3 \
            "https://static.rust-lang.org/dist/2026-10-01/$archive" -o "$archive"; \
        printf '%s  %s\n' "$checksum" "$archive" | sha256sum --check --strict -; \
        tar -xJf "$archive"; \
        "${archive%.tar.xz}/install.sh" --prefix=/opt/rust --disable-ldconfig; \
        test -s "${archive%.tar.xz}/LICENSE-MIT"; \
        test -s "${archive%.tar.xz}/LICENSE-APACHE"; \
        case "$archive" in \
            rustc-*|rust-std-*) test -s "${archive%.tar.xz}/COPYRIGHT" ;; \
            cargo-*) test -s "${archive%.tar.xz}/LICENSE-THIRD-PARTY" ;; \
        esac; \
        mkdir -p "/opt/rust-licenses/${archive%.tar.xz}"; \
        find "${archive%.tar.xz}" -maxdepth 1 -type f \
            \( -name 'LICENSE*' -o -name COPYRIGHT \) \
            -exec cp '{}' "/opt/rust-licenses/${archive%.tar.xz}/" \;; \
        rm -rf "$archive" "${archive%.tar.xz}"; \
    done < /tmp/rust-components.sha256; \
    test -s /opt/rust/share/doc/rust/COPYRIGHT.html; \
    test -s /opt/rust/share/doc/rust/COPYRIGHT-library.html; \
    test -s /opt/rust/share/doc/rust/licenses/MIT.txt; \
    mkdir -p /opt/rust-licenses/generated-notices; \
    cp /opt/rust/share/doc/rust/COPYRIGHT.html /opt/rust/share/doc/rust/COPYRIGHT-library.html /opt/rust-licenses/generated-notices/; \
    cp -R /opt/rust/share/doc/rust/licenses /opt/rust-licenses/generated-notices/; \
    useradd --uid 10001 --create-home builder; \
    mkdir -p /work /out; chown builder:builder /work /out
ENV PATH="/opt/rust/bin:${PATH}" CARGO_HOME=/home/builder/.cargo
WORKDIR /work
USER builder

# Windows cross toolchain (add-windows-port W1): mingw-w64 and msitools (wixl) from the same
# snapshot and the checksummed Rust std for x86_64-pc-windows-gnu.
FROM toolchain AS windows-toolchain
USER root
RUN apt-get update --error-on=any \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 binutils-mingw-w64-x86-64 msitools wixl faketime \
    && rm -rf /var/lib/apt/lists/*
COPY docker/rust-components-windows.sha256 /tmp/rust-components-windows.sha256
RUN set -eu; cd /tmp; \
    while read -r checksum archive; do \
        curl --fail --show-error --silent --location --retry 3 \
            "https://static.rust-lang.org/dist/2026-10-01/$archive" -o "$archive"; \
        printf '%s  %s\n' "$checksum" "$archive" | sha256sum --check --strict -; \
        tar -xJf "$archive"; \
        "${archive%.tar.xz}/install.sh" --prefix=/opt/rust --disable-ldconfig; \
        rm -rf "$archive" "${archive%.tar.xz}"; \
    done < /tmp/rust-components-windows.sha256
# Static libgcc/winpthreads (the exe imports only Windows system DLLs) and
# deterministic PE headers.
ENV CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS="-C link-arg=-static -C link-arg=-Wl,--no-insert-timestamp"
USER builder

FROM toolchain AS source
COPY --chown=builder:builder Cargo.toml Cargo.lock rust-toolchain.toml LICENSE NOTICE Dockerfile .dockerignore ./
COPY --chown=builder:builder docker/rust-components.sha256 docker/
COPY --chown=builder:builder apps/ apps/
COPY --chown=builder:builder crates/ crates/
COPY --chown=builder:builder vendor/ vendor/
COPY --chown=builder:builder packaging/ packaging/

# Only this stage may access the registry when external crates are introduced.
FROM source AS fetch
RUN cargo fetch --locked --target x86_64-unknown-linux-gnu

FROM fetch AS verify
RUN --network=none sh packaging/verify.sh

FROM verify AS build
RUN --network=none cargo build --workspace --release --frozen --target x86_64-unknown-linux-gnu \
    && sh packaging/export-scaffold.sh

# Windows cross build (add-windows-port tasks 1.2 and 5.1): clippy, both
# release exes and the test executables run on a windows-latest runner.
FROM windows-toolchain AS windows-source
COPY --chown=builder:builder Cargo.toml Cargo.lock rust-toolchain.toml LICENSE NOTICE Dockerfile .dockerignore ./
COPY --chown=builder:builder docker/rust-components.sha256 docker/rust-components-windows.sha256 docker/
COPY --chown=builder:builder apps/ apps/
COPY --chown=builder:builder crates/ crates/
COPY --chown=builder:builder vendor/ vendor/
COPY --chown=builder:builder packaging/ packaging/

FROM windows-source AS windows-fetch
# All platforms: the host side (build scripts, proc macros) also resolves.
RUN cargo fetch --locked

FROM windows-fetch AS windows-build
RUN --network=none set -eu; \
    cargo clippy --workspace --lib --bins --tests --target x86_64-pc-windows-gnu --frozen -- -D warnings; \
    cargo build -p rust-microsip --release --target x86_64-pc-windows-gnu --frozen; \
    cargo test --workspace --lib --bins --tests --target x86_64-pc-windows-gnu --frozen --no-run --message-format=json \
        | python3 packaging/windows/collect-tests.py /out/windows/tests; \
    version=$(awk '/^\[workspace.package\]/ { s = 1; next } /^\[/ { s = 0 } s && /^version = / { gsub(/"/, "", $3); print $3; exit }' Cargo.toml); \
    sh packaging/windows/package.sh target/x86_64-pc-windows-gnu/release /out/windows "$version"; \
    sh packaging/windows/msi.sh target/x86_64-pc-windows-gnu/release /out/windows/upgrade-test 99.0.0 NOTICE; \
    cd /out/windows && find . -type f ! -name SHA256SUMS | sort | xargs sha256sum > SHA256SUMS

FROM scratch AS windows-artifacts
COPY --from=windows-build /out/windows/ /

# AppImage tools from the same pinned snapshot plus the pinned official runtime.
FROM toolchain AS appimage-tools
USER root
COPY packaging/appimage/pins.txt /tmp/appimage-pins.txt
RUN set -eu; apt-get update --error-on=any \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        squashfs-tools librsvg2-bin desktop-file-utils \
    && rm -rf /var/lib/apt/lists/*; \
    mkdir -p /opt/appimage; \
    awk '$2 == "runtime-x86_64" { print $1, $3 }' /tmp/appimage-pins.txt | { \
        read -r checksum url; \
        curl --fail --show-error --silent --location --retry 3 "$url" -o /opt/appimage/runtime-x86_64; \
        printf '%s  %s\n' "$checksum" /opt/appimage/runtime-x86_64 | sha256sum --check --strict -; }; \
    chmod 0644 /opt/appimage/runtime-x86_64
USER builder

# Convenience AppImage around the unchanged ELF; the ELF remains the primary artifact.
FROM appimage-tools AS appimage
COPY --from=build --chown=builder:builder /out/ /out/
COPY --chown=builder:builder packaging/appimage/ packaging/appimage/
COPY --chown=builder:builder apps/rust-microsip/assets/OFL.txt apps/rust-microsip/assets/OFL.txt
COPY --chown=builder:builder apps/rust-microsip/assets/icons/rust-microsip.svg apps/rust-microsip/assets/icons/rust-microsip.svg
RUN --network=none sh packaging/appimage/build-appimage.sh

FROM scratch AS artifacts
COPY --from=appimage /out/ /

# Inspect the filtered context using synthetic sentinels in tests.
FROM toolchain AS context-audit
COPY --chown=builder:builder . /context/
RUN --network=none python3 - <<'PY'
from pathlib import Path

root = Path("/context")
allowed = {
    "Dockerfile", ".dockerignore", "Cargo.toml", "Cargo.lock",
    "rust-toolchain.toml", "LICENSE", "NOTICE",
    "apps/rust-microsip/Cargo.toml", "apps/rust-microsip/build.rs", "apps/rust-microsip/windows/rust-microsip.manifest", "crates/telephony/Cargo.toml", "crates/media/Cargo.toml", "docker/rust-components.sha256", "docker/rust-components-windows.sha256",
    "crates/core/Cargo.toml", "crates/audio/Cargo.toml", "crates/config/Cargo.toml", "crates/storage/Cargo.toml", "crates/platform/Cargo.toml", "crates/platform-windows/Cargo.toml", "crates/control/Cargo.toml",
    "packaging/reviewed-licenses.json",
    "apps/rust-microsip/assets/NotoSans-Regular.ttf", "apps/rust-microsip/assets/OFL.txt",
    "apps/rust-microsip/assets/source.json",
    "packaging/verify.sh", "packaging/export-scaffold.sh", "packaging/windows/collect-tests.py", "packaging/windows/zip.py", "packaging/windows/msi.sh", "packaging/windows/package.sh", "packaging/windows/notices.py", "packaging/windows/rust-microsip.wxs", "packaging/windows/MsiShortcutProperty.idt",
    "packaging/appimage/build-appimage.sh", "packaging/appimage/verify.sh",
    "packaging/appimage/pins.txt", "packaging/appimage/rust-microsip.desktop",
    "apps/rust-microsip/assets/icons/rust-microsip.svg", "apps/rust-microsip/assets/icons/rust-microsip-256.png",
    "packaging/check-g0-dependencies.py", "packaging/tests/test_g0_dependencies.py",
    "packaging/collect-cargo-notices.py", "packaging/tests/test_cargo_notices.py",
}
files = [p for p in root.rglob("*") if p.is_file()]
unexpected = []
for path in files:
    relative = str(path.relative_to(root))
    is_source = path.suffix == ".rs" and str(path.parent.relative_to(root)) in {
        "apps/rust-microsip/src", "apps/rust-microsip/src/bin", "apps/rust-microsip/tests",
        "crates/telephony/src", "crates/telephony/tests", "crates/telephony/examples",
        "crates/media/src", "crates/media/tests", "apps/rust-microsip/src/ui", "apps/rust-microsip/src/mcp", "apps/rust-microsip/examples",
        "crates/telephony/src/engine", "crates/core/src", "crates/core/tests", "crates/audio/src", "crates/config/src", "crates/storage/src", "crates/storage/tests", "crates/platform/src", "crates/platform/src/control", "crates/platform-windows/src", "crates/control/src", "apps/rust-microsip/src/ui/state",
    }
    # Reviewed, commit-pinned third-party license texts only.
    is_source = is_source or relative.startswith(("packaging/third-party-notices/", "packaging/appimage/runtime-licenses/", "vendor/wgpu-hal/"))
    if relative not in allowed and not is_source:
        unexpected.append(relative)
if unexpected:
    raise SystemExit("CONTEXT_REJECTED: unexpected files: " + repr(sorted(unexpected)))
marker = "synthetic-" + "private-context-marker"
if any(marker in p.read_text(errors="replace") for p in files):
    raise SystemExit("CONTEXT_REJECTED: synthetic private content")
PY

# Lab suite (docker/asterisk/suite.sh): the release app and the production
# engine examples with a private X server and AT-SPI. Local test image only;
# never published. Scenario scripts are mounted read-only at run time.
# `build` is FROM `verify`: building this stage runs fmt, clippy, all
# workspace tests and rustdoc for the same sources first.
FROM build AS suite-build
RUN --network=none cargo build -p rust-microsip-telephony --example engine_lab --example engine_network \
        --release --frozen --target x86_64-unknown-linux-gnu \
    && cargo build -p rust-microsip --example mcp_probe --release --frozen --target x86_64-unknown-linux-gnu
FROM ubuntu:24.04@sha256:f610ab94648195aa356059f5b41d6085c9d4d903c072430cdd1af7bdb646106b AS suite-runner
COPY --from=bootstrap-ca /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
RUN printf '%s\n' \
        'Types: deb' \
        'URIs: https://snapshot.ubuntu.com/ubuntu/20261001T000000Z/' \
        'Suites: noble noble-updates noble-security' \
        'Components: main universe' \
        'Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg' \
        'Check-Valid-Until: no' > /etc/apt/sources.list.d/ubuntu.sources \
    && apt-get update --error-on=any \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        libasound2t64 libx11-6 libx11-xcb1 libxcb1 libxkbcommon0 libxkbcommon-x11-0 \
        libwayland-client0 libwayland-cursor0 libwayland-egl1 \
        libegl1 libgl1-mesa-dri libglx-mesa0 mesa-vulkan-drivers \
        xvfb x11-apps xdotool imagemagick procps \
        dbus dbus-x11 at-spi2-core python3-pyatspi gir1.2-atspi-2.0 \
    && rm -rf /var/lib/apt/lists/*
# A null ALSA device: calls really open and release capture and playback
# (no sound card in the container, nothing is recorded).
RUN printf '%s\n' 'pcm.!default { type null }' 'ctl.!default { type hw card 0 }' > /etc/asound.conf
COPY --from=suite-build /work/target/x86_64-unknown-linux-gnu/release/rust-microsip /usr/local/bin/rust-microsip
COPY --from=suite-build /work/target/x86_64-unknown-linux-gnu/release/examples/engine_lab /usr/local/bin/engine-lab
COPY --from=suite-build /work/target/x86_64-unknown-linux-gnu/release/examples/engine_network /usr/local/bin/engine-network
COPY --from=suite-build /work/target/x86_64-unknown-linux-gnu/release/examples/mcp_probe /usr/local/bin/mcp-probe
COPY --from=suite-build /work/LICENSE /work/NOTICE /usr/share/rust-microsip-suite/
USER 10001:10001

# Lab-only probe: same adapter source/lock as the workspace, not a client release.
FROM verify AS g0-registration-build
RUN --network=none cargo build -p rust-microsip-telephony --example g0_registration --release --frozen --target x86_64-unknown-linux-gnu
FROM ubuntu:24.04@sha256:f610ab94648195aa356059f5b41d6085c9d4d903c072430cdd1af7bdb646106b AS g0-registration-runner
COPY --from=g0-registration-build /work/target/x86_64-unknown-linux-gnu/release/examples/g0_registration /usr/local/bin/g0-registration
COPY --from=g0-registration-build /tmp/g0-cargo-notices /usr/share/rust-microsip-g0/cargo-licenses
COPY --from=g0-registration-build /opt/rust-licenses /usr/share/rust-microsip-g0/toolchain-licenses
COPY --from=g0-registration-build /work/LICENSE /work/NOTICE /usr/share/rust-microsip-g0/
USER 10001:10001
ENTRYPOINT ["/usr/local/bin/g0-registration"]

# Synthetic call proof only; no desktop/audio device access or release claim.
FROM verify AS g0-calls-build
RUN --network=none cargo build -p rust-microsip-telephony --example g0_calls --release --frozen --target x86_64-unknown-linux-gnu
FROM ubuntu:24.04@sha256:f610ab94648195aa356059f5b41d6085c9d4d903c072430cdd1af7bdb646106b AS g0-calls-runner
COPY --from=g0-calls-build /work/target/x86_64-unknown-linux-gnu/release/examples/g0_calls /usr/local/bin/g0-calls
COPY --from=g0-calls-build /tmp/g0-cargo-notices /usr/share/rust-microsip-g0/cargo-licenses
COPY --from=g0-calls-build /opt/rust-licenses /usr/share/rust-microsip-g0/toolchain-licenses
COPY --from=g0-calls-build /work/LICENSE /work/NOTICE /usr/share/rust-microsip-g0/
USER 10001:10001
ENTRYPOINT ["/usr/local/bin/g0-calls"]

# RFC 4733 IVR proof only; no desktop/audio device access or release claim.
FROM verify AS g0-controls-build
RUN --network=none cargo build -p rust-microsip-telephony --example g0_controls --release --frozen --target x86_64-unknown-linux-gnu
FROM ubuntu:24.04@sha256:f610ab94648195aa356059f5b41d6085c9d4d903c072430cdd1af7bdb646106b AS g0-controls-runner
COPY --from=g0-controls-build /work/target/x86_64-unknown-linux-gnu/release/examples/g0_controls /usr/local/bin/g0-controls
COPY --from=g0-controls-build /tmp/g0-cargo-notices /usr/share/rust-microsip-g0/cargo-licenses
COPY --from=g0-controls-build /opt/rust-licenses /usr/share/rust-microsip-g0/toolchain-licenses
COPY --from=g0-controls-build /work/LICENSE /work/NOTICE /usr/share/rust-microsip-g0/
USER 10001:10001
ENTRYPOINT ["/usr/local/bin/g0-controls"]

# SDP hold/resume and rejected re-INVITE proof, not a client release.
FROM verify AS g0-hold-build
RUN --network=none cargo build -p rust-microsip-telephony --example g0_hold --release --frozen --target x86_64-unknown-linux-gnu
FROM ubuntu:24.04@sha256:f610ab94648195aa356059f5b41d6085c9d4d903c072430cdd1af7bdb646106b AS g0-hold-runner
COPY --from=g0-hold-build /work/target/x86_64-unknown-linux-gnu/release/examples/g0_hold /usr/local/bin/g0-hold
COPY --from=g0-hold-build /tmp/g0-cargo-notices /usr/share/rust-microsip-g0/cargo-licenses
COPY --from=g0-hold-build /opt/rust-licenses /usr/share/rust-microsip-g0/toolchain-licenses
COPY --from=g0-hold-build /work/LICENSE /work/NOTICE /usr/share/rust-microsip-g0/
USER 10001:10001
ENTRYPOINT ["/usr/local/bin/g0-hold"]

# REFER/NOTIFY and preserved-call failure proof, not a client release.
FROM verify AS g0-transfer-build
RUN --network=none cargo build -p rust-microsip-telephony --example g0_transfer --release --frozen --target x86_64-unknown-linux-gnu
FROM ubuntu:24.04@sha256:f610ab94648195aa356059f5b41d6085c9d4d903c072430cdd1af7bdb646106b AS g0-transfer-runner
COPY --from=g0-transfer-build /work/target/x86_64-unknown-linux-gnu/release/examples/g0_transfer /usr/local/bin/g0-transfer
COPY --from=g0-transfer-build /tmp/g0-cargo-notices /usr/share/rust-microsip-g0/cargo-licenses
COPY --from=g0-transfer-build /opt/rust-licenses /usr/share/rust-microsip-g0/toolchain-licenses
COPY --from=g0-transfer-build /work/LICENSE /work/NOTICE /usr/share/rust-microsip-g0/
USER 10001:10001
ENTRYPOINT ["/usr/local/bin/g0-transfer"]
