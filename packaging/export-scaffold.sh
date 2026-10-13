#!/bin/sh
# Export the desktop ELF build candidate with notices, manifest and SBOM.
# Not an AppImage and not a release approval: release gates are separate.
set -eu
binary=target/x86_64-unknown-linux-gnu/release/rust-microsip
cp "$binary" /out/rust-microsip
cp LICENSE NOTICE /out/
cp -r /opt/rust-licenses /out/toolchain-licenses
cp -r /tmp/g0-cargo-notices /out/cargo-licenses
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
files += sorted((root / 'crates').rglob('*.rs'))
files += sorted((root / 'crates').rglob('Cargo.toml'))
files += sorted((root / 'packaging').glob('*.sh'))
files += sorted((root / 'packaging').rglob('*.py'))
files += sorted(p for p in (root / 'packaging' / 'appimage').rglob('*') if p.is_file())
files += sorted((root / 'apps' / 'rust-microsip' / 'assets').rglob('*.svg'))
sources = {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(files)}
digest = hashlib.sha256(json.dumps(sources, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
version = subprocess.check_output(['/out/rust-microsip', '--version'], text=True).split()[1]
report = {'schema_version': 2, 'version': version}
dockerfile = (root / 'Dockerfile').read_text()
rust_version = subprocess.check_output(['rustc', '--version'], text=True).split()[1]
ubuntu_image = re.search(r'^FROM (ubuntu:[^ ]+) AS toolchain$', dockerfile, re.M)[1]
snapshot = re.search(r'URIs: https://snapshot.ubuntu.com/ubuntu/([0-9TZ]+)/', dockerfile)[1]
report.update({
    'artifact_kind': 'build-candidate-not-release',
    'source_files_sha256': sources,
    'source_manifest_sha256': digest,
    'target': 'x86_64-unknown-linux-gnu',
    'rust_version': rust_version,
    'ubuntu_snapshot': snapshot,
    'ubuntu_image': ubuntu_image,
    'scope': 'desktop ELF: egui UI, SIP/RTP/SRTP engine, ALSA audio, TOML config, SQLite history/contacts, single instance, notifications; the AppImage wraps the same ELF',
})
pathlib.Path('/out/build-manifest.json').write_text(json.dumps(report, indent=2) + '\n')
sbom = {
    'bomFormat': 'CycloneDX', 'specVersion': '1.6', 'version': 1,
    'metadata': {'component': {'type': 'application', 'name': 'rust-microsip',
        'version': report['version'], 'licenses': [{'license': {'id': 'GPL-3.0-or-later'}}]}},
    'components': [{'type': 'library', 'name': 'Rust standard library', 'version': rust_version,
        'properties': [{'name': 'rust-microsip:license-inventory',
            'value': 'incomplete; Rust and third-party terms in toolchain-licenses/generated-notices/COPYRIGHT-library.html and licenses/; no single license expression asserted'}]}],
    'properties': [{'name': 'rust-microsip:scope', 'value': 'desktop build candidate plus selected workspace build inventory; not an ELF link map; release SBOM review pending'}],
}
inventory = json.loads(pathlib.Path('/out/cargo-licenses/inventory.json').read_text())
sbom['components'] += [{
    'type': 'library', 'name': p['name'], 'version': p['version'],
    'purl': f"pkg:cargo/{p['name']}@{p['version']}",
    'hashes': [{'alg': 'SHA-256', 'content': p['archive_sha256']}],
    'licenses': [{'expression': p.get('license_choice') or p['declared_license']}],
    'properties': [{'name': 'rust-microsip:scope', 'value': 'selected Linux normal/build/dev graph; not an ELF link map'}],
} for p in inventory['packages']]
pathlib.Path('/out/scaffold-sbom.cdx.json').write_text(json.dumps(sbom, indent=2) + '\n')
PY
(cd /out && find . -type f ! -name SHA256SUMS -print0 | LC_ALL=C sort -z | xargs -0 sha256sum > SHA256SUMS)
