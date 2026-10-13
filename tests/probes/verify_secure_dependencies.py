#!/usr/bin/env python3
"""Verify exact selected graph and byte identity of the isolated registry sources.

Run after candidate metadata/tree/notice collection and before compiling.
Archive checks are bounded identity checks, not a whole-source or crypto audit.
"""
if not __debug__:
    raise SystemExit('Optimized Python is not permitted for evidence verification')

from pathlib import Path
import hashlib
import io
import json
import re
import sys
import os
import tarfile

root = Path(__file__).resolve().parents[2]
name = sys.argv[1] if len(sys.argv) == 2 else 'secure-call-work'
if len(sys.argv) > 2 or not re.fullmatch(r'[a-z0-9-]{1,64}', name):
    raise SystemExit('Expected one neutral artifact directory name')
work = root/'artifacts'/name
cache = root/'artifacts/dependency-research/cargo-home/registry'
prepared=json.loads((work/'prepared-source-inputs.json').read_text())
assert len(prepared)==38
assert prepared==json.loads((root/'tests/probes/secure_call_inputs.json').read_text())['candidate_inputs_sha256']
# Cargo cannot discover extra build scripts, examples or configuration.
allowed=set(prepared)|{'prepared-source-inputs.json','secure-metadata.json','secure-tree.txt'}
ignored={'target','secure-notices','secure-notices-exact'}
actual=set()
for parent,dirs,files in os.walk(work,followlinks=False):
    for child in dirs:
        assert not (Path(parent)/child).is_symlink()
    if Path(parent)==work:
        dirs[:]=[d for d in dirs if d not in ignored]
    for file in files:
        path=Path(parent)/file
        assert not path.is_symlink()
        actual.add(str(path.relative_to(work)))
assert actual==allowed, 'Unexpected or missing candidate input/configuration'
for config in ('config','config.toml'):
    assert not (root/'artifacts/dependency-research/cargo-home'/config).exists()
for relative,digest in prepared.items():
    path=Path(relative)
    assert not path.is_absolute() and '..' not in path.parts
    assert hashlib.sha256((work/path).read_bytes()).hexdigest()==digest, relative

old = json.loads((root/'docs/inventory/g0-srtp-candidate-cargo.json').read_text())['packages']
new = json.loads((work/'secure-notices-exact/inventory.json').read_text())['packages']
by = {(p['name'], p['version']): p for p in old}
features = {}
for line in (work/'secure-tree.txt').read_text().splitlines():
    if not line.strip():
        continue
    match = re.fullmatch(r'([\w-]+) v([^ ]+)(?: \([^)]*\))?\|([^ ]*)(?: \(\*\))?', line)
    if not match:
        raise ValueError('Unsupported target-tree row')
    if match[1].startswith('rust-microsip'):
        continue
    features.setdefault((match[1], match[2]), set()).update(filter(None, match[3].split(',')))
assert len(new) == len(features) == len(by) == 100
assert {(p["name"],p["version"]) for p in new}==set(features)==set(by)
count = 0
for package in new:
    identity = package['name'], package['version']
    expected = by[identity]
    for key in ('archive_sha256', 'notices_sha256', 'features', 'declared_license'):
        assert package[key] == expected[key], (identity, key)
    assert sorted(features[identity]) == expected['target_tree_features'], identity
    prefix = '-'.join(identity)
    archives = list((cache/'cache').glob('*/'+prefix+'.crate'))
    assert len(archives) == 1 and not archives[0].is_symlink()
    assert archives[0].stat().st_size <= 16*1024*1024
    data = archives[0].read_bytes()
    assert hashlib.sha256(data).hexdigest() == expected['archive_sha256']
    directories = list((cache/'src').glob(prefix)) + list((cache/'src').glob('*/'+prefix))
    assert len(directories) == 1 and not directories[0].is_symlink()
    source = directories[0]
    members = set()
    size = 0
    with tarfile.open(fileobj=io.BytesIO(data), mode='r|gz') as archive:
        for index, member in enumerate(archive):
            size += member.size
            assert index < 10000 and 0 <= member.size <= 16*1024*1024 and size <= 64*1024*1024
            parts = member.name.split('/')
            assert parts[0] == prefix and not any(p in {'', '.', '..'} for p in parts)
            assert not member.issym() and not member.islnk()
            if not member.isfile():
                continue
            relative = '/'.join(parts[1:])
            assert relative and relative not in members
            members.add(relative)
            path = source/relative
            assert path.is_file() and not path.is_symlink()
            assert path.read_bytes() == archive.extractfile(member).read(), (identity, relative)
    actual = {str(p.relative_to(source)) for p in source.rglob('*') if p.is_file()}
    assert actual-members <= {'.cargo-ok', '.cargo-checksum.json'}
    count += len(members)
assert sum(len(p['notices_sha256']) for p in new) == 195
print(json.dumps({'selected_registry_packages':100,'preserved_notices':195,'verified_archive_source_files':count,'metadata_and_actual_target_features_match':True}))
