"""Check actual normalized Compose output, without printing secrets or paths."""
import json
import re
from pathlib import Path
import sys


def require(condition):
    if not condition:
        raise ValueError("unsafe Compose configuration")


def verify(config, profile):
    if profile not in ("internal", "host", "calls"):
        raise ValueError("unknown profile")
    secret_root = Path("artifacts/asterisk-lab").resolve()
    services = config["services"]
    if profile == "host":
        require(set(services) == {"pbx"})
    if profile in ("internal", "calls"):
        require(set(services) == ({"pbx", "peer", "test-runner"} if profile == "internal" else {"pbx", "test-runner"}))
        require(config["networks"]["isolated"]["internal"])
    if profile == "calls":
        require(services["pbx"]["environment"]["LAB_VERIFY_CONTACT"] == "1")
        require(services["test-runner"]["image"] == "rust-microsip-g0-calls:0.1.0-lab.1")
        require(services["test-runner"]["build"]["target"] == "g0-calls-runner")
        require(services["test-runner"].get("command") in (None, []))
    for name, service in services.items():
        require(service["read_only"] and service["cap_drop"] == ["ALL"])
        require("no-new-privileges:true" in service["security_opt"])
        require(not service.get("privileged") and not service.get("devices"))
        require(service.get("network_mode") != "host")
        require(re.fullmatch(r"[1-9][0-9]*:[0-9]+", service["user"]) is not None)
        require(not service.get("cap_add") and not service.get("volumes_from"))
        require(service.get("pid") != "host" and service.get("ipc") != "host")
        require(len(service["volumes"]) == 1)
        mount = service["volumes"][0]
        expected = secret_root / ("pbx" if name == "pbx" else "clients")
        require(Path(mount["source"]) == expected)
        require(mount["target"] == "/lab" and mount["read_only"])
        require(not mount["bind"]["create_host_path"])
        if profile in ("internal", "calls"):
            require(not service.get("ports") and set(service["networks"]) == {"isolated"})
    if profile == "host":
        pbx = services["pbx"]
        require(set(pbx["networks"]) == {"host-lab"})
        require(not config["networks"]["host-lab"].get("internal", False))
        require(pbx["environment"]["LAB_PROFILE"] == "host")
        ports = {(p["host_ip"], str(p["published"]), str(p["target"]), p["protocol"]) for p in pbx["ports"]}
        # Compose expands ranges into individual ports in its canonical JSON.
        expected = {("127.0.0.1", "15060", "5060", p) for p in ("tcp", "udp")}
        expected.add(("127.0.0.1", "15061", "5061", "tcp"))
        expected.update(("127.0.0.1", str(p), str(p), "udp") for p in range(10000, 10020))
        require(ports == expected)


if __name__ == "__main__":
    try:
        verify(json.load(sys.stdin), sys.argv[1])
    except (AssertionError, ValueError, KeyError, IndexError):
        print("Compose safety contract rejected", file=sys.stderr)
        sys.exit(1)
    print("Compose safety contract OK (not media/VM ACL proof)")
