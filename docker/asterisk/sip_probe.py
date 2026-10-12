"""Bounded REGISTER fixture for lab readiness, NOT a production SIP stack.

No call/media features are simulated here. The G0 Rust adapter must separately
prove dialogs, secure media and every call scenario against this real PBX.
"""
import hashlib
import json
from pathlib import Path
import re
import secrets
import signal
import socket
import ssl
import time

import lab

MAX_MESSAGE = 65536


class ProbeError(ValueError):
    """Deliberately payload-free failure (no messages, auth or passwords)."""


def parse_response(message):
    if len(message) > MAX_MESSAGE:
        raise ProbeError()
    try:
        head = message.split(b"\r\n\r\n", 1)[0].decode("ascii")
        first, *lines = head.split("\r\n")
        if not re.fullmatch(r"SIP/2\.0 [1-6][0-9]{2} [\x20-\x7e]*", first):
            raise ProbeError()
        headers = {}
        for line in lines:
            name, value = line.split(":", 1)
            if not re.fullmatch(r"[a-zA-Z0-9-]+", name):
                raise ProbeError()
            headers.setdefault(name.lower(), []).append(value.strip())
        return int(first.split()[1]), headers
    except (UnicodeError, ValueError) as error:
        raise ProbeError() from error


def digest_authorization(challenge, username, password, uri, count):
    if any(char in challenge for char in ("\r", "\n")) or not challenge.startswith("Digest "):
        raise ProbeError()
    fields = {}
    for item in re.finditer(r'(\w+)=(?:"([^"\r\n]*)"|([^,\s]+))', challenge[7:]):
        fields[item[1].lower()] = item[2] if item[2] is not None else item[3]
    if fields.get("algorithm", "MD5").upper() != "MD5" or not fields.get("nonce") or not fields.get("realm"):
        raise ProbeError()
    if fields.get("qop") not in (None, "auth"):
        raise ProbeError()
    # MD5 is solely the fictional PBX's SIP Digest protocol, not password storage.
    def md5(value):
        return hashlib.md5(value.encode("ascii"), usedforsecurity=False).hexdigest()
    nonce, realm = fields["nonce"], fields["realm"]
    ha1, ha2 = md5(f"{username}:{realm}:{password}"), md5(f"REGISTER:{uri}")
    result = f'Digest username="{username}", realm="{realm}", nonce="{nonce}", uri="{uri}", algorithm=MD5'
    if fields.get("qop"):
        cnonce, nc = secrets.token_hex(12), f"{count:08x}"
        response = md5(f"{ha1}:{nonce}:{nc}:{cnonce}:auth:{ha2}")
        result += f', qop=auth, nc={nc}, cnonce="{cnonce}"'
    else:
        response = md5(f"{ha1}:{nonce}:{ha2}")
    if "opaque" in fields:
        result += f', opaque="{fields["opaque"]}"'
    return result + f', response="{response}"'


class RegistrationClient:
    """One bounded fixture connection, without logging SIP payloads."""
    def __init__(self, username, password, protocol, host="pbx", port=None):
        self.username, self.password, self.protocol = username, password, protocol
        self.host, self.port = host, port or (5061 if protocol == "tls" else 5060)
        self.call_id, self.tag = secrets.token_hex(16), secrets.token_hex(8)
        self.sequence = 0
        self.buffer = b""
        self.sock = None
        self.registered = False

    def connect(self):
        socktype = socket.SOCK_DGRAM if self.protocol == "udp" else socket.SOCK_STREAM
        address = socket.getaddrinfo(self.host, self.port, socket.AF_INET, socktype)[0][4]
        connection = socket.socket(socket.AF_INET, socktype)
        connection.settimeout(4)
        try:
            connection.connect(address)
            if self.protocol == "tls":
                context = ssl.create_default_context(cafile="/lab/ca.crt")
                context.minimum_version = ssl.TLSVersion.TLSv1_2
                connection = context.wrap_socket(connection, server_hostname=self.host)
            self.sock = connection
        except BaseException:
            connection.close()
            raise

    def receive(self, deadline):
        if self.protocol == "udp":
            return self.sock.recv(MAX_MESSAGE + 1)
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise ProbeError()
            self.sock.settimeout(min(1.5, remaining))
            if b"\r\n\r\n" in self.buffer:
                head, body = self.buffer.split(b"\r\n\r\n", 1)
                _, headers = parse_response(head)
                try:
                    length = int(headers.get("content-length", ["0"])[0])
                except ValueError as error:
                    raise ProbeError() from error
                if length < 0 or len(head) + 4 + length > MAX_MESSAGE:
                    raise ProbeError()
                if len(body) >= length:
                    result = head + b"\r\n\r\n" + body[:length]
                    self.buffer = body[length:]
                    return result
            chunk = self.sock.recv(8192)
            if not chunk or len(self.buffer) + len(chunk) > MAX_MESSAGE:
                raise ProbeError()
            self.buffer += chunk

    def request(self, expires, authorization=None):
        self.sequence += 1
        ip, port = self.sock.getsockname()[:2]
        branch = "z9hG4bK" + secrets.token_hex(12)
        transport = self.protocol.upper()
        uri = f"sip:{self.host}:{self.port}"
        message = (f"REGISTER {uri} SIP/2.0\r\n"
                   f"Via: SIP/2.0/{transport} {ip}:{port};branch={branch};rport\r\n"
                   "Max-Forwards: 10\r\n"
                   f"From: <sip:{self.username}@{self.host}>;tag={self.tag}\r\n"
                   f"To: <sip:{self.username}@{self.host}>\r\nCall-ID: {self.call_id}\r\n"
                   f"CSeq: {self.sequence} REGISTER\r\n"
                   f"Contact: <sip:{self.username}@{ip}:{port};transport={self.protocol}>\r\n"
                   f"Expires: {expires}\r\nUser-Agent: Fictional-Registration-Fixture\r\n")
        if authorization:
            message += f"Authorization: {authorization}\r\n"
        message += "Content-Length: 0\r\n\r\n"
        wire = message.encode("ascii")
        deadline = time.monotonic() + 6
        attempts = 3 if self.protocol == "udp" else 1
        for _ in range(attempts):
            self.sock.sendall(wire)
            self.sock.settimeout(1.5)
            try:
                while time.monotonic() < deadline:
                    status, headers = parse_response(self.receive(deadline))
                    if headers.get("call-id") != [self.call_id] or headers.get("cseq") != [f"{self.sequence} REGISTER"]:
                        continue
                    if status >= 200:
                        return status, headers
            except socket.timeout:
                continue
        raise ProbeError()

    def register(self, expires=120, expected_success=True):
        if self.sock is None:
            self.connect()
        status, headers = self.request(expires)
        if status != 401 or "www-authenticate" not in headers:
            raise ProbeError()
        uri = f"sip:{self.host}:{self.port}"
        auth = digest_authorization(headers["www-authenticate"][0], self.username, self.password, uri, 1)
        status, _ = self.request(expires, auth)
        if expected_success:
            if status != 200:
                raise ProbeError()
            self.registered = expires > 0
        elif status not in (401, 403):
            raise ProbeError()
        return status

    def close(self):
        try:
            if self.registered:
                self.register(expires=0)
        finally:
            if self.sock:
                self.sock.close()
                self.sock = None


def reject_untrusted_tls(host="pbx", port=5061):
    """Verify this fixture itself does not silently trust an unknown lab CA."""
    context = ssl.create_default_context()
    with socket.create_connection((host, port), timeout=4) as connection:
        try:
            with context.wrap_socket(connection, server_hostname=host):
                pass
        except ssl.SSLCertVerificationError:
            return
    raise ProbeError()


def run(mode):
    accounts = lab.load_accounts()
    clients = []
    try:
        suffix = "2" if mode == "peer" else "1"
        receipt = []
        for prefix, protocol in (("100", "udp"), ("110", "tcp"), ("120", "tls")):
            name = prefix + suffix
            client = RegistrationClient(name, accounts[name], protocol)
            clients.append(client)
            client.register()
            receipt.append({"transport": protocol, "challenge": 401, "registration": 200})
        if mode == "peer":
            Path("/tmp/peer-ready").write_text("registered UDP TCP TLS\n")
            stopped = False
            def stop(_signal, _frame):
                nonlocal stopped
                stopped = True
            signal.signal(signal.SIGTERM, stop)
            signal.signal(signal.SIGINT, stop)
            renewal = time.monotonic() + 60
            while not stopped:
                time.sleep(0.2)
                if time.monotonic() >= renewal:
                    for client in clients:
                        client.register()
                    renewal = time.monotonic() + 60
        else:
            reject_untrusted_tls()
            for prefix, protocol in (("100", "udp"), ("110", "tcp"), ("120", "tls")):
                wrong = RegistrationClient(prefix + "1", "deliberately-wrong-fictional-password", protocol)
                try:
                    wrong.register(expected_success=False)
                finally:
                    wrong.close()
            print(json.dumps({"schema_version": 1, "fixture": "registration-only-not-rust-adapter",
                              "transports": receipt, "wrong_password_rejected": True,
                              "unknown_ca_rejected": True, "calls_tested": False, "g0_passed": False}))
    finally:
        failed = False
        for client in clients:
            try:
                client.close()
            except (OSError, ProbeError):
                failed = True
        if failed:
            raise ProbeError()
