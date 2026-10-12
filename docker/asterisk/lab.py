"""Infrastructure for a fictional, disposable Asterisk lab (never the app)."""
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import stat
import subprocess
import sys
import tempfile

ACCOUNTS = ("1001", "1002", "1101", "1102", "1201", "1202", "1301", "1302")
RUNTIME = Path("/run/asterisk")
CONFIG = RUNTIME / "config/asterisk.conf"
MODULES = (
    "res_sorcery_config", "res_sorcery_memory", "res_sorcery_astdb",
    "res_sorcery_memory_cache", "res_pjproject", "res_pjsip", "res_pjsip_authenticator_digest",
    "res_pjsip_outbound_authenticator_digest", "res_pjsip_endpoint_identifier_user",
    "res_pjsip_registrar", "res_pjsip_session",
    "res_pjsip_pubsub", "res_pjsip_sdp_rtp", "res_pjsip_caller_id",
    "res_pjsip_nat", "res_pjsip_refer", "res_pjsip_dtmf_info",
    "res_timing_timerfd", "res_rtp_asterisk", "res_srtp", "chan_pjsip", "pbx_config",
    "bridge_simple", "bridge_native_rtp", "codec_alaw", "codec_ulaw",
    "codec_resample", "format_pcm", "app_echo", "app_playtones", "app_read",
    "app_dial", "app_stack", "app_senddtmf", "func_channel", "func_timeout",
    "func_callerid", "func_logic",
)


def private_write(path, content):
    """Create a new private regular file, never following a leaf symlink."""
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
        handle.write(content)


def validate_empty_secret_directory(root):
    """Refuse overwrite, foreign ownership, symlinks and public permissions."""
    info = root.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid():
        raise ValueError("lab directory must be a real directory owned by this user")
    if stat.S_IMODE(info.st_mode) != 0o700 or any(root.iterdir()):
        raise ValueError("lab directory must be empty with mode 0700")


def generate(root):
    """Generate short-lived fictional credentials; no personal paths are read."""
    validate_empty_secret_directory(root)
    old_umask = os.umask(0o077)
    stage = Path(tempfile.mkdtemp(prefix=".generating-", dir=root))
    try:
        for name in ("issuer", "pbx", "clients"):
            (stage / name).mkdir(mode=0o700)
        passwords = {name: secrets.token_hex(24) for name in ACCOUNTS}
        for name in ("pbx", "clients"):
            private_write(stage / name / "credentials.json", json.dumps(passwords))
        issuer, pbx = stage / "issuer", stage / "pbx"
        commands = [
            ["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-sha256", "-days", "3",
             "-subj", "/CN=Disposable Rust MicroSIP Lab CA",
             "-addext", "basicConstraints=critical,CA:TRUE",
             "-addext", "keyUsage=critical,keyCertSign,cRLSign",
             "-keyout", str(issuer / "ca.key"), "-out", str(issuer / "ca.crt")],
            ["req", "-new", "-newkey", "rsa:2048", "-nodes", "-sha256",
             "-subj", "/CN=pbx", "-keyout", str(pbx / "pbx.key"), "-out", str(stage / "pbx.csr")],
        ]
        private_write(stage / "server.ext", "basicConstraints=critical,CA:FALSE\n"
                      "keyUsage=critical,digitalSignature,keyEncipherment\n"
                      "extendedKeyUsage=serverAuth\nsubjectAltName=DNS:pbx,DNS:localhost,IP:127.0.0.1\n")
        commands.append(["x509", "-req", "-sha256", "-days", "3", "-in", str(stage / "pbx.csr"),
                         "-CA", str(issuer / "ca.crt"), "-CAkey", str(issuer / "ca.key"),
                         "-set_serial", str(secrets.randbits(128) or 1),
                         "-extfile", str(stage / "server.ext"), "-out", str(pbx / "pbx.crt")])
        for command in commands:
            subprocess.run(["openssl", *command], check=True, stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL, timeout=30)
        for name in ("pbx", "clients"):
            shutil.copyfile(issuer / "ca.crt", stage / name / "ca.crt")
        for path in stage.rglob("*"):
            path.chmod(0o700 if path.is_dir() else 0o600)
        for name in ("issuer", "pbx", "clients"):
            (stage / name).rename(root / name)
    finally:
        shutil.rmtree(stage)
        os.umask(old_umask)


def load_accounts(root=Path("/lab")):
    """Read only explicitly mounted disposable credentials, not HOME or dotenv."""
    path = root / "credentials.json"
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or stat.S_IMODE(info.st_mode) != 0o600:
        raise ValueError("lab credentials must be a regular 0600 file")
    if info.st_uid != os.getuid() or info.st_size > 8192:
        raise ValueError("invalid lab credential ownership or size")
    accounts = json.loads(path.read_text(encoding="utf-8"))
    validate_accounts(accounts)
    return accounts


def validate_accounts(accounts):
    if set(accounts) != set(ACCOUNTS) or any(
        not isinstance(value, str) or not re.fullmatch(r"[a-zA-Z0-9_-]{16,128}", value)
        for value in accounts.values()
    ):
        raise ValueError("invalid fictional accounts")


def pjsip_config(accounts, profile):
    """Render separate transport/security fixtures; never weaken secure endpoints."""
    validate_accounts(accounts)
    if profile not in ("internal", "host"):
        raise ValueError("unsupported lab profile")
    config = "[global]\ntype=global\nuser_agent=Disposable-Lab\nendpoint_identifier_order=username\n\n"
    for protocol, port in (("udp", 5060), ("tcp", 5060), ("tls", 5061)):
        config += f"[transport-{protocol}]\ntype=transport\nprotocol={protocol}\nbind=0.0.0.0:{port}\n"
        if protocol == "tls":
            config += ("cert_file=/lab/pbx.crt\npriv_key_file=/lab/pbx.key\nca_list_file=/lab/ca.crt\n"
                       "method=tlsv1_2\nverify_client=no\nrequire_client_cert=no\n")
        if profile == "host":
            # Treat the Docker bridge gateway used by loopback DNAT as external.
            # local_net must NOT include the bridge subnet: otherwise SDP rewriting
            # is incorrectly bypassed for host connections.
            external_port = 15061 if protocol == "tls" else 15060
            config += ("local_net=127.0.0.0/8\nexternal_media_address=127.0.0.1\n"
                       f"external_signaling_address=127.0.0.1\nexternal_signaling_port={external_port}\n")
        config += "\n"
    for name in ACCOUNTS:
        protocol = "udp" if name.startswith("10") else "tcp" if name.startswith("11") else "tls"
        encryption = "sdes" if name.startswith("12") else "no"
        config += (f"[{name}]\ntype=endpoint\ntransport=transport-{protocol}\ncontext=lab-only\n"
                   f"auth=auth-{name}\naors={name}\ndisallow=all\nallow=alaw,ulaw\n"
                   f"media_encryption={encryption}\nmedia_encryption_optimistic=no\n"
                   "direct_media=no\nrtp_symmetric=yes\nforce_rport=yes\nrewrite_contact=yes\n"
                   "dtmf_mode=rfc4733\nice_support=no\nallow_transfer=yes\n\n"
                   f"[auth-{name}]\ntype=auth\nauth_type=userpass\nusername={name}\n"
                   f"password={accounts[name]}\nrealm=lab.invalid\n\n"
                   f"[{name}]\ntype=aor\nmax_contacts=1\nremove_existing=no\n"
                   "minimum_expiration=30\ndefault_expiration=120\nmaximum_expiration=300\n\n")
    return config


def dialplan():
    """Exact local destinations only, synthetic tones, no recording or PSTN."""
    config = ("[general]\nstatic=yes\nwriteprotect=yes\n\n[lab-only]\n"
              "exten => 600,1,Answer()\n same => n,Echo()\n same => n,Hangup()\n"
              "exten => 601,1,Answer()\n same => n,Read(DIGITS,,3,,1,8)\n"
              ' same => n,GotoIf($["${DIGITS}" = "123"]?accepted:rejected)\n'
              " same => n(accepted),PlayTones(1000)\n same => n,Wait(3)\n same => n,Hangup()\n"
              " same => n(rejected),PlayTones(400)\n same => n,Wait(3)\n same => n,Hangup()\n"
              "exten => 602,1,Answer()\n same => n,PlayTones(1000)\n"
              " same => n,Wait(20)\n same => n,Hangup()\n")
    for name in ACCOUNTS:
        config += f"exten => {name},1,Dial(PJSIP/{name},20)\n same => n,Hangup()\n"
    return config


def configure_pbx():
    """All mutable PBX state lives in an ephemeral, private tmpfs."""
    os.umask(0o077)
    for directory in ("config", "db", "cache", "spool", "log", "data"):
        (RUNTIME / directory).mkdir(mode=0o700, exist_ok=True)
    directories = {
        "astetcdir": RUNTIME / "config", "astmoddir": "/usr/lib/asterisk/modules",
        "astvarlibdir": RUNTIME / "data", "astdbdir": RUNTIME / "db",
        "astdatadir": "/var/lib/asterisk", "astkeydir": "/lab",
        "astcachedir": RUNTIME / "cache", "astspooldir": RUNTIME / "spool",
        "astrundir": RUNTIME, "astlogdir": RUNTIME / "log", "astsbindir": "/usr/sbin",
    }
    files = {
        "asterisk.conf": "[directories]\n" + "".join(f"{key} => {value}\n" for key, value in directories.items())
        + "\n[options]\nverbose=0\ndebug=0\nnocolor=yes\nmaxcalls=8\n",
        "modules.conf": "[modules]\nautoload=no\n" + "".join(f"load={name}.so\n" for name in MODULES),
        "pjsip.conf": pjsip_config(load_accounts(), os.environ.get("LAB_PROFILE", "internal")),
        "extensions.conf": dialplan(),
        "rtp.conf": "[general]\nrtpstart=10000\nrtpend=10019\nicesupport=no\nstrictrtp=yes\n",
        "manager.conf": "[general]\nenabled=no\n",
        "http.conf": "[general]\nenabled=no\n",
        "cdr.conf": "[general]\nenable=no\n",
        "cel.conf": "[general]\nenable=no\n",
        "logger.conf": "[general]\n\n[logfiles]\nconsole => error,warning\n",
        "acl.conf": "", "features.conf": "[general]\n", "ccss.conf": "[general]\ncc_max_requests=0\n",
        "udptl.conf": "[general]\nudptlstart=4000\nudptlend=4001\n",
        "stasis.conf": "[taskpool]\ninitial_size=5\nmax_size=16\n",
        "pjproject.conf": "[global]\ntype=global\nlog_level=0\n",
        "indications.conf": "[general]\ncountry=us\n[us]\ndescription=Disposable lab tones\n"
        "ringcadence=2000,4000\ndial=350+440\nbusy=480+620/500,0/500\n"
        "ring=440+480/2000,0/4000\ncongestion=480+620/250,0/250\n",
    }
    for name, content in files.items():
        private_write(RUNTIME / "config" / name, content)


def pbx_command(command):
    return subprocess.run(["asterisk", "-C", str(CONFIG), "-rx", command],
                          check=True, text=True, capture_output=True, timeout=5).stdout


def health():
    transports = pbx_command("pjsip show transports")
    endpoints = pbx_command("pjsip show endpoints")
    if not all(f"transport-{name}" in transports for name in ("udp", "tcp", "tls")):
        raise ValueError("PJSIP transports not ready")
    if not all(re.search(r"Endpoint:\s+" + name + r"(?:/|\s)", endpoints) for name in ACCOUNTS):
        raise ValueError("PJSIP endpoints not ready")
    srtp = pbx_command("module show like res_srtp.so")
    if not re.search(r"res_srtp\.so\s+.*Running", srtp):
        raise ValueError("SRTP module not ready")
    for route in ("600", "601", "602"):
        if f"'{route}'" not in pbx_command(f"dialplan show {route}@lab-only"):
            raise ValueError("dialplan not ready")


def main():
    command = sys.argv[1:]
    if command == ["generate"]:
        generate(Path("/lab"))
        print("Fictional credentials created; certificates expire in 3 days. Delete lab data after use.")
    elif command == ["pbx"]:
        configure_pbx()
        os.execv("/usr/sbin/asterisk", ["asterisk", "-f", "-C", str(CONFIG)])
    elif command == ["health"]:
        health()
    elif command == ["unit-test"]:
        os.execv(sys.executable, [sys.executable, "-B", "-m", "unittest", "discover", "-s", "/opt/lab/tests"])
    elif command in (["peer"], ["probe"]):
        import sip_probe
        sip_probe.run(command[0])
    else:
        raise ValueError("unsupported lab command")


if __name__ == "__main__":
    try:
        main()
    except Exception:
        # Never echo exception payloads: JSON, auth and OpenSSL can contain secrets.
        print("Lab operation failed; verify disposable paths, permissions and readiness.", file=sys.stderr)
        sys.exit(1)
