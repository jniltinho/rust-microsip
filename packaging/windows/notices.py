"""THIRD-PARTY-NOTICES.txt and a CycloneDX SBOM for the Windows packages
(add-windows-port task 4.3).

Usage: python3 packaging/windows/notices.py <cargo-notices-dir> <out.txt> <sbom.cdx.json> <version>
<cargo-notices-dir> is the output of collect-cargo-notices.py run with
RMS_NOTICE_TARGET=x86_64-pc-windows-gnu. Adds the statically linked Rust
standard library and the mingw-w64 runtime (libgcc, winpthreads) notices
from the windows-toolchain image. Deterministic: sorted, no timestamps.
"""

import html
import json
import re
import sys
from pathlib import Path

RUST_DOC = Path("/opt/rust/share/doc/rust")
MINGW = [Path("/usr/share/doc/gcc-mingw-w64-base/copyright"), Path("/usr/share/doc/mingw-w64-common/copyright")]


def section(title: str, body: str) -> str:
    return f"{'=' * 78}\n{title}\n{'=' * 78}\n\n{body.strip()}\n\n"


def main() -> None:
    cargo, out, sbom, version = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3]), sys.argv[4]
    inventory = json.loads((cargo / "inventory.json").read_text())
    if inventory["target"] != "x86_64-pc-windows-gnu":
        raise SystemExit(f"notices are for {inventory['target']}, not Windows")
    parts = ["rust-microsip for Windows: third-party notices\n\n"
             "rust-microsip itself is GPL-3.0-or-later (LICENSE.txt). The executables also contain\n"
             "the Rust standard library, the mingw-w64 runtime and the Rust crates listed below.\n\n"]
    library = html.unescape(re.sub(r"<[^>]+>", "", (RUST_DOC / "COPYRIGHT-library.html").read_text()))
    parts.append(section("Rust standard library", re.sub(r"\n\s*\n+", "\n\n", library)))
    for text in sorted((RUST_DOC / "licenses").glob("*.txt")):
        parts.append(section(f"Rust standard library license text: {text.stem}", text.read_text()))
    for copyright in MINGW:
        parts.append(section(f"mingw-w64 runtime: {copyright.parent.name}", copyright.read_text()))
    for package in inventory["packages"]:
        directory = cargo / f"{package['name']}-{package['version']}"
        title = f"{package['name']} {package['version']} ({package['declared_license']})"
        texts = [f"--- {name}\n\n{(directory / name).read_text(errors='replace').strip()}"
                 for name in sorted(package["notices_sha256"])]
        parts.append(section(title, "\n\n".join(texts)))
    out.write_text("".join(parts))
    scope = "selected Windows normal/build graph of rust-microsip; not a PE link map"
    components = [{"type": "library", "name": "Rust standard library",
                   "properties": [{"name": "rust-microsip:license-inventory",
                                   "value": "see THIRD-PARTY-NOTICES.txt; no single license expression asserted"}]},
                  {"type": "library", "name": "mingw-w64 runtime (libgcc, winpthreads)",
                   "properties": [{"name": "rust-microsip:license-inventory",
                                   "value": "see THIRD-PARTY-NOTICES.txt"}]}]
    components += [{"type": "library", "name": p["name"], "version": p["version"],
                    "purl": f"pkg:cargo/{p['name']}@{p['version']}",
                    "hashes": [{"alg": "SHA-256", "content": p["archive_sha256"]}],
                    "licenses": [{"expression": p.get("license_choice") or p["declared_license"]}]}
                   for p in inventory["packages"]]
    sbom.write_text(json.dumps({
        "bomFormat": "CycloneDX", "specVersion": "1.6", "version": 1,
        "metadata": {"component": {"type": "application", "name": "rust-microsip", "version": version,
                                   "licenses": [{"license": {"id": "GPL-3.0-or-later"}}]}},
        "components": components,
        "properties": [{"name": "rust-microsip:scope", "value": scope}],
    }, indent=2) + "\n")


if __name__ == "__main__":
    main()
