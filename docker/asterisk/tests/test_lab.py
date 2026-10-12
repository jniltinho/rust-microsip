"""Safety contracts for the disposable PBX, not application acceptance tests."""
import os
from pathlib import Path
import tempfile
import unittest

import lab


class ConfigurationTests(unittest.TestCase):
    def setUp(self):
        self.accounts = {name: "fictional-test-only" for name in lab.ACCOUNTS}

    def test_default_has_no_external_advertisement(self):
        config = lab.pjsip_config(self.accounts, "internal")
        self.assertNotIn("external_media_address", config)
        self.assertNotIn("local_net", config)
        self.assertIn("direct_media=no", config)
        self.assertNotIn("anonymous", config)
        self.assertEqual(config.count("type=endpoint"), 8)

    def test_secure_and_insecure_fixtures_are_distinct(self):
        config = lab.pjsip_config(self.accounts, "internal")
        self.assertIn("[1201]\ntype=endpoint", config)
        secure = config.split("[1201]\n")[1].split("[1202]\n")[0]
        self.assertIn("media_encryption=sdes", secure)
        self.assertIn("media_encryption_optimistic=no", secure)
        self.assertIn("transport=transport-tls", secure)
        rtp_only = config.split("[1301]\n")[1].split("[1302]\n")[0]
        self.assertIn("media_encryption=no", rtp_only)

    def test_host_advertises_only_loopback(self):
        config = lab.pjsip_config(self.accounts, "host")
        self.assertIn("external_media_address=127.0.0.1", config)
        self.assertIn("external_signaling_port=15061", config)
        self.assertNotIn("local_net=172.", config)

    def test_rejects_unknown_profile_and_secret_injection(self):
        with self.assertRaises(ValueError):
            lab.pjsip_config(self.accounts, "production")
        self.accounts["1001"] = "bad\n[anonymous]"
        with self.assertRaises(ValueError):
            lab.pjsip_config(self.accounts, "internal")

    def test_only_allowlisted_routes(self):
        config = lab.dialplan()
        self.assertIn("exten => 600,1,Answer()", config)
        self.assertIn("exten => 601,1,Answer()", config)
        self.assertIn("exten => 602,1,Answer()", config)
        self.assertNotIn("_X", config)
        self.assertNotIn("System(", config)
        self.assertNotIn("Record(", config)
        self.assertIn('GotoIf($["${DIGITS}" = "123"]?accepted:rejected)', config)


class SecretDirectoryTests(unittest.TestCase):
    def test_refuses_public_nonempty_or_symlink_directory(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            root.chmod(0o755)
            with self.assertRaises(ValueError):
                lab.validate_empty_secret_directory(root)
            root.chmod(0o700)
            (root / "keep").write_text("do-not-touch")
            with self.assertRaises(ValueError):
                lab.validate_empty_secret_directory(root)
            (root / "keep").unlink()
            link = root / "link"
            link.symlink_to(root, target_is_directory=True)
            with self.assertRaises(ValueError):
                lab.validate_empty_secret_directory(link)

    def test_generated_keys_stay_private_and_separated(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            lab.generate(root)
            self.assertTrue((root / "issuer/ca.key").is_file())
            self.assertFalse((root / "pbx/ca.key").exists())
            self.assertFalse((root / "clients/pbx.key").exists())
            for path in root.rglob("*"):
                expected = 0o700 if path.is_dir() else 0o600
                self.assertEqual(path.stat().st_mode & 0o777, expected)
                self.assertEqual(path.stat().st_uid, os.getuid())
            before = (root / "pbx/pbx.key").read_bytes()
            with self.assertRaises(ValueError):
                lab.generate(root)
            self.assertEqual(before, (root / "pbx/pbx.key").read_bytes())


if __name__ == "__main__":
    unittest.main()
