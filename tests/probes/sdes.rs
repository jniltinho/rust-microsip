//! Strict initial SDES offer/answer subset for the isolated disposable PBX.
//! No transport-security decision is delegated to this parser.

use base64::{Engine, engine::general_purpose::STANDARD};
use ring::rand::{SecureRandom, SystemRandom};
use rust_microsip_media::{Codec, srtp_adapter::MasterKey};
use std::net::{IpAddr, SocketAddr};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(super) enum SdesError {
    #[error("unsupported secure-media description")]
    Description,
    #[error("secure-media randomness unavailable")]
    Randomness,
}

pub(super) struct LocalKey(Zeroizing<[u8; 30]>);
impl LocalKey {
    pub(super) fn generate() -> Result<Self, SdesError> {
        let mut bytes = Zeroizing::new([0; 30]);
        SystemRandom::new()
            .fill(bytes.as_mut())
            .map_err(|_| SdesError::Randomness)?;
        Ok(Self(bytes))
    }
    pub(super) fn into_master(self) -> Result<MasterKey, SdesError> {
        MasterKey::from_bytes(self.0.as_ref()).map_err(|_| SdesError::Description)
    }
    pub(super) fn description(
        &self,
        local: SocketAddr,
        codec: Codec,
        tag: u32,
        version: u32,
    ) -> Zeroizing<Vec<u8>> {
        self.described(local, codec, tag, version, Flow::SendRecv, false)
    }
    /// A keyed offer with an explicit direction and optional RFC 4733 PT 101.
    pub(super) fn described(
        &self,
        local: SocketAddr,
        codec: Codec,
        tag: u32,
        version: u32,
        flow: Flow,
        events: bool,
    ) -> Zeroizing<Vec<u8>> {
        let encoded = Zeroizing::new(STANDARD.encode(self.0.as_ref()));
        let family = if local.is_ipv4() { "IP4" } else { "IP6" };
        let pt = codec.payload_type();
        let (list, event_lines) = if events {
            (
                format!("{pt} 101"),
                "a=rtpmap:101 telephone-event/8000\r\na=fmtp:101 0-15\r\n",
            )
        } else {
            (pt.to_string(), "")
        };
        Zeroizing::new(format!("v=0\r\no=rust-microsip 1 {version} IN {family} {}\r\ns=G0 synthetic secure RTP\r\nc=IN {family} {}\r\nt=0 0\r\nm=audio {} RTP/SAVP {list}\r\na=rtpmap:{pt} {}/8000\r\n{event_lines}a=crypto:{tag} AES_CM_128_HMAC_SHA1_80 inline:{}\r\na=ptime:20\r\na={}\r\n",local.ip(),local.ip(),local.port(),codec.name(),encoded.as_str(),flow.attribute()).into_bytes())
    }
}

/// A negotiated media direction (RFC 3264).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Flow {
    SendRecv,
    SendOnly,
    RecvOnly,
    Inactive,
}
impl Flow {
    fn attribute(self) -> &'static str {
        match self {
            Self::SendRecv => "sendrecv",
            Self::SendOnly => "sendonly",
            Self::RecvOnly => "recvonly",
            Self::Inactive => "inactive",
        }
    }
}

/// Parse a keyed answer that may carry one direction attribute; everything
/// else must satisfy the strict [`parse`] subset.
pub(super) fn parse_directed(
    bytes: &[u8],
    codec: Codec,
    pbx: IpAddr,
    expected_tag: Option<u32>,
) -> Result<(Target, Flow), SdesError> {
    let text = std::str::from_utf8(bytes).map_err(|_| SdesError::Description)?;
    let mut flow = None;
    let mut rest = Zeroizing::new(String::with_capacity(bytes.len()));
    for line in text.split_inclusive("\r\n") {
        let value = match line {
            "a=sendrecv\r\n" => Some(Flow::SendRecv),
            "a=sendonly\r\n" => Some(Flow::SendOnly),
            "a=recvonly\r\n" => Some(Flow::RecvOnly),
            "a=inactive\r\n" => Some(Flow::Inactive),
            _ => None,
        };
        match value {
            Some(found) if flow.replace(found).is_some() => return Err(SdesError::Description),
            Some(_) => (),
            None => rest.push_str(line),
        }
    }
    Ok((
        parse(rest.as_bytes(), codec, pbx, expected_tag)?,
        flow.unwrap_or(Flow::SendRecv),
    ))
}

/// Require PT 101 listed and mapped to telephone-event/8000 in an answer.
pub(super) fn telephone_events(bytes: &[u8]) -> Result<(), SdesError> {
    let text = std::str::from_utf8(bytes).map_err(|_| SdesError::Description)?;
    let listed = text
        .lines()
        .find_map(|l| l.strip_prefix("m=audio "))
        .is_some_and(|m| m.split(' ').skip(2).any(|pt| pt == "101"));
    let mapped = text
        .lines()
        .filter(|l| l.starts_with("a=rtpmap:101 "))
        .count()
        == 1
        && text.lines().any(|l| {
            l.eq_ignore_ascii_case("a=rtpmap:101 telephone-event/8000")
                || l.eq_ignore_ascii_case("a=rtpmap:101 telephone-event/8000/1")
        });
    if listed && mapped {
        Ok(())
    } else {
        Err(SdesError::Description)
    }
}

/// Strip every key line and the secure profile: a key-free plaintext offer.
pub(super) fn plaintext(offer: &[u8]) -> Result<Zeroizing<String>, SdesError> {
    let text = std::str::from_utf8(offer).map_err(|_| SdesError::Description)?;
    Ok(Zeroizing::new(
        text.split_inclusive("\r\n")
            .filter(|l| !l.starts_with("a=crypto:"))
            .collect::<String>()
            .replace("RTP/SAVP", "RTP/AVP"),
    ))
}

pub(super) struct Target {
    pub(super) address: SocketAddr,
    pub(super) control: SocketAddr,
    pub(super) tag: u32,
    pub(super) key: MasterKey,
}

pub(super) fn parse(
    bytes: &[u8],
    codec: Codec,
    pbx: IpAddr,
    expected_tag: Option<u32>,
) -> Result<Target, SdesError> {
    let bad = SdesError::Description;
    if bytes.is_empty() || bytes.len() > 8192 || !bytes.ends_with(b"\r\n") {
        return Err(bad);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| bad)?;
    let mut address = None;
    let mut connection_seen = [false; 2];
    let mut port = None;
    let mut tag = None;
    let mut key = None;
    let mut mapping = false;
    let mut direction = false;
    let mut preamble = [false; 4];
    for (index, line) in text[..text.len() - 2].split("\r\n").enumerate() {
        if index >= 128
            || line.len() > 512
            || line.len() < 2
            || !line.bytes().all(|b| (32..=126).contains(&b))
            || line.as_bytes()[1] != b'='
        {
            return Err(bad);
        }
        if index == 0 && line != "v=0" {
            return Err(bad);
        }
        if let Some(field) = ["v=", "o=", "s=", "t="]
            .iter()
            .position(|prefix| line.starts_with(prefix))
        {
            if port.is_some() || preamble[field] || (field == 0 && line != "v=0") {
                return Err(bad);
            }
            preamble[field] = true;
        }
        if line.starts_with("k=")
            || line.starts_with("a=key-mgmt:")
            || line.starts_with("a=fingerprint:")
            || line.starts_with("a=ice-")
            || line.starts_with("a=candidate:")
            || line.starts_with("a=rtcp")
            || line.starts_with("a=group:")
            || line.starts_with("a=bundle-only")
        {
            return Err(bad);
        }
        if matches!(line, "a=sendonly" | "a=recvonly" | "a=inactive") {
            return Err(bad);
        }
        if line == "a=sendrecv" {
            if direction {
                return Err(bad);
            }
            direction = true;
        }
        if let Some(value) = line.strip_prefix("c=") {
            let scope = usize::from(port.is_some());
            if connection_seen[scope] {
                return Err(bad);
            }
            connection_seen[scope] = true;
            let fields: Vec<_> = value.split(' ').collect();
            if fields.len() != 3 || fields[0] != "IN" || !matches!(fields[1], "IP4" | "IP6") {
                return Err(bad);
            }
            let ip: IpAddr = fields[2].parse().map_err(|_| bad)?;
            if ip != pbx || (fields[1] == "IP4") != ip.is_ipv4() {
                return Err(bad);
            }
            address = Some(ip);
        }
        if let Some(value) = line.strip_prefix("m=") {
            let fields: Vec<_> = value.split(' ').collect();
            if port.is_some()
                || fields.len() < 4
                || fields.len() > 19
                || fields[0] != "audio"
                || fields[2] != "RTP/SAVP"
            {
                return Err(bad);
            }
            let parsed: u16 = fields[1].parse().map_err(|_| bad)?;
            if !(10000..=10018).contains(&parsed) || !parsed.is_multiple_of(2) {
                return Err(bad);
            }
            let mut seen = [false; 128];
            for pt in &fields[3..] {
                let pt: usize = pt.parse().map_err(|_| bad)?;
                if pt > 127 || seen[pt] {
                    return Err(bad);
                }
                seen[pt] = true;
            }
            if !seen[usize::from(codec.payload_type())] {
                return Err(bad);
            }
            port = Some(parsed);
        }
        if let Some(value) = line.strip_prefix("a=rtpmap:") {
            if port.is_none() {
                return Err(bad);
            }
            let (pt, name) = value.split_once(' ').ok_or(bad)?;
            if pt.parse::<u8>().map_err(|_| bad)? == codec.payload_type() {
                if mapping
                    || (!name.eq_ignore_ascii_case(&format!("{}/8000", codec.name()))
                        && !name.eq_ignore_ascii_case(&format!("{}/8000/1", codec.name())))
                {
                    return Err(bad);
                }
                mapping = true;
            }
        }
        if let Some(value) = line.strip_prefix("a=crypto:") {
            if port.is_none() || key.is_some() {
                return Err(bad);
            }
            let fields: Vec<_> = value.split(' ').collect();
            if fields.len() != 3
                || !fields[1].eq_ignore_ascii_case("AES_CM_128_HMAC_SHA1_80")
                || fields[0].len() > 9
                || fields[0].starts_with('0')
                || !fields[0].bytes().all(|b| b.is_ascii_digit())
            {
                return Err(bad);
            }
            let parsed: u32 = fields[0].parse().map_err(|_| bad)?;
            if expected_tag.is_some_and(|expected| parsed != expected) {
                return Err(bad);
            }
            let (method, encoded) = fields[2].split_once(':').ok_or(bad)?;
            if !method.eq_ignore_ascii_case("inline") || encoded.len() != 40 {
                return Err(bad);
            }
            let mut decoded = Zeroizing::new([0; 30]);
            if STANDARD
                .decode_slice(encoded, decoded.as_mut())
                .map_err(|_| bad)?
                != 30
            {
                return Err(bad);
            }
            key = Some(MasterKey::from_bytes(decoded.as_ref()).map_err(|_| bad)?);
            tag = Some(parsed);
        }
    }
    if !preamble.into_iter().all(|seen| seen) {
        return Err(bad);
    }
    Ok(Target {
        control: SocketAddr::new(address.ok_or(bad)?, port.ok_or(bad)? + 1),
        address: SocketAddr::new(address.ok_or(bad)?, port.ok_or(bad)?),
        tag: tag.ok_or(bad)?,
        key: key.ok_or(bad)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        LocalKey(Zeroizing::new([0x52; 30]))
            .description(
                "192.0.2.1:10000".parse().unwrap_or_else(|_| unreachable!()),
                Codec::Pcma,
                7,
                1,
            )
            .to_vec()
    }
    fn accepted(bytes: &[u8], tag: Option<u32>) -> bool {
        parse(
            bytes,
            Codec::Pcma,
            "192.0.2.1".parse().unwrap_or_else(|_| unreachable!()),
            tag,
        )
        .is_ok()
    }
    fn changed(old: &str, new: &str) -> Vec<u8> {
        String::from_utf8(fixture())
            .unwrap_or_default()
            .replace(old, new)
            .into_bytes()
    }
    #[test]
    fn accepts_bound_initial_offer_and_answer() -> Result<(), SdesError> {
        let target = parse(
            &fixture(),
            Codec::Pcma,
            "192.0.2.1".parse().map_err(|_| SdesError::Description)?,
            Some(7),
        )?;
        assert_eq!(target.tag, 7);
        assert_eq!(target.address.port(), 10000);
        assert!(accepted(&fixture(), None));
        assert!(!accepted(&fixture(), Some(8)));
        Ok(())
    }
    #[test]
    fn rejects_plaintext_downgrade() {
        assert!(!accepted(&changed("RTP/SAVP", "RTP/AVP"), None));
    }
    #[test]
    fn plaintext_reoffer_is_key_free_and_versioned_offers_differ() -> Result<(), SdesError> {
        let clear = plaintext(&fixture())?;
        assert!(
            !clear.contains("inline:") && !clear.contains("SAVP") && clear.contains("RTP/AVP 8")
        );
        assert!(!accepted(clear.as_bytes(), None));
        let key = LocalKey(Zeroizing::new([0x52; 30]));
        let address = "192.0.2.1:10000"
            .parse()
            .map_err(|_| SdesError::Description)?;
        let second = key.description(address, Codec::Pcma, 7, 3);
        assert!(std::str::from_utf8(&second).is_ok_and(|t| t.contains("o=rust-microsip 1 3 ")));
        Ok(())
    }
    #[test]
    fn requires_single_media_scoped_crypto() {
        let crypto = String::from_utf8(fixture())
            .unwrap_or_default()
            .lines()
            .find(|l| l.starts_with("a=crypto:"))
            .unwrap_or_default()
            .to_owned()
            + "\r\n";
        assert!(!accepted(&changed(&crypto, ""), None));
        assert!(!accepted(&changed(&crypto, &crypto.repeat(2)), None));
        assert!(!accepted(
            &changed("t=0 0\r\n", &format!("t=0 0\r\n{crypto}"))
                .into_iter()
                .collect::<Vec<_>>(),
            None
        ));
    }
    #[test]
    fn rejects_key_parameters_and_weak_profiles() {
        for replacement in [
            "AES_CM_128_HMAC_SHA1_32",
            "F8_128_HMAC_SHA1_80",
            "AES_CM_128_HMAC_SHA1_80 UNENCRYPTED_SRTP",
        ] {
            assert!(!accepted(
                &changed("AES_CM_128_HMAC_SHA1_80", replacement),
                None
            ));
        }
        for suffix in [
            "|2^20",
            "|1:4",
            ";inline:AAAA",
            " UNAUTHENTICATED_SRTP",
            " UNENCRYPTED_SRTCP",
        ] {
            assert!(!accepted(
                &changed("\r\na=ptime", &format!("{suffix}\r\na=ptime")),
                None
            ));
        }
    }
    #[test]
    fn rejects_invalid_tags_and_noncanonical_key() {
        for tag in ["0", "01", "-1", "1000000000", ""] {
            assert!(!accepted(
                &changed("a=crypto:7", &format!("a=crypto:{tag}")),
                None
            ));
        }
        let text = String::from_utf8(fixture()).unwrap_or_default();
        let key = text
            .split("inline:")
            .nth(1)
            .unwrap_or_default()
            .split('\r')
            .next()
            .unwrap_or_default();
        for invalid in [
            "!".repeat(40),
            "A".repeat(39),
            "A".repeat(44),
            "=".repeat(40),
        ] {
            assert!(!accepted(&changed(key, &invalid), None));
        }
    }
    #[test]
    fn rejects_unanchored_or_ambiguous_media() {
        for (old, new) in [
            ("c=IN IP4 192.0.2.1", "c=IN IP4 192.0.2.2"),
            ("c=IN IP4", "c=IN IP6"),
            ("10000 RTP", "9999 RTP"),
            ("10000 RTP", "10020 RTP"),
            ("RTP/SAVP 8", "RTP/SAVP 0"),
            ("RTP/SAVP 8", "RTP/SAVP 8 8"),
            ("PCMA/8000", "PCMU/8000"),
        ] {
            assert!(!accepted(&changed(old, new), None));
        }
        assert!(!accepted(
            &changed("a=sendrecv", "m=audio 10002 RTP/SAVP 8\r\na=sendrecv"),
            None
        ));
        assert!(!accepted(
            &changed(
                "c=IN IP4 192.0.2.1",
                "c=IN IP4 192.0.2.1\r\nc=IN IP4 192.0.2.1"
            ),
            None
        ));
    }
    #[test]
    fn rejects_unsupported_direction_and_keying_mechanisms() {
        for attr in [
            "a=inactive",
            "a=sendonly",
            "a=recvonly",
            "a=fingerprint:sha-256 AA",
            "a=ice-ufrag:test",
            "a=candidate:1",
            "a=rtcp-mux",
            "a=key-mgmt:test",
            "k=clear:test",
        ] {
            assert!(!accepted(&changed("a=sendrecv", attr), None));
        }
    }
    #[test]
    fn enforces_all_input_bounds_and_line_framing() {
        assert!(!accepted(&[], None));
        assert!(!accepted(&vec![b' '; 8193], None));
        assert!(!accepted(&changed("a=sendrecv", &"a".repeat(513)), None));
        assert!(!accepted(
            &changed("a=sendrecv", &"a=x\r\n".repeat(129)),
            None
        ));
        assert!(!accepted(&changed("\r\n", "\n"), None));
        for control in ["\0", "\t", "\r", "\n", "é"] {
            assert!(!accepted(
                &changed("a=sendrecv", &format!("a=x{control}")),
                None
            ));
        }
    }
    #[test]
    fn directed_and_event_offers_round_trip_and_reject_ambiguity() -> Result<(), SdesError> {
        let key = LocalKey(Zeroizing::new([0x52; 30]));
        let address = "192.0.2.1:10000"
            .parse()
            .map_err(|_| SdesError::Description)?;
        let pbx = "192.0.2.1".parse().map_err(|_| SdesError::Description)?;
        let held = key.described(address, Codec::Pcma, 7, 2, Flow::SendOnly, true);
        let (target, flow) = parse_directed(&held, Codec::Pcma, pbx, Some(7))?;
        assert_eq!((flow, target.address.port()), (Flow::SendOnly, 10000));
        telephone_events(&held)?;
        assert!(telephone_events(&fixture()).is_err());
        let twice = String::from_utf8(held.to_vec())
            .unwrap_or_default()
            .replace("a=sendonly\r\n", "a=sendonly\r\na=inactive\r\n");
        assert!(parse_directed(twice.as_bytes(), Codec::Pcma, pbx, Some(7)).is_err());
        let wrong = String::from_utf8(held.to_vec())
            .unwrap_or_default()
            .replace("telephone-event/8000", "telephone-event/16000");
        assert!(telephone_events(wrong.as_bytes()).is_err());
        // The strict parser still rejects a direction it was not asked to accept.
        assert!(parse(&held, Codec::Pcma, pbx, Some(7)).is_err());
        Ok(())
    }
    #[test]
    fn fresh_os_keys_are_not_the_same() -> Result<(), SdesError> {
        let a = LocalKey::generate()?;
        let b = LocalKey::generate()?;
        assert_ne!(a.0.as_ref(), b.0.as_ref());
        Ok(())
    }
}
