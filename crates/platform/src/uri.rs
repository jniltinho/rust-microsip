//! Strict `sip:` / `tel:` link parsing (LIN-02).
//!
//! Parsing is a pure function: the input is data, never passed to a shell, and
//! the result is only a [`DialTarget`] that the UI must confirm before dialing.

use rust_microsip_core::DialTarget;

/// Maximum accepted length of a raw link, in bytes.
pub const MAX_URI_LEN: usize = 256;

/// Why an external link was rejected. Values are never echoed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum UriError {
    /// The link exceeds [`MAX_URI_LEN`].
    #[error("the link is too long")]
    TooLong,
    /// The link contains whitespace, control or non-ASCII characters.
    #[error("the link contains unsupported characters")]
    InvalidCharacters,
    /// The scheme is not `sip:` or `tel:`.
    #[error("only sip: and tel: links are supported")]
    UnsupportedScheme,
    /// URI parameters (`;...`) or headers (`?...`) are present.
    #[error("link parameters and headers are not supported")]
    UnsupportedParameters,
    /// A percent escape is malformed or decodes to a forbidden byte.
    #[error("the link has an invalid percent escape")]
    InvalidEncoding,
    /// The `sip:` host part is malformed.
    #[error("the link host is invalid")]
    InvalidHost,
    /// The user part does not map to a valid dial target.
    #[error("the link does not contain a valid number or user")]
    InvalidTarget,
}

/// Parse a `sip:user@host`, `sip:user` or `tel:+123` link into a dial target.
///
/// The scheme is case-insensitive. Only the user part is dialed, always through
/// the configured account; a `sip:` host is validated and then dropped so a
/// link can never redirect signaling to a foreign server. `tel:` visual
/// separators (`-`, `.`, `(`, `)`) are removed. Parameters, headers, passwords,
/// whitespace and control characters (including CR/LF, raw or percent-encoded)
/// are rejected.
///
/// # Errors
/// Returns the first [`UriError`] found.
pub fn parse_dial_uri(input: &str) -> Result<DialTarget, UriError> {
    if input.len() > MAX_URI_LEN {
        return Err(UriError::TooLong);
    }
    if !input.chars().all(|c| c.is_ascii_graphic()) {
        return Err(UriError::InvalidCharacters);
    }
    if input.contains([';', '?']) {
        return Err(UriError::UnsupportedParameters);
    }
    let (scheme, rest) = input.split_once(':').ok_or(UriError::UnsupportedScheme)?;
    let user = match scheme.to_ascii_lowercase().as_str() {
        "sip" => {
            let (user, host) = match rest.split_once('@') {
                Some((user, host)) => (user, Some(host)),
                None => (rest, None),
            };
            if let Some(host) = host {
                validate_host(host)?;
            }
            percent_decode(user)?
        }
        "tel" => percent_decode(rest)?
            .chars()
            .filter(|c| !matches!(c, '-' | '.' | '(' | ')'))
            .collect(),
        _ => return Err(UriError::UnsupportedScheme),
    };
    DialTarget::parse(&user).map_err(|_| UriError::InvalidTarget)
}

/// Single-pass percent decoding restricted to printable ASCII results.
fn percent_decode(value: &str) -> Result<String, UriError> {
    let bytes = value.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let byte = if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3).ok_or(UriError::InvalidEncoding)?;
            let hex = std::str::from_utf8(hex).map_err(|_| UriError::InvalidEncoding)?;
            if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(UriError::InvalidEncoding);
            }
            i += 3;
            u8::from_str_radix(hex, 16).map_err(|_| UriError::InvalidEncoding)?
        } else {
            i += 1;
            bytes[i - 1]
        };
        if !byte.is_ascii_graphic() {
            return Err(UriError::InvalidEncoding);
        }
        out.push(char::from(byte));
    }
    Ok(out)
}

/// Accept `name`, `name:port`, `[v6]` or `[v6]:port` with conservative characters.
fn validate_host(host: &str) -> Result<(), UriError> {
    let (name, port) = if let Some(v6) = host.strip_prefix('[') {
        let (addr, tail) = v6.split_once(']').ok_or(UriError::InvalidHost)?;
        if addr.is_empty()
            || !addr
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
        {
            return Err(UriError::InvalidHost);
        }
        match tail {
            "" => (addr, None),
            _ => (
                addr,
                Some(tail.strip_prefix(':').ok_or(UriError::InvalidHost)?),
            ),
        }
    } else {
        let (name, port) = match host.split_once(':') {
            Some((name, port)) => (name, Some(port)),
            None => (host, None),
        };
        let labels_ok = name
            .split('.')
            .all(|l| !l.is_empty() && !l.starts_with('-') && !l.ends_with('-'));
        if !labels_ok
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-.".contains(c))
        {
            return Err(UriError::InvalidHost);
        }
        (name, port)
    };
    if name.len() > 253 {
        return Err(UriError::InvalidHost);
    }
    if let Some(port) = port {
        match port.parse::<u16>() {
            Ok(p) if p > 0 && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit()) => {}
            _ => return Err(UriError::InvalidHost),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(input: &str) -> String {
        match parse_dial_uri(input) {
            Ok(t) => t.as_str().to_owned(),
            Err(e) => panic!("{input:?} rejected: {e:?}"),
        }
    }

    #[test]
    fn accepts_legitimate_links() {
        for (input, want) in [
            ("sip:1002@pbx.example.test", "1002"),
            ("sip:1002", "1002"),
            ("SIP:1002@pbx.example.test:5061", "1002"),
            ("sip:alice.smith@[2001:db8::1]:5060", "alice.smith"),
            ("sip:%2A97@10.0.0.1", "*97"),
            ("tel:+5511999990000", "+5511999990000"),
            ("tel:+1-555-(010).0199", "+15550100199"),
            ("Tel:%2B123", "+123"),
        ] {
            assert_eq!(ok(input), want, "{input}");
        }
    }

    #[test]
    fn rejects_malicious_and_malformed_links() {
        use UriError::*;
        let long = format!("sip:{}", "1".repeat(MAX_URI_LEN));
        let cases: &[(&str, UriError)] = &[
            (&long, TooLong),
            ("sip:1002\r\nVia: evil", InvalidCharacters),
            ("sip:1002\n", InvalidCharacters),
            ("sip:10 02", InvalidCharacters),
            ("sip:1002\u{7f}", InvalidCharacters),
            ("sip:10\u{e9}2", InvalidCharacters),
            ("sip:1002%0d%0aVia:x", InvalidEncoding),
            ("sip:1002%00", InvalidEncoding),
            ("sip:1002%20", InvalidEncoding),
            ("sip:1002%c3%a9", InvalidEncoding),
            ("sip:1002%2", InvalidEncoding),
            ("sip:1002%zz", InvalidEncoding),
            ("sip:1002?Subject=x", UnsupportedParameters),
            ("sip:1002@h?Route=%3Csip:evil%3E", UnsupportedParameters),
            ("sip:1002@h;transport=tcp", UnsupportedParameters),
            ("tel:+123;phone-context=x", UnsupportedParameters),
            ("sips:1002@h", UnsupportedScheme),
            ("http://evil", UnsupportedScheme),
            ("1002", UnsupportedScheme),
            ("file:/etc/passwd", UnsupportedScheme),
            ("sip:1002@-bad", InvalidHost),
            ("sip:1002@h:0", InvalidHost),
            ("sip:1002@h:99999", InvalidHost),
            ("sip:1002@h:+1", InvalidHost),
            ("sip:1002@a..b", InvalidHost),
            ("sip:1002@[zz]", InvalidHost),
            ("sip:1002@h_x", InvalidHost),
            ("sip:a@b@c", InvalidHost),
            ("sip:", InvalidTarget),
            ("tel:", InvalidTarget),
            ("sip:@pbx", InvalidTarget),
            ("sip://1002", InvalidTarget),
            ("sip:user:secret@pbx", InvalidTarget),
            ("tel:1+2", InvalidTarget),
            ("sip:%25", InvalidTarget),
            ("sip:%2525", InvalidTarget),
        ];
        for (input, want) in cases {
            assert_eq!(parse_dial_uri(input).err(), Some(*want), "{input:?}");
        }
    }

    #[test]
    fn shell_metacharacters_are_data_and_rejected() {
        for input in [
            "sip:$(reboot)@h",
            "sip:`id`",
            "sip:1002|sh",
            "sip:1002&&rm",
            "tel:1;rm",
            "sip:1002>out",
            "sip:%24%28id%29",
            "sip:1002%3Bid",
            "sip:'1002'",
            "sip:\"1002\"",
            "sip:..%2F..%2Fetc",
        ] {
            assert!(parse_dial_uri(input).is_err(), "{input:?}");
        }
    }

    #[test]
    fn bounded_fuzz_never_panics() {
        // Deterministic pseudo-random strings over a hostile alphabet.
        let alphabet: Vec<char> = "sip:tel%+@[]:;?&|$`'\"\\\r\n\t 0aZ#*-._/()\u{0}\u{e9}"
            .chars()
            .collect();
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
        for _ in 0..20_000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let len = (state % 40) as usize;
            let mut s = String::from(if state & 1 == 0 { "sip:" } else { "tel:" });
            let mut x = state;
            for _ in 0..len {
                x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                s.push(alphabet[(x >> 33) as usize % alphabet.len()]);
            }
            if let Ok(target) = parse_dial_uri(&s) {
                let t = target.as_str();
                assert!(
                    t.len() <= 64
                        && t.chars()
                            .all(|c| c.is_ascii_alphanumeric() || "*#+-_.".contains(c))
                );
            }
        }
    }
}
