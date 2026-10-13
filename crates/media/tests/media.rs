//! Functional reference vectors and adversarial RTP boundaries.
use rust_microsip_media::{Codec, MediaError, RtpHeader, RtpPacket};
use sha2::{Digest, Sha256};

#[test]
fn every_i16_encoding_matches_independent_reference_hashes() {
    // Generated using CPython 3.12.3 audioop, ascending signed i16 samples.
    for (codec, expected) in [
        (
            Codec::Pcma,
            "38488f6fd710f4686360edc4d38639f96c491595ef93f8eb8d62d5e07ca6ce7b",
        ),
        (
            Codec::Pcmu,
            "81d633c9e6972a18c74a58720b96cb8ca0bdd096d4060b646dd708c3b846019a",
        ),
    ] {
        let mut hash = Sha256::new();
        for value in i16::MIN..=i16::MAX {
            hash.update([codec.encode_sample(value)]);
        }
        assert_eq!(
            hash.finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            expected
        );
    }
}

#[test]
fn every_octet_decoding_matches_independent_reference_hashes() {
    for (codec, expected) in [
        (
            Codec::Pcma,
            "e04788d110e58ff8c70c93b8480190d973e3b67876b6119abbaec766cc75c174",
        ),
        (
            Codec::Pcmu,
            "3dab54339e520bb2c924826e3b72a917a2b612e9fd12fc867500f1d983a75827",
        ),
    ] {
        let mut hash = Sha256::new();
        for value in u8::MIN..=u8::MAX {
            hash.update(codec.decode_sample(value).to_le_bytes());
        }
        assert_eq!(
            hash.finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            expected
        );
    }
}

#[test]
fn static_payloads_and_silence_are_explicit() {
    assert_eq!(Codec::Pcma.payload_type(), 8);
    assert_eq!(Codec::Pcmu.payload_type(), 0);
    assert_eq!(Codec::Pcma.encode_sample(0), 0xd5);
    assert_eq!(Codec::Pcmu.encode_sample(0), 0xff);
}

fn header() -> RtpHeader {
    RtpHeader {
        marker: true,
        payload_type: 8,
        sequence: 65535,
        timestamp: u32::MAX,
        ssrc: 0x12345678,
    }
}

#[test]
fn rtp_roundtrip_uses_caller_buffers() -> Result<(), MediaError> {
    let mut bytes = [0; 172];
    let length = header().write(&[0xd5; 160], &mut bytes)?;
    let parsed = RtpPacket::parse(&bytes[..length])?;
    assert_eq!(parsed.header, header());
    assert_eq!(parsed.payload, &[0xd5; 160]);
    Ok(())
}

#[test]
fn rtp_rejects_truncation_version_oversize_and_bad_padding() {
    for len in 0..12 {
        assert!(RtpPacket::parse(&[0; 12][..len]).is_err());
    }
    assert!(RtpPacket::parse(&[0x80; 1501]).is_err());
    for value in [0, 0x40, 0xc0, 0x8f, 0x90, 0xa0] {
        let mut bytes = [0; 12];
        bytes[0] = value;
        assert!(RtpPacket::parse(&bytes).is_err());
    }
    let mut bytes = [0; 16];
    bytes[0] = 0xa0;
    bytes[15] = 5;
    assert!(RtpPacket::parse(&bytes).is_err());
}

#[test]
fn rtp_skips_bounded_csrc_extension_and_padding() -> Result<(), MediaError> {
    let mut bytes = [0; 28];
    bytes[0] = 0xb1; // V2, padding, extension, one CSRC.
    bytes[1] = 8;
    bytes[18..20].copy_from_slice(&1u16.to_be_bytes()); // One extension word.
    bytes[24..26].copy_from_slice(&[0xd5, 0x55]);
    bytes[27] = 2;
    assert_eq!(RtpPacket::parse(&bytes)?.payload, &[0xd5, 0x55]);
    Ok(())
}

#[test]
fn rtp_encoder_refuses_small_output_and_invalid_payload_type() {
    assert!(header().write(&[0; 160], &mut [0; 171]).is_err());
    let mut h = header();
    h.payload_type = 128;
    assert!(h.write(&[0; 1], &mut [0; 20]).is_err());
}

#[test]
fn rtp_parser_never_panics_on_bounded_deterministic_noise() {
    let mut bytes = [0; 1500];
    let mut state = 0x12345678u32;
    for length in 0..=1500 {
        for b in &mut bytes[..length] {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            *b = (state >> 24) as u8;
        }
        let _ = RtpPacket::parse(&bytes[..length]);
    }
}
