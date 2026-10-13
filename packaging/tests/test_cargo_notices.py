"""Adversarial archive and source checks for notice preservation."""
import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("cargo_notices", Path(__file__).parents[1] / "collect-cargo-notices.py")
collector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(collector)


def archive(name, content=b"fictional complete notice", symlink=False):
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode="w:gz") as target:
        member = tarfile.TarInfo(name)
        if symlink:
            member.type = tarfile.SYMTYPE
            member.linkname = "/outside"
        else:
            member.size = len(content)
        target.addfile(member, None if symlink else io.BytesIO(content))
    payload = stream.getvalue()
    return payload, hashlib.sha256(payload).hexdigest()


class NoticeTests(unittest.TestCase):
    def test_rejects_counterfeit_srtp_archive_before_preserving_notices(self):
        payload, checksum = archive("rustrtc-0.4.7/LICENSE")
        package = {"id": "p", "name": "rustrtc", "version": "0.4.7",
                   "source": collector.REGISTRY, "license": "MIT"}
        metadata = {"packages": [package], "resolve": {"nodes": [{"id": "p", "features": []}]}}
        lock = {"package": [{"name": "rustrtc", "version": "0.4.7",
                             "source": collector.REGISTRY, "checksum": checksum}]}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "registry").mkdir()
            (root / "registry/rustrtc-0.4.7.crate").write_bytes(payload)
            output = root / "notices"
            with self.assertRaisesRegex(ValueError, "protected dependency identity mismatch"):
                collector.collect(metadata, lock, root, output)
            self.assertFalse(output.exists())

    def test_accepts_only_reviewed_legacy_license_with_both_notices(self):
        collector.verify_license("version_check", "0.9.5", "MIT/Apache-2.0",
                                 {"LICENSE-MIT", "LICENSE-APACHE"})

    def test_rejects_other_legacy_license_identities_or_missing_notice(self):
        for name, version, notices in (
                ("other", "0.9.5", {"LICENSE-MIT", "LICENSE-APACHE"}),
                ("version_check", "0.9.6", {"LICENSE-MIT", "LICENSE-APACHE"}),
                ("version_check", "0.9.5", {"LICENSE-MIT"}),
                ("version_check", "0.9.5", {"LICENSE-APACHE"})):
            with self.subTest(name=name, version=version, notices=notices), self.assertRaises(ValueError):
                collector.verify_license(name, version, "MIT/Apache-2.0", notices)

    def test_rejects_counterfeit_legacy_archive_before_license_exception(self):
        payload, checksum = archive("version_check-0.9.5/LICENSE-MIT")
        package = {"id": "p", "name": "version_check", "version": "0.9.5",
                   "source": collector.REGISTRY, "license": "MIT/Apache-2.0"}
        metadata = {"packages": [package], "resolve": {"nodes": [{"id": "p", "features": []}]}}
        lock = {"package": [{"name": "version_check", "version": "0.9.5",
                             "source": collector.REGISTRY, "checksum": checksum}]}
        with tempfile.TemporaryDirectory() as directory, self.assertRaisesRegex(
                ValueError, "protected dependency identity mismatch"):
            collector.collect(metadata, lock, Path(directory), Path(directory))

    def test_preserves_exact_nested_notice(self):
        payload, checksum = archive("example-1.0.0/nested/LICENSE-MIT")
        self.assertEqual(collector.verified_notices(payload, checksum, "example-1.0.0"),
                         {"nested/LICENSE-MIT": b"fictional complete notice"})

    def test_rejects_checksum_mismatch(self):
        payload, _ = archive("example-1.0.0/LICENSE")
        with self.assertRaises(ValueError):
            collector.verified_notices(payload, "0" * 64, "example-1.0.0")

    def test_rejects_path_escape_symlink_and_missing_notice(self):
        for name, symlink in (("example-1.0.0/../LICENSE", False),
                              ("example-1.0.0/LICENSE", True), ("example-1.0.0/README", False)):
            payload, checksum = archive(name, symlink=symlink)
            with self.subTest(name=name), self.assertRaises(ValueError):
                collector.verified_notices(payload, checksum, "example-1.0.0")

    def test_rejects_git_source_before_reading_archive(self):
        package = {"id": "p", "name": "ring", "version": "0.17.14", "source": "git+https://example.invalid"}
        metadata = {"packages": [package], "resolve": {"nodes": [{"id": "p", "features": []}]}}
        with tempfile.TemporaryDirectory() as directory, self.assertRaises(ValueError):
            collector.collect(metadata, {"package": []}, Path(directory), Path(directory))

    def test_rejects_path_patch_outside_own_workspace(self):
        package = {"id": "p", "name": "rsipstack", "version": "0.7.3", "source": None}
        metadata = {"workspace_members": ["own"], "packages": [package],
                    "resolve": {"nodes": [{"id": "p", "features": []}]}}
        with tempfile.TemporaryDirectory() as directory, self.assertRaises(ValueError):
            collector.collect(metadata, {"package": []}, Path(directory), Path(directory))
