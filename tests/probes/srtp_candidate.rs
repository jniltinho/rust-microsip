//! Isolated pre-adoption probe, deliberately outside the application workspace.
//!
//! Copy into the inventoried candidate media crate as described in the receipt.
//! The replay test must fail for unadapted rustrtc 0.4.7. Do not ignore it or
//! interpret the other passing checks as approval of secure media.

use rustrtc::errors::SrtpError;
use rustrtc::rtp::{RtpHeader, RtpPacket};
use rustrtc::srtp::{SrtpContext, SrtpDirection, SrtpKeyingMaterial, SrtpPacket, SrtpProfile};

type TestResult = Result<(), Box<dyn std::error::Error>>;
const SSRC: u32 = 0x1020_3040;

fn context(
    direction: SrtpDirection,
    key_byte: u8,
) -> Result<SrtpContext, Box<dyn std::error::Error>> {
    // Deterministic fictional test material, never runtime SDES key generation.
    Ok(SrtpContext::new(
        SSRC,
        SrtpProfile::Aes128Sha1_80,
        SrtpKeyingMaterial::new(vec![key_byte; 16], vec![0x22; 14]),
        direction,
    )?)
}

fn protect(sender: &mut SrtpContext, sequence: u16) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let packet = RtpPacket {
        header: RtpHeader::new(8, sequence, u32::from(sequence) * 160, SSRC),
        payload: vec![0x55; 160].into(),
        padding_len: 0,
    };
    let mut protected = vec![0; sender.protected_rtp_len(&packet)];
    sender.protect(&packet, &mut protected)?;
    Ok(protected)
}

#[test]
fn candidate_roundtrip_encrypts_and_adds_eighty_bit_tag() -> TestResult {
    let mut sender = context(SrtpDirection::Sender, 0x11)?;
    let mut receiver = context(SrtpDirection::Receiver, 0x11)?;
    let protected = protect(&mut sender, 10)?;
    assert_eq!(protected.len(), 12 + 160 + 10);
    assert_ne!(&protected[12..172], &[0x55; 160]);
    let clear = receiver.unprotect(SrtpPacket::parse(protected.as_slice().into())?)?;
    assert_eq!(&clear.payload[..], &[0x55; 160]);
    Ok(())
}

#[test]
fn candidate_rejects_modified_payload_without_poisoning_valid_packet() -> TestResult {
    let mut sender = context(SrtpDirection::Sender, 0x11)?;
    let mut receiver = context(SrtpDirection::Receiver, 0x11)?;
    let protected = protect(&mut sender, 10)?;
    let mut tampered = protected.clone();
    tampered[12] ^= 1;
    assert!(matches!(
        receiver.unprotect(SrtpPacket::parse(tampered.as_slice().into())?),
        Err(SrtpError::AuthenticationFailed)
    ));
    assert!(
        receiver
            .unprotect(SrtpPacket::parse(protected.as_slice().into())?)
            .is_ok()
    );
    Ok(())
}

#[test]
fn candidate_rejects_wrong_key() -> TestResult {
    let mut sender = context(SrtpDirection::Sender, 0x11)?;
    let mut receiver = context(SrtpDirection::Receiver, 0x33)?;
    let protected = protect(&mut sender, 10)?;
    assert!(matches!(
        receiver.unprotect(SrtpPacket::parse(protected.as_slice().into())?),
        Err(SrtpError::AuthenticationFailed)
    ));
    Ok(())
}

#[test]
fn candidate_handles_authenticated_reordering_across_rollover() -> TestResult {
    let mut sender = context(SrtpDirection::Sender, 0x11)?;
    let mut receiver = context(SrtpDirection::Receiver, 0x11)?;
    let initial = protect(&mut sender, 65_534)?;
    let late = protect(&mut sender, 65_535)?;
    let rollover = protect(&mut sender, 0)?;
    for protected in [initial, rollover, late] {
        let clear = receiver.unprotect(SrtpPacket::parse(protected.as_slice().into())?)?;
        assert_eq!(&clear.payload[..], &[0x55; 160]);
    }
    Ok(())
}

#[test]
fn candidate_must_reject_duplicate_authenticated_packet() -> TestResult {
    let mut sender = context(SrtpDirection::Sender, 0x11)?;
    let mut receiver = context(SrtpDirection::Receiver, 0x11)?;
    let protected = protect(&mut sender, 10)?;
    receiver.unprotect(SrtpPacket::parse(protected.as_slice().into())?)?;
    assert!(
        receiver
            .unprotect(SrtpPacket::parse(protected.as_slice().into())?)
            .is_err(),
        "Unadapted candidate accepted a duplicate authenticated packet; secure-media acceptance is blocked"
    );
    Ok(())
}
