//! Bounded SDP offer/answer for one audio stream: PCMU/PCMA, RFC 4733 events,
//! optional or mandatory SDES (`AES_CM_128_HMAC_SHA1_80`) and media direction.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ring::rand::{SecureRandom, SystemRandom};
use rust_microsip_core::MediaSecurity;
use rust_microsip_media::{Codec, srtp_adapter::MasterKey};
use std::net::{IpAddr, SocketAddr};
use zeroize::Zeroizing;

const SUITE: &str = "AES_CM_128_HMAC_SHA1_80";
/// Dynamic payload type offered for telephone-event.
pub(super) const EVENT_PT: u8 = 101;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(super) enum SdpError {
    #[error("unsupported session description")]
    Unsupported,
    #[error("secure media required but not offered")]
    InsecurePeer,
    #[error("plaintext media required but SRTP offered")]
    UnexpectedSecure,
    #[error("no common codec")]
    NoCodec,
    #[error("randomness unavailable")]
    Randomness,
}

/// RFC 3264 media direction.
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
    /// The answer direction for a remote offer direction.
    pub(super) fn answer(self) -> Self {
        match self {
            Self::SendOnly => Self::RecvOnly,
            Self::RecvOnly => Self::SendOnly,
            other => other,
        }
    }
    pub(super) fn sends(self) -> bool {
        matches!(self, Self::SendRecv | Self::SendOnly)
    }
    pub(super) fn receives(self) -> bool {
        matches!(self, Self::SendRecv | Self::RecvOnly)
    }
}

/// Fresh OS-random SDES master key and salt (30 bytes). Never logged.
pub(super) struct LocalKey(Zeroizing<[u8; 30]>);
impl LocalKey {
    pub(super) fn generate() -> Result<Self, SdpError> {
        let mut bytes = Zeroizing::new([0; 30]);
        SystemRandom::new()
            .fill(bytes.as_mut())
            .map_err(|_| SdpError::Randomness)?;
        Ok(Self(bytes))
    }
    pub(super) fn master(&self) -> Result<MasterKey, SdpError> {
        MasterKey::from_bytes(self.0.as_ref()).map_err(|_| SdpError::Unsupported)
    }
}

/// What this endpoint describes in an offer or answer.
pub(super) struct LocalDescription<'a> {
    pub address: SocketAddr,
    pub version: u64,
    pub codecs: &'a [Codec],
    pub event_pt: Option<u8>,
    /// SDES key and crypto tag; `None` describes plaintext RTP/AVP.
    pub key: Option<(&'a LocalKey, u32)>,
    pub flow: Flow,
}
impl LocalDescription<'_> {
    /// Render CRLF-terminated SDP. The buffer may contain key material.
    pub(super) fn render(&self) -> Zeroizing<Vec<u8>> {
        let family = if self.address.is_ipv4() { "IP4" } else { "IP6" };
        let ip = self.address.ip();
        let profile = if self.key.is_some() {
            "RTP/SAVP"
        } else {
            "RTP/AVP"
        };
        let mut formats: Vec<String> = self
            .codecs
            .iter()
            .map(|c| c.payload_type().to_string())
            .collect();
        if let Some(pt) = self.event_pt {
            formats.push(pt.to_string());
        }
        let mut text = Zeroizing::new(format!(
            "v=0\r\no=rust-microsip 1 {} IN {family} {ip}\r\ns=rust-microsip\r\nc=IN {family} {ip}\r\nt=0 0\r\nm=audio {} {profile} {}\r\n",
            self.version,
            self.address.port(),
            formats.join(" ")
        ));
        for codec in self.codecs {
            text.push_str(&format!(
                "a=rtpmap:{} {}/8000\r\n",
                codec.payload_type(),
                codec.name()
            ));
        }
        if let Some(pt) = self.event_pt {
            text.push_str(&format!(
                "a=rtpmap:{pt} telephone-event/8000\r\na=fmtp:{pt} 0-16\r\n"
            ));
        }
        if let Some((key, tag)) = self.key {
            let encoded = Zeroizing::new(STANDARD.encode(key.0.as_ref()));
            text.push_str(&format!(
                "a=crypto:{tag} {SUITE} inline:{}\r\n",
                encoded.as_str()
            ));
        }
        text.push_str(&format!("a=ptime:20\r\na={}\r\n", self.flow.attribute()));
        Zeroizing::new(text.as_bytes().to_vec())
    }
}

/// Parsed remote audio description.
pub(super) struct RemoteDescription {
    pub rtp: SocketAddr,
    /// Static G.711 payloads in the peer's preference order.
    pub codecs: Vec<Codec>,
    pub event_pt: Option<u8>,
    /// Chosen SDES crypto tag and key, when the profile is RTP/SAVP.
    pub crypto: Option<(u32, MasterKey)>,
    pub flow: Flow,
    /// Hash-free identity of the chosen key line, to detect a peer rekey.
    pub key_line: Zeroizing<String>,
}
impl RemoteDescription {
    /// The first peer codec we support, in the peer's order.
    pub(super) fn codec(&self) -> Result<Codec, SdpError> {
        self.codecs.first().copied().ok_or(SdpError::NoCodec)
    }
}

fn crypto_line(value: &str) -> Option<(u32, MasterKey, Zeroizing<String>)> {
    let mut fields = value.split(' ');
    let tag: u32 = fields
        .next()?
        .parse()
        .ok()
        .filter(|t| *t > 0 && *t < 1_000_000_000)?;
    if !fields.next()?.eq_ignore_ascii_case(SUITE) {
        return None;
    }
    let params = fields.next()?;
    // Session parameters (UNENCRYPTED_SRTP, KDR, ...) are not supported.
    if fields.next().is_some() {
        return None;
    }
    let (method, rest) = params.split_once(':')?;
    if !method.eq_ignore_ascii_case("inline") {
        return None;
    }
    // Lifetime and MKI suffixes are not supported.
    if rest.len() != 40 || rest.contains('|') {
        return None;
    }
    let mut decoded = Zeroizing::new([0; 30]);
    if STANDARD.decode_slice(rest, decoded.as_mut()).ok()? != 30 {
        return None;
    }
    Some((
        tag,
        MasterKey::from_bytes(decoded.as_ref()).ok()?,
        Zeroizing::new(value.to_owned()),
    ))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Session,
    Audio,
    Other,
}

/// Parse an offer or answer against the account's media policy.
pub(super) fn parse(bytes: &[u8], policy: MediaSecurity) -> Result<RemoteDescription, SdpError> {
    let bad = SdpError::Unsupported;
    if bytes.is_empty() || bytes.len() > 16 * 1024 {
        return Err(bad);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| bad)?;
    let (mut session_ip, mut media_ip) = (None, None);
    let mut media: Option<(u16, bool, Vec<u8>)> = None;
    // Which section the current line belongs to: session, the chosen audio
    // stream, or another media stream whose attributes are ignored.
    let mut section = Section::Session;
    let mut session_flow = Flow::SendRecv;
    let mut media_flow = None;
    let mut maps: Vec<(u8, String)> = Vec::new();
    let mut crypto = None;
    for (index, line) in text.lines().enumerate() {
        if index > 256 || line.len() > 1024 || line.bytes().any(|b| b.is_ascii_control()) {
            return Err(bad);
        }
        if let Some(value) = line.strip_prefix("m=") {
            if media.is_some() || !value.starts_with("audio ") {
                section = Section::Other;
                continue;
            }
            section = Section::Audio;
            let mut fields = value.split(' ').skip(1);
            let port: u16 = fields
                .next()
                .ok_or(bad)?
                .split('/')
                .next()
                .ok_or(bad)?
                .parse()
                .map_err(|_| bad)?;
            let secure = match fields.next().ok_or(bad)? {
                "RTP/AVP" => false,
                "RTP/SAVP" => true,
                _ => return Err(bad),
            };
            let formats: Vec<u8> = fields.filter_map(|f| f.parse().ok()).take(32).collect();
            media = Some((port, secure, formats));
            continue;
        }
        if section == Section::Other {
            continue;
        }
        if let Some(value) = line.strip_prefix("c=") {
            let mut fields = value.split(' ');
            if fields.next() != Some("IN") {
                return Err(bad);
            }
            let family = fields.next().ok_or(bad)?;
            let address = fields.next().ok_or(bad)?.split('/').next().ok_or(bad)?;
            let ip: IpAddr = address.parse().map_err(|_| bad)?;
            if (family == "IP4") != ip.is_ipv4() {
                return Err(bad);
            }
            if media.is_some() {
                media_ip = Some(ip);
            } else {
                session_ip = Some(ip);
            }
        } else if let Some(value) = line.strip_prefix("a=rtpmap:") {
            let (pt, name) = value.split_once(' ').ok_or(bad)?;
            if let Ok(pt) = pt.parse() {
                maps.push((pt, name.to_ascii_lowercase()));
            }
        } else if let Some(value) = line.strip_prefix("a=crypto:") {
            if crypto.is_none() && section == Section::Audio {
                crypto = crypto_line(value);
            }
        } else {
            let direction = match line {
                "a=sendrecv" => Some(Flow::SendRecv),
                "a=sendonly" => Some(Flow::SendOnly),
                "a=recvonly" => Some(Flow::RecvOnly),
                "a=inactive" => Some(Flow::Inactive),
                _ => None,
            };
            match (direction, section) {
                (Some(found), Section::Audio) => media_flow = Some(found),
                (Some(found), Section::Session) => session_flow = found,
                _ => (),
            }
        }
    }
    let (port, secure, formats) = media.ok_or(bad)?;
    let ip = media_ip.or(session_ip).ok_or(bad)?;
    let mut flow = media_flow.unwrap_or(session_flow);
    // Never send media to group or broadcast addresses.
    let group = match ip {
        IpAddr::V4(v4) => v4.is_multicast() || v4.is_broadcast(),
        IpAddr::V6(v6) => v6.is_multicast(),
    };
    if group {
        return Err(bad);
    }
    match (policy, secure) {
        (MediaSecurity::Srtp, false) => return Err(SdpError::InsecurePeer),
        (MediaSecurity::PlainRtp, true) => return Err(SdpError::UnexpectedSecure),
        _ => (),
    }
    let crypto = if secure {
        Some(crypto.ok_or(SdpError::InsecurePeer)?)
    } else {
        None
    };
    if port == 0 || ip.is_unspecified() {
        // A zero port or a 0.0.0.0 (RFC 2543) hold: nothing may be sent.
        flow = match flow {
            Flow::SendRecv | Flow::SendOnly if port != 0 => Flow::SendOnly,
            _ => Flow::Inactive,
        };
    }
    let codecs = formats
        .iter()
        .filter_map(|pt| match pt {
            0 => Some(Codec::Pcmu),
            8 => Some(Codec::Pcma),
            _ => None,
        })
        .collect();
    let event_pt = formats.iter().copied().find(|pt| {
        (96..=127).contains(pt)
            && maps
                .iter()
                .any(|(mapped, name)| mapped == pt && name.starts_with("telephone-event/8000"))
    });
    let (crypto, key_line) = match crypto {
        Some((tag, key, line)) => (Some((tag, key)), line),
        None => (None, Zeroizing::new(String::new())),
    };
    Ok(RemoteDescription {
        rtp: SocketAddr::new(ip, port),
        codecs,
        event_pt,
        crypto,
        flow,
        key_line,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> LocalKey {
        LocalKey(Zeroizing::new([0x52; 30]))
    }
    fn offer(secure: bool, flow: Flow) -> Zeroizing<Vec<u8>> {
        let key = key();
        LocalDescription {
            address: "192.0.2.10:4000".parse().unwrap_or_else(|_| unreachable!()),
            version: 1,
            codecs: &[Codec::Pcmu, Codec::Pcma],
            event_pt: Some(EVENT_PT),
            key: secure.then_some((&key, 1)),
            flow,
        }
        .render()
    }

    #[test]
    fn round_trips_plain_and_secure_offers() -> Result<(), SdpError> {
        let plain = parse(&offer(false, Flow::SendRecv), MediaSecurity::PlainRtp)?;
        assert_eq!(
            plain.rtp,
            "192.0.2.10:4000"
                .parse()
                .map_err(|_| SdpError::Unsupported)?
        );
        assert_eq!(plain.codecs, vec![Codec::Pcmu, Codec::Pcma]);
        assert_eq!(plain.event_pt, Some(101));
        assert!(plain.crypto.is_none());
        let secure = parse(&offer(true, Flow::SendOnly), MediaSecurity::Srtp)?;
        assert_eq!(secure.crypto.as_ref().map(|c| c.0), Some(1));
        assert_eq!(secure.flow, Flow::SendOnly);
        assert_eq!(secure.flow.answer(), Flow::RecvOnly);
        Ok(())
    }

    #[test]
    fn enforces_media_policy_without_downgrade() {
        assert_eq!(
            parse(&offer(false, Flow::SendRecv), MediaSecurity::Srtp).err(),
            Some(SdpError::InsecurePeer)
        );
        assert_eq!(
            parse(&offer(true, Flow::SendRecv), MediaSecurity::PlainRtp).err(),
            Some(SdpError::UnexpectedSecure)
        );
        let no_key = String::from_utf8(offer(true, Flow::SendRecv).to_vec())
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.starts_with("a=crypto"))
            .map(|l| format!("{l}\r\n"))
            .collect::<String>();
        assert_eq!(
            parse(no_key.as_bytes(), MediaSecurity::Srtp).err(),
            Some(SdpError::InsecurePeer)
        );
    }

    #[test]
    fn rejects_unsupported_key_parameters_and_suites() {
        let text = String::from_utf8(offer(true, Flow::SendRecv).to_vec()).unwrap_or_default();
        for (old, new) in [
            (SUITE, "AES_CM_128_HMAC_SHA1_32"),
            ("\r\na=ptime", "|2^20\r\na=ptime"),
            ("\r\na=ptime", " UNENCRYPTED_SRTP\r\na=ptime"),
        ] {
            let changed = text.replacen(old, new, 1);
            assert!(
                parse(changed.as_bytes(), MediaSecurity::Srtp).is_err(),
                "{new}"
            );
        }
    }

    #[test]
    fn hold_forms_and_codec_selection() -> Result<(), SdpError> {
        let text = String::from_utf8(offer(false, Flow::SendRecv).to_vec()).unwrap_or_default();
        let zero = text.replace("c=IN IP4 192.0.2.10\r\nt", "c=IN IP4 0.0.0.0\r\nt");
        assert_eq!(
            parse(zero.as_bytes(), MediaSecurity::PlainRtp)?.flow,
            Flow::SendOnly
        );
        let port_zero = text.replace("m=audio 4000 ", "m=audio 0 ");
        assert_eq!(
            parse(port_zero.as_bytes(), MediaSecurity::PlainRtp)?.flow,
            Flow::Inactive
        );
        let only_g729 = text.replace("RTP/AVP 0 8 101", "RTP/AVP 18 101");
        assert_eq!(
            parse(only_g729.as_bytes(), MediaSecurity::PlainRtp)?
                .codec()
                .err(),
            Some(SdpError::NoCodec)
        );
        let pcma_first = text.replace("RTP/AVP 0 8 101", "RTP/AVP 8 0 101");
        assert_eq!(
            parse(pcma_first.as_bytes(), MediaSecurity::PlainRtp)?.codec()?,
            Codec::Pcma
        );
        Ok(())
    }

    #[test]
    fn video_before_audio_does_not_hide_audio_attributes_and_groups_are_rejected()
    -> Result<(), SdpError> {
        let text = String::from_utf8(offer(true, Flow::SendOnly).to_vec()).unwrap_or_default();
        let (head, audio) = text.split_once("m=audio").unwrap_or_default();
        let reordered = format!("{head}m=video 5000 RTP/SAVP 96\r\na=inactive\r\nm=audio{audio}");
        let parsed = parse(reordered.as_bytes(), MediaSecurity::Srtp)?;
        assert_eq!(parsed.flow, Flow::SendOnly);
        assert!(parsed.crypto.is_some());
        let session_level = text
            .replace("a=sendonly\r\n", "")
            .replace("t=0 0\r\n", "t=0 0\r\na=inactive\r\n");
        assert_eq!(
            parse(session_level.as_bytes(), MediaSecurity::Srtp)?.flow,
            Flow::Inactive
        );
        for group in ["224.0.0.1", "255.255.255.255"] {
            let changed = text.replace("192.0.2.10", group);
            assert!(
                parse(changed.as_bytes(), MediaSecurity::Srtp).is_err(),
                "{group}"
            );
        }
        Ok(())
    }

    #[test]
    fn bounded_fuzzing_never_panics() {
        // Deterministic xorshift mutations of real offers plus random lines.
        let mut state = 0xD1B5_4A32_D192_ED03_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let seeds = [
            offer(false, Flow::SendRecv).to_vec(),
            offer(true, Flow::SendOnly).to_vec(),
        ];
        for round in 0..20_000 {
            let mut bytes = seeds[round % 2].clone();
            for _ in 0..1 + next() % 6 {
                let at = (next() as usize) % (bytes.len() + 1);
                match next() % 4 {
                    0 if at < bytes.len() => bytes[at] = next() as u8,
                    1 => bytes.insert(at, b" =:/\r\n0123456789"[(next() % 16) as usize]),
                    2 if at < bytes.len() => {
                        bytes.remove(at);
                    }
                    _ => bytes.truncate(at),
                }
            }
            for policy in [MediaSecurity::PlainRtp, MediaSecurity::Srtp] {
                let _ = parse(&bytes, policy);
            }
        }
    }

    #[test]
    fn rejects_malformed_and_oversized_input() {
        for bad in [
            &b""[..],
            b"v=0\r\n",
            b"v=0\r\nm=audio x RTP/AVP 0\r\nc=IN IP4 192.0.2.1\r\n",
        ] {
            assert!(parse(bad, MediaSecurity::PlainRtp).is_err());
        }
        assert!(parse(&vec![b'a'; 20_000], MediaSecurity::PlainRtp).is_err());
    }

    #[test]
    fn additional_media_lines_are_ignored_and_keys_are_fresh() -> Result<(), SdpError> {
        let text = String::from_utf8(offer(false, Flow::SendRecv).to_vec()).unwrap_or_default()
            + "m=video 5000 RTP/AVP 96\r\nc=IN IP4 198.51.100.1\r\na=sendonly\r\n";
        let parsed = parse(text.as_bytes(), MediaSecurity::PlainRtp)?;
        assert_eq!(parsed.flow, Flow::SendRecv);
        assert_eq!(parsed.rtp.ip().to_string(), "192.0.2.10");
        let (a, b) = (LocalKey::generate()?, LocalKey::generate()?);
        assert_ne!(a.0.as_ref(), b.0.as_ref());
        Ok(())
    }
}
