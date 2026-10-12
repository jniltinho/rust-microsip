"""Adversarial tests for the selected-build G0 dependency boundary."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("g0_dependencies", Path(__file__).parents[1] / "check-g0-dependencies.py")
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class DependencyBoundaryTests(unittest.TestCase):
    def test_accepts_zero_dependency_scaffold(self):
        checker.verify("rust-microsip v0.1.0-dev.0 (/work/apps/rust-microsip)|\n")

    def test_accepts_reviewed_ring_and_rustcrypto_data_plane(self):
        checker.verify("rustls v0.23.45|ring,tls12,std\nring v0.17.14|alloc\nrustrtc v0.4.7|\naes v0.9.3|\n")

    def test_blocks_static_and_dynamic_unreviewed_crypto_providers(self):
        for name in ("aws-lc-rs", "aws-lc-sys", "openssl-sys", "openssl-src", "boring-sys", "srtp-sys"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                checker.verify(f"{name} v1.0.0|\n")

    def test_blocks_unreviewed_codec_direct_or_transitive(self):
        for version in ("0.4.9", "0.5.0"):
            with self.subTest(version=version), self.assertRaises(ValueError):
                checker.verify(f"rustrtc v0.4.7|\naudio-codec v{version}|std\n")

    def test_blocks_rustls_default_or_aws_feature_unification(self):
        for features in ("default,ring", "ring,aws_lc_rs", "std"):
            with self.subTest(features=features), self.assertRaises(ValueError):
                checker.verify(f"rustls v0.23.45|{features}\nring v0.17.14|\n")

    def test_blocks_unreviewed_ring_or_rustls_versions(self):
        for line in ("ring v0.17.13|", "rustls v0.24.0|ring\nring v0.17.14|"):
            with self.subTest(line=line), self.assertRaises(ValueError):
                checker.verify(line)

    def test_blocks_implicit_default_font_bundle(self):
        for name in ("eframe", "egui", "epaint"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                checker.verify(f"{name} v0.36.2|default_fonts\n")
        with self.assertRaises(ValueError):
            checker.verify("epaint_default_fonts v0.36.2|\n")

    def test_rejects_empty_malformed_or_excessive_input(self):
        for tree in ("", "not cargo output", "bad|line|suffix", "a" * (1024 * 1024 + 1)):
            with self.subTest(size=len(tree)), self.assertRaises(ValueError):
                checker.verify(tree)

    def test_blocks_mixed_zbus_runtime_or_rustrtc_defaults(self):
        for line in ("zbus v5.19.0|tokio,async-io", "rustrtc v0.4.7|std", "rustrtc v0.4.7|dtls", "rustrtc v0.5.0|", "rustrtc v0.4.7|upnp"):
            with self.subTest(line=line), self.assertRaises(ValueError):
                checker.verify(line)


if __name__ == "__main__":
    unittest.main()
