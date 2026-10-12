#!/bin/sh
# Export only the scaffold, not an AppImage or an approved-G0 receipt.
set -eu
binary=target/x86_64-unknown-linux-gnu/release/rust-microsip
cp "$binary" /out/rust-microsip
cp LICENSE NOTICE /out/
cp -r /opt/rust-licenses /out/toolchain-licenses
readelf -d "$binary" > /out/elf-dynamic.txt
readelf --version-info "$binary" > /out/elf-versions.txt
dpkg-query -W -f='${Package}\t${Version}\n' > /out/builder-packages.txt
rustc --version --verbose > /out/toolchain.txt
python3 - <<'PY'
import hashlib
import json
import pathlib
import re
import subprocess

root = pathlib.Path('/work')
files = [root / name for name in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'LICENSE', 'NOTICE', 'Dockerfile', '.dockerignore', 'docker/rust-components.sha256')]
files += sorted((root / 'apps').rglob('*.rs'))
files += sorted((root / 'apps').rglob('Cargo.toml'))
files += sorted((root / 'packaging').glob('*.sh'))
files += sorted((root / 'packaging').rglob('*.py'))
sources = {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(files)}
digest = hashlib.sha256(json.dumps(sources, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
report = json.loads(subprocess.check_output(['/out/rust-microsip', '--doctor', '--json']))
dockerfile = (root / 'Dockerfile').read_text()
rust_version = subprocess.check_output(['rustc', '--version'], text=True).split()[1]
ubuntu_image = re.search(r'^FROM (ubuntu:[^ ]+) AS toolchain$', dockerfile, re.M)[1]
snapshot = re.search(r'URIs: https://snapshot.ubuntu.com/ubuntu/([0-9TZ]+)/', dockerfile)[1]
report.update({
    'artifact_kind': 'scaffold-not-release',
    'source_files_sha256': sources,
    'source_manifest_sha256': digest,
    'target': 'x86_64-unknown-linux-gnu',
    'rust_version': rust_version,
    'ubuntu_snapshot': snapshot,
    'ubuntu_image': ubuntu_image,
    'scope': 'CLI scaffolding only; no UI, SIP, audio, SQLite, IPC or AppImage',
})
pathlib.Path('/out/build-manifest.json').write_text(json.dumps(report, indent=2) + '\n')
sbom = {
    'bomFormat': 'CycloneDX', 'specVersion': '1.6', 'version': 1,
    'metadata': {'component': {'type': 'application', 'name': 'rust-microsip',
        'version': report['version'], 'licenses': [{'license': {'id': 'GPL-3.0-or-later'}}]}},
    'components': [{'type': 'library', 'name': 'Rust standard library', 'version': rust_version,
        'properties': [{'name': 'rust-microsip:license-inventory',
            'value': 'incomplete; Rust and third-party terms in toolchain-licenses/generated-notices/COPYRIGHT-library.html and licenses/; no single license expression asserted'}]}],
    'properties': [{'name': 'rust-microsip:scope', 'value': 'scaffold; no external Cargo crates; runtime libraries listed separately by readelf; full G0/release SBOM pending'}],
}
pathlib.Path('/out/scaffold-sbom.cdx.json').write_text(json.dumps(sbom, indent=2) + '\n')
PY
(cd /out && find . -type f ! -name SHA256SUMS -print0 | LC_ALL=C sort -z | xargs -0 sha256sum > SHA256SUMS)
