"""Check selected cargo-tree features before G0 dependency adoption.

Input: cargo tree --prefix none --no-dedupe --format '{p}|{f}' -e normal,build
for the exact target and feature set being built. This deny boundary supplements,
but does not replace, full notice/source/security and native-linker audits.
"""
import re
import sys

MAX_BYTES = 1024 * 1024
BLOCKED = frozenset({
    "audio-codec", "rsip", "aws-lc-rs", "aws-lc-sys", "openssl", "openssl-sys",
    "openssl-src", "boring", "boring-sys", "bssl-sys", "srtp-sys",
    "epaint-default-fonts", "aws-lc-fips-sys", "libsrtp", "libsrtp-sys",
    "mbedtls-sys", "wolfssl-sys",
})


def verify(tree):
    """Reject unsafe/unreviewed selected providers without printing input."""
    if not tree.strip() or len(tree.encode("utf-8")) > MAX_BYTES:
        raise ValueError("invalid dependency tree")
    found = {}
    for line in tree.splitlines():
        if not line.strip():
            continue
        if line.count("|") != 1:
            raise ValueError("invalid dependency tree")
        package, raw_features = line.split("|")
        match = re.fullmatch(r"([A-Za-z0-9_-]+) v([0-9][A-Za-z0-9.+-]*)(?: \(([^\r\n]*)\))?", package)
        if match is None:
            raise ValueError("invalid dependency tree")
        name, version, source = match.groups()
        name = name.replace("_", "-")
        if name in {"ring", "rustls", "rustrtc"} and source is not None:
            raise ValueError("unreviewed provider source")
        raw_features = raw_features.removesuffix(" (*)")
        if raw_features and re.fullmatch(r"[A-Za-z0-9_+./,:-]+", raw_features) is None:
            raise ValueError("invalid dependency tree")
        features = set(filter(None, raw_features.split(",")))
        found.setdefault((name, version), set()).update(features)
    for (name, version), features in found.items():
        if name in BLOCKED:
            raise ValueError("blocked dependency")
        if name == "ring" and version != "0.17.14":
            raise ValueError("unreviewed ring version")
        if name == "rustls" and (version != "0.23.45" or "ring" not in features
                                or features & {"default", "aws_lc_rs", "aws-lc-rs", "fips"}):
            raise ValueError("unreviewed TLS provider")
        if name in {"eframe", "egui", "epaint"} and "default_fonts" in features:
            raise ValueError("unreviewed font bundle")
        if name == "zbus" and {"tokio", "async-io"} <= features:
            raise ValueError("unreviewed runtime union")
        if name == "rustrtc" and (version != "0.4.7" or features):
            raise ValueError("unreviewed media feature set")
    if any(name == "rustls" for name, _ in found) and ("ring", "0.17.14") not in found:
        raise ValueError("missing reviewed TLS provider")


if __name__ == "__main__":
    try:
        payload = sys.stdin.buffer.read(MAX_BYTES + 1)
        if len(payload) > MAX_BYTES:
            raise ValueError("excessive dependency tree")
        verify(payload.decode("utf-8"))
    except (ValueError, UnicodeError):
        print("G0 dependency boundary rejected; review selected crates/features.", file=sys.stderr)
        sys.exit(1)
    print("G0 dependency boundary OK (not a complete license/security audit).")
