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

FROM toolchain AS source
COPY --chown=builder:builder Cargo.toml Cargo.lock rust-toolchain.toml LICENSE NOTICE Dockerfile .dockerignore ./
COPY --chown=builder:builder docker/rust-components.sha256 docker/
COPY --chown=builder:builder apps/ apps/
COPY --chown=builder:builder crates/ crates/
COPY --chown=builder:builder packaging/ packaging/

# Only this stage may access the registry when external crates are introduced.
FROM source AS fetch
RUN cargo fetch --locked --target x86_64-unknown-linux-gnu

FROM fetch AS verify
RUN --network=none sh packaging/verify.sh

FROM verify AS build
RUN --network=none cargo build --workspace --release --frozen --target x86_64-unknown-linux-gnu \
    && sh packaging/export-scaffold.sh

FROM scratch AS artifacts
COPY --from=build /out/ /

# Inspect the filtered context using synthetic sentinels in tests.
FROM toolchain AS context-audit
COPY --chown=builder:builder . /context/
RUN --network=none python3 - <<'PY'
from pathlib import Path

root = Path("/context")
allowed = {
    "Dockerfile", ".dockerignore", "Cargo.toml", "Cargo.lock",
    "rust-toolchain.toml", "LICENSE", "NOTICE",
    "apps/rust-microsip/Cargo.toml", "crates/telephony/Cargo.toml", "docker/rust-components.sha256",
    "packaging/verify.sh", "packaging/export-scaffold.sh",
    "packaging/check-g0-dependencies.py", "packaging/tests/test_g0_dependencies.py",
    "packaging/collect-cargo-notices.py", "packaging/tests/test_cargo_notices.py",
}
files = [p for p in root.rglob("*") if p.is_file()]
unexpected = []
for path in files:
    relative = str(path.relative_to(root))
    is_source = path.suffix == ".rs" and str(path.parent.relative_to(root)) in {
        "apps/rust-microsip/src", "apps/rust-microsip/tests",
        "crates/telephony/src", "crates/telephony/tests", "crates/telephony/examples",
    }
    if relative not in allowed and not is_source:
        unexpected.append(relative)
if unexpected:
    raise SystemExit("CONTEXT_REJECTED: unexpected files: " + repr(sorted(unexpected)))
marker = "synthetic-" + "private-context-marker"
if any(marker in p.read_text(errors="replace") for p in files):
    raise SystemExit("CONTEXT_REJECTED: synthetic private content")
PY

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
