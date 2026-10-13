"""Synthetic-call overlay identity/isolation controls, without Docker access."""
import copy
from pathlib import Path
import unittest
import verify_compose


def fixture():
    services = {}
    for name in ("pbx", "test-runner"):
        services[name] = {
            "read_only": True, "cap_drop": ["ALL"], "security_opt": ["no-new-privileges:true"],
            "user": "1000:1000", "networks": {"isolated": None},
            "volumes": [{"source": str((Path("artifacts/asterisk-lab") / ("pbx" if name == "pbx" else "clients")).resolve()),
                         "target": "/lab", "read_only": True, "bind": {"create_host_path": False}}],
        }
    services["pbx"]["environment"] = {"LAB_VERIFY_CONTACT": "1"}
    services["test-runner"].update({"image": "rust-microsip-g0-calls:0.1.0-lab.1",
                                   "build": {"target": "g0-calls-runner"}, "command": None})
    return {"services": services, "networks": {"isolated": {"internal": True}}}


class CallsComposeTests(unittest.TestCase):
    def test_accepts_normalized_reset_command_without_published_ports(self):
        verify_compose.verify(fixture(), "calls")

    def test_rejects_wrong_image_command_proof_mode_and_ports(self):
        for service, field, value in (("test-runner", "image", "wrong"),
                                      ("test-runner", "command", ["probe"]),
                                      ("pbx", "environment", {"LAB_VERIFY_CONTACT": "0"}),
                                      ("pbx", "ports", [{"target": 5060}])):
            candidate = copy.deepcopy(fixture())
            candidate["services"][service][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                verify_compose.verify(candidate, "calls")
