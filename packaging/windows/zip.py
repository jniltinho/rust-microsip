"""Deterministic portable zip for Windows (add-windows-port W10, task 4.1).

Usage: python3 packaging/windows/zip.py <release-dir> <out-dir> <version>
Members in a fixed order with a fixed timestamp; writes SHA256SUMS for the
exes and the zip. A test build, not a release: notices for the Windows
dependency graph come with task 4.3.
"""

import hashlib
import sys
import zipfile
from pathlib import Path

STAMP = (2026, 1, 1, 0, 0, 0)


def main() -> None:
    release, out, version = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3]
    out.mkdir(parents=True, exist_ok=True)
    root = Path(__file__).resolve().parents[2]
    members = [
        (release / "rust-microsip.exe", "rust-microsip.exe"),
        (release / "rust-microsip-cli.exe", "rust-microsip-cli.exe"),
        (root / "LICENSE", "LICENSE.txt"),
        (root / "NOTICE", "NOTICE.txt"),
    ]
    name = f"rust-microsip-{version}-windows-x86_64.zip"
    with zipfile.ZipFile(out / name, "w", zipfile.ZIP_DEFLATED) as archive:
        for source, arc in members:
            info = zipfile.ZipInfo(f"rust-microsip-{version}/{arc}", STAMP)
            info.external_attr = 0o644 << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, source.read_bytes())
    sums = []
    for path in [release / "rust-microsip.exe", release / "rust-microsip-cli.exe", out / name]:
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        sums.append(f"{digest}  {path.name}\n")
        if path.parent != out:
            (out / path.name).write_bytes(path.read_bytes())
    (out / "SHA256SUMS").write_text("".join(sums))
    print(f"{out / name}")


if __name__ == "__main__":
    main()
