#!/usr/bin/env python3
"""Copy only explicit reviewed application inputs into an isolated candidate."""
if not __debug__:
    raise SystemExit('Optimized Python is not permitted for evidence verification')

from pathlib import Path
import hashlib, json, re, shutil, sys
root = Path(__file__).resolve().parents[2]
name = sys.argv[1] if len(sys.argv) == 2 else 'secure-call-work'
if len(sys.argv) > 2 or not re.fullmatch(r'[a-z0-9-]{1,64}', name):
    raise SystemExit('Expected one neutral artifact directory name')
if (root/'artifacts').is_symlink():
    raise SystemExit('Symlinked artifact root')
work = root / 'artifacts' / name
work.mkdir(parents=True, exist_ok=False)
inputs = json.loads((root/'docs/receipts/g0-srtp-adapter-2026-10-13.json').read_text())['application_build_inputs_sha256']
expected_paths = {
    '.dockerignore',
    'Cargo.lock',
    'Cargo.toml',
    'Dockerfile',
    'LICENSE',
    'NOTICE',
    'apps/rust-microsip/Cargo.toml',
    'apps/rust-microsip/src/lib.rs',
    'apps/rust-microsip/src/main.rs',
    'apps/rust-microsip/tests/cli.rs',
    'crates/media/Cargo.toml',
    'crates/media/src/dtmf.rs',
    'crates/media/src/lib.rs',
    'crates/media/tests/media.rs',
    'crates/telephony/Cargo.toml',
    'crates/telephony/examples/g0_calls.rs',
    'crates/telephony/examples/g0_controls.rs',
    'crates/telephony/examples/g0_hold.rs',
    'crates/telephony/examples/g0_registration.rs',
    'crates/telephony/examples/g0_transfer.rs',
    'crates/telephony/src/g0_calls.rs',
    'crates/telephony/src/g0_controls.rs',
    'crates/telephony/src/g0_hold.rs',
    'crates/telephony/src/g0_transfer.rs',
    'crates/telephony/src/lib.rs',
    'crates/telephony/tests/registration.rs',
    'docker/rust-components.sha256',
    'packaging/check-g0-dependencies.py',
    'packaging/collect-cargo-notices.py',
    'packaging/export-scaffold.sh',
    'packaging/tests/test_cargo_notices.py',
    'packaging/tests/test_g0_dependencies.py',
    'packaging/verify.sh',
    'rust-toolchain.toml',
}
if set(inputs) != expected_paths:
    raise SystemExit("Application input allowlist changed; review required")
for relative in inputs:
    source = root/relative
    if source.is_symlink() or not source.is_file() or any(parent.is_symlink() for parent in source.parents if parent.is_relative_to(root)):
        raise SystemExit('Expected a regular allowlisted input')
    payload=source.read_bytes()
    if hashlib.sha256(payload).hexdigest() != inputs[relative]:
        raise SystemExit('Application input differs from the reviewed baseline')
    target=work/relative; target.parent.mkdir(parents=True,exist_ok=True); target.write_bytes(payload)
probe_destinations = {'srtp_adapter.rs':'crates/media/src/srtp_adapter.rs','sdes.rs':'crates/telephony/src/sdes.rs','secure_call.rs':'crates/telephony/src/secure_call.rs','secure_call_example.rs':'crates/telephony/examples/secure_call.rs'}
pins = json.loads((root/'tests/probes/secure_call_inputs.json').read_text())
if set(pins['probe_files_sha256']) != set(probe_destinations):
    raise SystemExit('Unexpected probe input allowlist')
for name, destination in probe_destinations.items():
    source=root/'tests/probes'/name
    if source.is_symlink() or not source.is_file() or any(parent.is_symlink() for parent in source.parents if parent.is_relative_to(root)):
        raise SystemExit('Symlinked or missing probe input')
    payload=source.read_bytes()
    if hashlib.sha256(payload).hexdigest() != pins['probe_files_sha256'][name]:
        raise SystemExit('Probe source differs from the reviewed pin')
    (work/destination).write_bytes(payload)
if '[dependencies]' in (work/'crates/media/Cargo.toml').read_text():
    raise SystemExit('Unexpected media manifest shape')
with (work/'crates/media/Cargo.toml').open('a') as f:
    f.write('\n[dependencies]\nrustrtc = { version = "=0.4.7", default-features = false }\nthiserror = "=2.0.21"\nzeroize = "=1.9.1"\n')
with (work/'crates/media/src/lib.rs').open('a') as f: f.write('\n/// Isolated secure-packet candidate.\npub mod srtp_adapter;\n')
with (work/'crates/telephony/src/g0_calls.rs').open('a') as f: f.write('\n/// Isolated TLS/SDES call candidate.\n#[path = "secure_call.rs"]\npub mod secure;\n')
p=work/'crates/telephony/Cargo.toml'
if p.read_text().count('[dependencies]') != 1:
    raise SystemExit('Unexpected telephony manifest shape')
p.write_text(p.read_text().replace('[dependencies]','[dependencies]\nbase64 = { version = "=0.22.1", default-features = false, features = ["std"] }\nring = "=0.17.14"'))
# Preparation never compiles. Metadata and the exact graph are verified separately.
lock=root/'docs/inventory/g0-secure-call.Cargo.lock'
if lock.is_symlink() or any(parent.is_symlink() for parent in lock.parents if parent.is_relative_to(root)):
    raise SystemExit('Symlinked candidate baseline lock')
lock_bytes=lock.read_bytes()
if hashlib.sha256(lock_bytes).hexdigest()!=pins['baseline_candidate_lock_sha256']:
    raise SystemExit('Candidate baseline lock differs from its reviewed pin')
(work/'Cargo.lock').write_bytes(lock_bytes)
assert len(inputs)==34 and len(probe_destinations)==4
prepared={p:hashlib.sha256((work/p).read_bytes()).hexdigest() for p in sorted(set(inputs)|set(probe_destinations.values()))}
if prepared!=pins['candidate_inputs_sha256']:
    raise SystemExit('Post-modification input hashes differ from reviewed pins')
(work/'prepared-source-inputs.json').write_text(json.dumps(prepared,indent=2)+'\n')
print(f'Prepared {len(inputs)} explicit application inputs and {len(probe_destinations)} explicit probe inputs; no recursive repository copy.')
