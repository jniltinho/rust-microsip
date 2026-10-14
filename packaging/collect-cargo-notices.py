"""Preserve checksum-verified registry notices for the selected Linux graph.

The selected graph is the `cargo tree` normal/build/dev output for the exact
target, intersected with `cargo metadata` (whose resolve is an optional-feature
superset). Expressions outside LICENSES need an exact reviewed exception in
reviewed-licenses.json; archives without notices need commit-pinned upstream
files in third-party-notices/. This is an inventory, not a vulnerability
assessment, legal certification or a complete release SBOM.
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
# Path-patched copies of registry crates (vendor/README.md): exact identity,
# the original archive checksum, the directory and the pinned notice files.
VENDORED = {
    ("wgpu-hal", "30.0.1"): {
        "archive_sha256": "b6b7fb58561a792bc237628ba0792e332de418fefe145f13b5ed8201e6d52f58",
        "dir": "vendor/wgpu-hal",
        "notices": {
            "LICENSE.APACHE": "a6cba85bc92e0cff7a450b1d873c0eaa2e9fc96bf472df0247a26bec77bf3ff9",
            "LICENSE.MIT": "dc0d97139e8205818c703741c7be7cb3b96888bd5917b8d6fc6133731e403c21",
        },
    },
}
NOTICE_PREFIXES = ("license", "licence", "copying", "copyright", "notice", "unlicense")
# Bundled-component terms often live as LICENSES/<SPDX-id>.txt (REUSE layout).
NOTICE_DIRECTORIES = {"licenses", "licences"}
HERE = Path(__file__).resolve().parent
SHA256 = re.compile(r"[a-f0-9]{64}")
UPSTREAM_URL = re.compile(r"https://raw\.githubusercontent\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/[a-f0-9]{40}/[A-Za-z0-9_.+-]+")
LICENSES = {
    "MIT", "MIT OR Apache-2.0", "Apache-2.0 OR MIT", "MIT OR BSD-3-Clause",
    "Unlicense OR MIT", "Apache-2.0 AND ISC", "Apache-2.0 OR ISC OR MIT",
    "ISC", "BSD-3-Clause", "(MIT OR Apache-2.0) AND Unicode-3.0",
}


def load_reviewed(path):
    """Load exact (name, version, archive, expression) license exceptions."""
    reviewed = {}
    for entry in json.loads(Path(path).read_text()):
        if (set(entry) != {"name", "version", "archive_sha256", "declared_expression", "chosen_license", "reason"}
                or not all(isinstance(value, str) and value.strip() for value in entry.values())
                or not SHA256.fullmatch(entry["archive_sha256"])
                or (entry["name"], entry["version"]) in reviewed):
            raise ValueError("invalid reviewed license entry")
        reviewed[(entry["name"], entry["version"])] = entry
    return reviewed


def load_supplements(root):
    """Load the commit-pinned upstream notice manifest for exact archives."""
    supplements = {}
    for entry in json.loads((Path(root) / "manifest.json").read_text()):
        key = (entry.get("name"), entry.get("version"))
        if (set(entry) != {"name", "version", "archive_sha256", "files"} or key in supplements
                or not SHA256.fullmatch(entry["archive_sha256"]) or not entry["files"]):
            raise ValueError("invalid supplemental notice entry")
        for item in entry["files"]:
            if (set(item) != {"path", "url", "sha256"} or not SHA256.fullmatch(item["sha256"])
                    or not re.fullmatch(r"[A-Za-z0-9_.+-]+", item["path"]) or item["path"] in {".", ".."}
                    or not UPSTREAM_URL.fullmatch(item["url"]) or not item["url"].endswith("/" + item["path"])):
                raise ValueError("invalid supplemental notice entry")
        supplements[key] = entry
    return supplements


def supplemental_notices(entry, checksum, directory):
    """Read pinned upstream notices only for the exact archive they were reviewed for."""
    if entry["archive_sha256"] != checksum:
        raise ValueError("supplemental notice identity mismatch")
    notices = {}
    for item in entry["files"]:
        path = directory / item["path"]
        if path.is_symlink() or not path.is_file() or path.stat().st_size > 1024 * 1024:
            raise ValueError("invalid supplemental notice file")
        content = path.read_bytes()
        if hashlib.sha256(content).hexdigest() != item["sha256"]:
            raise ValueError("supplemental notice hash mismatch")
        notices[item["path"]] = content
    return notices


def selected_identities(tree):
    """Parse `cargo tree --format '{p}|{f}'` output into (name, version) pairs."""
    found = set()
    for line in tree.splitlines():
        if not line.strip():
            continue
        match = re.fullmatch(r"([A-Za-z0-9_-]+) v([0-9][A-Za-z0-9.+-]*)(?: \([^\r\n]*\))?", line.split("|", 1)[0])
        if match is None or "|" not in line:
            raise ValueError("invalid dependency tree")
        found.add(match.groups())
    if not found:
        raise ValueError("invalid dependency tree")
    return found


def verify_license(name, version, expression, notice_paths, reviewed=None, checksum=None):
    """Accept the allowlist, the legacy crate, or one exact reviewed exception."""
    if expression in LICENSES:
        return
    entry = (reviewed or {}).get((name, version))
    if (entry is not None and checksum is not None and entry["archive_sha256"] == checksum
            and entry["declared_expression"] == expression):
        return
    # The verified 0.9.5 archive supplies both complete texts. Do not interpret
    # arbitrary slash expressions or extend this exception to another version.
    if ((name, version, expression) == ("version_check", "0.9.5", "MIT/Apache-2.0")
            and {"LICENSE-MIT", "LICENSE-APACHE"} <= set(notice_paths)):
        return
    raise ValueError("unreviewed license expression or missing legacy notices")


def verified_notices(archive, checksum, prefix, allow_empty=False):
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
            if not member.isfile() or not (parts[-1].lower().startswith(NOTICE_PREFIXES)
                                           or NOTICE_DIRECTORIES & {p.lower() for p in parts[1:-1]}):
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
    if (not notices and not allow_empty) or sum(map(len, notices.values())) > 8 * 1024 * 1024:
        raise ValueError("missing or excessive notices")
    return notices


def vendored_notices(package, root):
    """Pinned notices of a reviewed vendored crate, or ValueError."""
    entry = VENDORED.get((package["name"], package["version"]))
    if entry is None or Path(package["manifest_path"]).resolve() != (root / entry["dir"] / "Cargo.toml").resolve():
        raise ValueError("unreviewed path dependency")
    notices = {}
    for relative, digest in entry["notices"].items():
        path = root / entry["dir"] / relative
        if path.is_symlink() or not path.is_file() or path.stat().st_size > 1024 * 1024:
            raise ValueError("missing vendored notice")
        content = path.read_bytes()
        if hashlib.sha256(content).hexdigest() != digest:
            raise ValueError("vendored notice changed")
        notices[relative] = content
    return entry, notices


def collect(metadata, lock, cache, output, tree=None, reviewed=None, supplements=None,
            supplement_root=HERE / "third-party-notices", vendor_root=HERE.parent):
    """Inventory selected registry nodes and preserve their original notices.

    With `tree`, metadata-only nodes (not built for the target) are listed but
    not inventoried; tree identities missing from metadata are rejected.
    """
    supplements = supplements or {}
    selected = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    metadata_only = []
    if tree is not None:
        known = {(p["name"], p["version"]) for p in metadata["packages"] if p["id"] in selected}
        if not tree <= known:
            raise ValueError("selected tree identity missing from metadata")
        for package in metadata["packages"]:
            if package["id"] in selected and (package["name"], package["version"]) not in tree:
                if package["source"] is not None:
                    metadata_only.append({"name": package["name"], "version": package["version"]})
                del selected[package["id"]]
    locked = {(p["name"], p["version"]): p for p in lock["package"] if "source" in p}
    rows = []
    for package in metadata["packages"]:
        if package["id"] not in selected:
            continue
        if package["source"] is None:
            if package["id"] in metadata["workspace_members"]:
                continue
            entry, notices = vendored_notices(package, vendor_root)
            verify_license(package["name"], package["version"], package["license"], notices)
            hashes = {}
            for relative, content in sorted(notices.items()):
                path = output / (package["name"] + "-" + package["version"]) / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                with path.open("xb") as target:
                    target.write(content)
                hashes[relative] = hashlib.sha256(content).hexdigest()
            rows.append({"name": package["name"], "version": package["version"],
                         "source": "vendored:" + entry["dir"], "archive_sha256": entry["archive_sha256"],
                         "declared_license": package["license"], "license_choice": None,
                         "license_basis": "allowlist-or-legacy", "notice_sources": ["vendored-pinned"],
                         "features": sorted(selected[package["id"]]["features"]), "notices_sha256": hashes})
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
        supplement = supplements.get((name, version))
        notices = verified_notices(archives[0].read_bytes(), pin["checksum"], name + "-" + version,
                                   allow_empty=supplement is not None)
        upstream = {}
        if supplement is not None:
            upstream = supplemental_notices(supplement, pin["checksum"], supplement_root / (name + "-" + version))
            if set(upstream) & set(notices):
                raise ValueError("supplemental notice collides with packaged notice")
            notices.update(upstream)
        verify_license(name, version, package["license"], notices, reviewed, pin["checksum"])
        entry = (reviewed or {}).get((name, version))
        choice = entry["chosen_license"] if entry and package["license"] not in LICENSES else None
        hashes = {}
        for relative, content in sorted(notices.items()):
            path = output / (name + "-" + version) / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            with path.open("xb") as target:
                target.write(content)
            hashes[relative] = hashlib.sha256(content).hexdigest()
        rows.append({"name": name, "version": version, "source": REGISTRY,
                     "archive_sha256": pin["checksum"], "declared_license": package["license"],
                     "license_choice": choice, "license_basis": "reviewed-exception" if choice else "allowlist-or-legacy",
                     "notice_sources": sorted({"upstream-pinned" if r in upstream else "archive" for r in hashes}),
                     "features": sorted(selected[package["id"]]["features"]), "notices_sha256": hashes})
    return {"schema_version": 1, "target": "x86_64-unknown-linux-gnu",
            "scope": "selected workspace graph including G0 example dev/build dependencies; not an ELF-link map or release approval",
            "packages": sorted(rows, key=lambda row: (row["name"], row["version"])),
            "metadata_only_not_built": sorted(metadata_only, key=lambda row: (row["name"], row["version"]))}


if __name__ == "__main__":
    try:
        if len(sys.argv) != 3:
            raise ValueError("expected output directory and cargo tree file")
        output = Path(sys.argv[1])
        tree = selected_identities(Path(sys.argv[2]).read_text())
        output.mkdir(exist_ok=False, parents=True)
        metadata = json.loads(subprocess.check_output([
            "cargo", "metadata", "--frozen", "--format-version", "1",
            "--filter-platform", "x86_64-unknown-linux-gnu"]))
        report = collect(metadata, tomllib.loads(Path("Cargo.lock").read_text()),
                         Path(os.environ["CARGO_HOME"]) / "registry/cache", output, tree,
                         load_reviewed(HERE / "reviewed-licenses.json"),
                         load_supplements(HERE / "third-party-notices"))
        (output / "inventory.json").write_text(json.dumps(report, indent=2) + "\n")
    except (ValueError, OSError, KeyError, TypeError, AttributeError, tarfile.TarError, subprocess.CalledProcessError):
        print("Cargo notice inventory failed; no publication is approved.", file=sys.stderr)
        sys.exit(1)
