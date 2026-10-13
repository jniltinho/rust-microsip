"""Preserve checksum-verified registry notices for the selected Linux graph.

Includes dev/build dependencies used to compile the G0 example. This is an
inventory, not a vulnerability assessment or a complete release SBOM.
"""
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tarfile
import tomllib

REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
PROTECTED = {
    ("rsipstack", "0.7.3"): "fd6577026d74d23d0eb114d158ea1a036275129e9b2a791be1679baad6821948",
    ("rustls", "0.23.45"): "0d41d731c7d2f962d1ccc364cec258de3c0e93b38c2fb3ba97ac74513048d634",
    ("ring", "0.17.14"): "a4689e6c2294d81e88dc6261c768b63bc4fcdb852be6d1352498b114f61383b7",
    ("rustrtc", "0.4.7"): "7fc50a0fd78b4b7a61fab57d1e4ee915eb9a8d710c22333f231cf8fc79f3befe",
    ("version_check", "0.9.5"): "0b928f33d975fc6ad9f86c8f283853ad26bdd5b10b7f1542aa2fa15e2289105a",
}
NOTICE_PREFIXES = ("license", "licence", "copying", "copyright", "notice", "unlicense")
LICENSES = {
    "MIT", "MIT OR Apache-2.0", "Apache-2.0 OR MIT", "MIT OR BSD-3-Clause",
    "Unlicense OR MIT", "Apache-2.0 AND ISC", "Apache-2.0 OR ISC OR MIT",
    "ISC", "BSD-3-Clause", "(MIT OR Apache-2.0) AND Unicode-3.0",
}


def verify_license(name, version, expression, notice_paths):
    """Keep legacy spelling restricted to the source-pinned reviewed crate."""
    if expression in LICENSES:
        return
    # The verified 0.9.5 archive supplies both complete texts. Do not interpret
    # arbitrary slash expressions or extend this exception to another version.
    if ((name, version, expression) == ("version_check", "0.9.5", "MIT/Apache-2.0")
            and {"LICENSE-MIT", "LICENSE-APACHE"} <= set(notice_paths)):
        return
    raise ValueError("unreviewed license expression or missing legacy notices")


def verified_notices(archive, checksum, prefix):
    """Read bounded regular notice members without extracting archive paths."""
    if len(archive) > 16 * 1024 * 1024 or hashlib.sha256(archive).hexdigest() != checksum:
        raise ValueError("archive identity mismatch")
    notices = {}
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r|gz") as source:
        total = 0
        for index, member in enumerate(source):
            total += member.size
            if index >= 10000 or member.size < 0 or total > 64 * 1024 * 1024:
                raise ValueError("excessive archive contents")
            parts = member.name.split("/")
            if (parts[0] != prefix or any(p in {"", ".", ".."} for p in parts)
                    or member.issym() or member.islnk()):
                raise ValueError("unsafe archive member")
            if not member.isfile() or not parts[-1].lower().startswith(NOTICE_PREFIXES):
                continue
            if member.size < 1 or member.size > 1024 * 1024:
                raise ValueError("invalid notice size")
            relative = "/".join(parts[1:])
            if not relative or relative in notices:
                raise ValueError("invalid notice path")
            handle = source.extractfile(member)
            if handle is None:
                raise ValueError("missing notice contents")
            notices[relative] = handle.read(1024 * 1024 + 1)
    if not notices or sum(map(len, notices.values())) > 8 * 1024 * 1024:
        raise ValueError("missing or excessive notices")
    return notices


def collect(metadata, lock, cache, output):
    """Inventory selected registry nodes and preserve their original notices."""
    selected = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    locked = {(p["name"], p["version"]): p for p in lock["package"] if "source" in p}
    rows = []
    for package in metadata["packages"]:
        if package["id"] not in selected:
            continue
        if package["source"] is None:
            if package["id"] not in metadata["workspace_members"]:
                raise ValueError("unreviewed path dependency")
            continue
        name, version = package["name"], package["version"]
        if not re.fullmatch(r"[A-Za-z0-9_-]+", name) or not re.fullmatch(r"[0-9][A-Za-z0-9.+-]*", version):
            raise ValueError("invalid package identity")
        pin = locked.get((name, version))
        if (pin is None or pin["source"] != REGISTRY or package["source"] != REGISTRY
                or not re.fullmatch(r"[a-f0-9]{64}", pin.get("checksum", ""))):
            raise ValueError("unreviewed dependency source")
        if name in {key[0] for key in PROTECTED} and PROTECTED.get((name, version)) != pin["checksum"]:
            raise ValueError("protected dependency identity mismatch")
        archives = list(cache.glob("*/" + name + "-" + version + ".crate"))
        if len(archives) != 1 or archives[0].is_symlink() or archives[0].stat().st_size > 16 * 1024 * 1024:
            raise ValueError("missing or ambiguous registry archive")
        notices = verified_notices(archives[0].read_bytes(), pin["checksum"], name + "-" + version)
        verify_license(name, version, package["license"], notices)
        hashes = {}
        for relative, content in sorted(notices.items()):
            path = output / (name + "-" + version) / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            with path.open("xb") as target:
                target.write(content)
            hashes[relative] = hashlib.sha256(content).hexdigest()
        rows.append({"name": name, "version": version, "source": REGISTRY,
                     "archive_sha256": pin["checksum"], "declared_license": package["license"],
                     "features": sorted(selected[package["id"]]["features"]), "notices_sha256": hashes})
    return {"schema_version": 1, "target": "x86_64-unknown-linux-gnu",
            "scope": "selected workspace graph including G0 example dev/build dependencies; not an ELF-link map or release approval",
            "packages": sorted(rows, key=lambda row: (row["name"], row["version"]))}


if __name__ == "__main__":
    try:
        if len(sys.argv) != 2:
            raise ValueError("expected output directory")
        output = Path(sys.argv[1])
        output.mkdir(exist_ok=False, parents=True)
        metadata = json.loads(subprocess.check_output([
            "cargo", "metadata", "--frozen", "--format-version", "1",
            "--filter-platform", "x86_64-unknown-linux-gnu"]))
        report = collect(metadata, tomllib.loads(Path("Cargo.lock").read_text()),
                         Path(os.environ["CARGO_HOME"]) / "registry/cache", output)
        (output / "inventory.json").write_text(json.dumps(report, indent=2) + "\n")
    except (ValueError, OSError, KeyError, tarfile.TarError, subprocess.CalledProcessError):
        print("Cargo notice inventory failed; no publication is approved.", file=sys.stderr)
        sys.exit(1)
