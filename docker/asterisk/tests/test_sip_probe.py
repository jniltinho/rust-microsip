"""Boundaries for the registration-only lab fixture, not the Rust adapter."""
import unittest
import sip_probe


class ParserTests(unittest.TestCase):
    def test_parses_bounded_response(self):
        status, headers = sip_probe.parse_response(b'SIP/2.0 401 Unauthorized\r\nWWW-Authenticate: Digest realm="lab.invalid", nonce="abc", algorithm=MD5, qop="auth"\r\nContent-Length: 0\r\n\r\n')
        self.assertEqual(status, 401)
        self.assertIn("www-authenticate", headers)

    def test_rejects_malformed_and_oversized_response(self):
        for message in (b"bad", b"SIP/2.0 200 OK\r\nnot-a-header\r\n\r\n", b"a" * 65537):
            with self.assertRaises(sip_probe.ProbeError):
                sip_probe.parse_response(message)

    def test_digest_contains_no_password_and_rejects_unsupported_algorithm(self):
        challenge = 'Digest realm="lab.invalid", nonce="abc", algorithm=MD5, qop="auth"'
        auth = sip_probe.digest_authorization(challenge, "1001", "fictional-password", "sip:pbx", 1)
        self.assertNotIn("fictional-password", auth)
        self.assertIn('qop=auth', auth)
        with self.assertRaises(sip_probe.ProbeError):
            sip_probe.digest_authorization(challenge.replace("MD5", "SHA-unknown"), "1001", "secret", "sip:pbx", 1)

    def test_rejects_digest_header_injection(self):
        with self.assertRaises(sip_probe.ProbeError):
            sip_probe.digest_authorization('Digest realm="bad\r\nInjected", nonce="abc"', "1001", "secret", "sip:pbx", 1)


if __name__ == "__main__":
    unittest.main()
