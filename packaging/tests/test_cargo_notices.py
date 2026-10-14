"""Adversarial archive and source checks for notice preservation."""
import hashlib
import importlib.util
import io
import json
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


class VendoredTests(unittest.TestCase):
    """Path-patched copies are accepted only as pinned in VENDORED."""

    def setup_root(self, directory, apache=None):
        root = Path(directory) / "repo"
        crate = root / "vendor" / "wgpu-hal"
        crate.mkdir(parents=True)
        (crate / "Cargo.toml").write_text("")
        real = Path(__file__).parents[2] / "vendor" / "wgpu-hal"
        (crate / "LICENSE.APACHE").write_bytes(apache if apache is not None else (real / "LICENSE.APACHE").read_bytes())
        (crate / "LICENSE.MIT").write_bytes((real / "LICENSE.MIT").read_bytes())
        package = {"id": "w", "name": "wgpu-hal", "version": "30.0.1", "source": None,
                   "license": "MIT OR Apache-2.0", "manifest_path": str(crate / "Cargo.toml")}
        metadata = {"workspace_members": ["own"], "packages": [package],
                    "resolve": {"nodes": [{"id": "w", "features": []}]}}
        return root, metadata

    def test_accepts_the_pinned_vendored_crate(self):
        with tempfile.TemporaryDirectory() as directory:
            root, metadata = self.setup_root(directory)
            report = collector.collect(metadata, {"package": []}, Path(directory), Path(directory) / "out",
                                       vendor_root=root)
            self.assertEqual(report["packages"][0]["source"], "vendored:vendor/wgpu-hal")

    def test_rejects_changed_notice_or_other_location(self):
        with tempfile.TemporaryDirectory() as directory:
            root, metadata = self.setup_root(directory, apache=b"edited")
            with self.assertRaises(ValueError):
                collector.collect(metadata, {"package": []}, Path(directory), Path(directory) / "out",
                                  vendor_root=root)
        with tempfile.TemporaryDirectory() as directory:
            root, metadata = self.setup_root(directory)
            with self.assertRaises(ValueError):
                collector.collect(metadata, {"package": []}, Path(directory), Path(directory) / "out",
                                  vendor_root=root / "elsewhere")


def exception(checksum, expression="Apache-2.0", name="example", version="1.0.0"):
    return {(name, version): {"name": name, "version": version, "archive_sha256": checksum,
                              "declared_expression": expression, "chosen_license": "Apache-2.0",
                              "reason": "fictional review"}}


def single_package(name, version, license_expression, payload):
    checksum = hashlib.sha256(payload).hexdigest()
    package = {"id": "p", "name": name, "version": version, "source": collector.REGISTRY,
               "license": license_expression}
    metadata = {"packages": [package], "resolve": {"nodes": [{"id": "p", "features": []}]}}
    lock = {"package": [{"name": name, "version": version, "source": collector.REGISTRY, "checksum": checksum}]}
    return metadata, lock, checksum


class ReviewedExceptionTests(unittest.TestCase):
    def test_accepts_only_exact_reviewed_identity(self):
        reviewed = exception("a" * 64)
        collector.verify_license("example", "1.0.0", "Apache-2.0", {}, reviewed, "a" * 64)
        for name, version, expression, checksum in (
                ("other", "1.0.0", "Apache-2.0", "a" * 64), ("example", "1.0.1", "Apache-2.0", "a" * 64),
                ("example", "1.0.0", "Apache-2.0 OR GPL-2.0-only", "a" * 64),
                ("example", "1.0.0", "Apache-2.0", "b" * 64), ("example", "1.0.0", "Apache-2.0", None)):
            with self.subTest(name=name, version=version, expression=expression), self.assertRaises(ValueError):
                collector.verify_license(name, version, expression, {}, reviewed, checksum)

    def test_rejects_unknown_package_without_exception(self):
        with self.assertRaises(ValueError):
            collector.verify_license("unknown", "1.0.0", "Apache-2.0", {"LICENSE"}, {}, "a" * 64)

    def test_loader_rejects_ranges_extra_fields_and_duplicates(self):
        good = next(iter(exception("a" * 64).values()))
        for data in ([good, good], [dict(good, archive_sha256="*")], [dict(good, extra="x")], [dict(good, reason="")]):
            with self.subTest(data=data), tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / "reviewed.json"
                path.write_text(json.dumps(data))
                with self.assertRaises(ValueError):
                    collector.load_reviewed(path)


class SupplementalNoticeTests(unittest.TestCase):
    URL = "https://raw.githubusercontent.com/example/repo/" + "c" * 40 + "/LICENSE-MIT"

    def run_collect(self, archive_name, archive_checksum=None, notice=b"upstream text", recorded=None):
        payload, _ = archive(archive_name)
        metadata, lock, checksum = single_package("example", "1.0.0", "MIT", payload)
        entry = {"name": "example", "version": "1.0.0", "archive_sha256": archive_checksum or checksum,
                 "files": [{"path": "LICENSE-MIT", "url": self.URL,
                            "sha256": recorded or hashlib.sha256(b"upstream text").hexdigest()}]}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "registry").mkdir()
            (root / "registry/example-1.0.0.crate").write_bytes(payload)
            (root / "supplement/example-1.0.0").mkdir(parents=True)
            (root / "supplement/example-1.0.0/LICENSE-MIT").write_bytes(notice)
            return collector.collect(metadata, lock, root, root / "out", None, {},
                                     {("example", "1.0.0"): entry}, root / "supplement")

    def test_uses_pinned_upstream_notice_for_exact_archive(self):
        report = self.run_collect("example-1.0.0/README")
        self.assertEqual(report["packages"][0]["notice_sources"], ["upstream-pinned"])

    def test_rejects_archive_checksum_mismatch(self):
        with self.assertRaisesRegex(ValueError, "identity mismatch"):
            self.run_collect("example-1.0.0/README", archive_checksum="d" * 64)

    def test_rejects_notice_hash_mismatch(self):
        with self.assertRaisesRegex(ValueError, "hash mismatch"):
            self.run_collect("example-1.0.0/README", notice=b"tampered")

    def test_rejects_collision_with_packaged_notice(self):
        with self.assertRaisesRegex(ValueError, "collides"):
            self.run_collect("example-1.0.0/LICENSE-MIT")

    def test_missing_notices_still_rejected_without_supplement(self):
        payload, checksum = archive("example-1.0.0/README")
        with self.assertRaises(ValueError):
            collector.verified_notices(payload, checksum, "example-1.0.0")

    def test_loader_rejects_unpinned_url(self):
        for url in ("https://raw.githubusercontent.com/example/repo/main/LICENSE-MIT",
                    "https://example.invalid/" + "c" * 40 + "/LICENSE-MIT"):
            entry = [{"name": "e", "version": "1", "archive_sha256": "a" * 64,
                      "files": [{"path": "LICENSE-MIT", "url": url, "sha256": "b" * 64}]}]
            with self.subTest(url=url), tempfile.TemporaryDirectory() as directory:
                (Path(directory) / "manifest.json").write_text(json.dumps(entry))
                with self.assertRaises(ValueError):
                    collector.load_supplements(directory)


class SelectionTests(unittest.TestCase):
    def test_retains_nested_licenses_directory(self):
        payload, checksum = archive("example-1.0.0/protocols/LICENSES/LGPL-2.1-or-later.txt")
        self.assertIn("protocols/LICENSES/LGPL-2.1-or-later.txt",
                      collector.verified_notices(payload, checksum, "example-1.0.0"))

    def test_tree_excludes_metadata_only_and_rejects_unknown_identity(self):
        payload, _ = archive("example-1.0.0/LICENSE")
        metadata, lock, _ = single_package("example", "1.0.0", "MIT", payload)
        with tempfile.TemporaryDirectory() as directory:
            report = collector.collect(metadata, lock, Path(directory), Path(directory) / "out", set())
            with self.assertRaisesRegex(ValueError, "missing from metadata"):
                collector.collect(metadata, lock, Path(directory), Path(directory) / "out2", {("ghost", "1.0.0")})
        self.assertEqual(report["packages"], [])
        self.assertEqual(report["metadata_only_not_built"], [{"name": "example", "version": "1.0.0"}])

    def test_parses_cargo_tree_identities(self):
        tree = "own v0.1.0 (/work)|\n\nserde_derive v1.0.0 (proc-macro)|default\nring v0.17.14|alloc (*)\n"
        self.assertEqual(collector.selected_identities(tree),
                         {("own", "0.1.0"), ("serde_derive", "1.0.0"), ("ring", "0.17.14")})
        for bad in ("", "no-version|", "ring v0.17.14"):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                collector.selected_identities(bad)
