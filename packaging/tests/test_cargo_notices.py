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
