//! Experimental AES-CM/80 data-plane adapter, isolated from the application.
//!
//! This module runs only in the inventoried candidate workspace. It does not
//! implement TLS/SDES negotiation or establish that a secure PBX call works.

#![deny(missing_docs)]

use std::{
    fmt,
    time::{Duration, Instant},
};

use rustrtc::errors::SrtpError;
use rustrtc::rtp::RtpPacket as ProviderRtp;
use rustrtc::srtp::{
    SrtpContext, SrtpDirection, SrtpKeyingMaterial, SrtpPacket, SrtpProfile, SrtpSession,
};
use zeroize::Zeroizing;

/// A redacted, input-free secure-packet failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SecureError {
    /// SDES AES-CM key/salt material must be exactly thirty bytes.
    #[error("invalid secure-media key length")]
    InvalidKey,
    /// A malformed or oversized packet cannot enter the provider.
    #[error("invalid secure-media packet")]
    InvalidPacket,
    /// Packet authentication failed without advancing replay state.
    #[error("secure-media authentication failed")]
    Authentication,
    /// This index has already been received in the replay window.
    #[error("secure-media replay rejected")]
    Replay,
    /// This packet is older than the bounded replay window.
    #[error("secure-media packet is too old")]
    TooOld,
    /// A bounded receive-side key epoch cannot admit another source.
    #[error("secure-media source capacity reached")]
    SourceCapacity,
    /// Pre-authentication work exceeded the fixed monotonic-window budget.
    #[error("secure-media authentication rate limited")]
    RateLimited,
    /// Another SSRC cannot join this single-source receiver.
    #[error("unexpected secure-media source")]
    WrongSource,
    /// A transmit packet would reuse an index/keystream.
    #[error("secure-media transmit index reuse rejected")]
    IndexReuse,
    /// The key's finite packet-index space has been exhausted.
    #[error("secure-media key lifetime exhausted")]
    KeyExpired,
    /// An unexpected provider failure closed the sender.
    #[error("secure-media provider failed")]
    Provider,
    /// No transmission is permitted after a terminal provider/key failure.
    #[error("secure-media sender is closed")]
    Closed,
}

/// Owned, non-cloneable AES-CM master key and salt, with redacted Debug.
/// Own storage is zeroized on drop; this does not erase provider/caller copies.
///
/// ```compile_fail
/// use rust_microsip_media::srtp_adapter::MasterKey;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<MasterKey>();
/// ```
pub struct MasterKey(Zeroizing<[u8; 30]>);

impl MasterKey {
    /// Copy exactly thirty decoded SDES bytes; callers must protect their input.
    ///
    /// # Errors
    /// Returns [`SecureError::InvalidKey`] for every other length.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SecureError> {
        if bytes.len() != 30 {
            return Err(SecureError::InvalidKey);
        }
        let mut key = Zeroizing::new([0; 30]);
        key.copy_from_slice(bytes);
        Ok(Self(key))
    }

    fn provider_material(&self) -> SrtpKeyingMaterial {
        SrtpKeyingMaterial::new(self.0[..16].to_vec(), self.0[16..].to_vec())
    }
}

impl fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MasterKey([REDACTED])")
    }
}

/// Maximum clear RTP/RTCP datagram size admitted by this worker-only adapter.
pub const MAX_PLAINTEXT: usize = 1500;
const TAG: usize = 10;
const CONTROL_TRAILER: usize = 14;
const MAX_RTP_INDEX: u64 = (1_u64 << 48) - 1;
const MAX_CONTROL_INDEX: u32 = (1_u32 << 31) - 1;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct ReplayWindow {
    highest: Option<u64>,
    seen: u64,
}

impl ReplayWindow {
    // Propose without mutation; the caller installs this only after authentication.
    fn propose(self, index: u64) -> Result<Self, SecureError> {
        let Some(highest) = self.highest else {
            return Ok(Self {
                highest: Some(index),
                seen: 1,
            });
        };
        if index > highest {
            let shift = index - highest;
            let seen = if shift >= 64 {
                1
            } else {
                (self.seen << shift) | 1
            };
            return Ok(Self {
                highest: Some(index),
                seen,
            });
        }
        let age = highest - index;
        if age >= 64 {
            return Err(SecureError::TooOld);
        }
        let bit = 1_u64 << age;
        if self.seen & bit != 0 {
            return Err(SecureError::Replay);
        }
        Ok(Self {
            highest: self.highest,
            seen: self.seen | bit,
        })
    }
}

fn rtp_index(highest: Option<u64>, sequence: u16) -> Result<u64, SecureError> {
    let Some(highest) = highest else {
        return Ok(u64::from(sequence));
    };
    let roc = highest >> 16;
    let delta = i32::from(sequence) - i32::from(highest as u16);
    let guessed = if delta < -32768 {
        roc.checked_add(1)
            .filter(|v| *v <= u64::from(u32::MAX))
            .ok_or(SecureError::KeyExpired)?
    } else if delta > 32768 {
        roc.checked_sub(1).ok_or(SecureError::TooOld)?
    } else {
        roc
    };
    Ok((guessed << 16) | u64::from(sequence))
}

fn provider_error(error: SrtpError) -> SecureError {
    match error {
        SrtpError::AuthenticationFailed => SecureError::Authentication,
        SrtpError::UnsupportedProfile => SecureError::Provider,
        _ => SecureError::InvalidPacket,
    }
}

fn source_at(packet: &[u8], offset: usize) -> u32 {
    // Callers have already checked the corresponding fixed header length.
    u32::from_be_bytes([
        packet[offset],
        packet[offset + 1],
        packet[offset + 2],
        packet[offset + 3],
    ])
}

fn validate_control(packet: &[u8]) -> Result<(), SecureError> {
    if packet.len() < 8 || packet.len() > MAX_PLAINTEXT {
        return Err(SecureError::InvalidPacket);
    }
    let mut offset = 0;
    while offset < packet.len() {
        if packet.len() - offset < 4 {
            return Err(SecureError::InvalidPacket);
        }
        let first = packet[offset];
        let kind = packet[offset + 1];
        let length =
            (usize::from(u16::from_be_bytes([packet[offset + 2], packet[offset + 3]])) + 1) * 4;
        if first >> 6 != 2
            || !(192..=223).contains(&kind)
            || length > packet.len() - offset
            || (offset == 0 && length < 8)
        {
            return Err(SecureError::InvalidPacket);
        }
        if first & 0x20 != 0 {
            let padding = usize::from(packet[offset + length - 1]);
            if offset + length != packet.len() || padding == 0 || padding > length - 4 {
                return Err(SecureError::InvalidPacket);
            }
        }
        offset += length;
    }
    Ok(())
}

/// Single-source AES-CM/80 sender, deliberately non-cloneable.
///
/// Allocates bounded packet storage in a worker, never in an audio callback.
/// Consumes packet indices even if the caller later fails to send the ciphertext.
/// Canonicalizes RTP padding through the provider's packet serializer.
/// The caller must create a fresh key/session; reconstructing a sender from
/// the same raw bytes can still reuse a key, which this local object cannot detect.
///
/// ```compile_fail
/// use rust_microsip_media::srtp_adapter::SecureSender;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<SecureSender>();
/// ```
pub struct SecureSender {
    context: Option<SrtpContext>,
    ssrc: u32,
    highest: Option<u64>,
    control_count: u32,
}

impl fmt::Debug for SecureSender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecureSender([REDACTED])")
    }
}

impl SecureSender {
    /// Bind the outbound SSRC and consume the key; initialization fixes AES-CM/80.
    ///
    /// # Errors
    /// Returns [`SecureError::Provider`] if provider initialization fails.
    pub fn new(ssrc: u32, key: MasterKey) -> Result<Self, SecureError> {
        let context = SrtpContext::new(
            ssrc,
            SrtpProfile::Aes128Sha1_80,
            key.provider_material(),
            SrtpDirection::Sender,
        )
        .map_err(|_| SecureError::Provider)?;
        Ok(Self {
            context: Some(context),
            ssrc,
            highest: None,
            control_count: 0,
        })
    }

    fn ensure_lifetime(&mut self) -> Result<(), SecureError> {
        if self.context.is_none() {
            return Err(SecureError::Closed);
        }
        if self.highest == Some(MAX_RTP_INDEX) || self.control_count == MAX_CONTROL_INDEX {
            self.context = None;
            return Err(SecureError::KeyExpired);
        }
        Ok(())
    }

    /// Encrypt/authenticate RTP, rejecting malformed/oversized input and reused indices.
    ///
    /// # Errors
    /// Returns typed packet/source/index/lifetime errors without input values.
    /// Unexpected provider errors permanently close this sender; retry requires a new key.
    pub fn protect_rtp(&mut self, plain: &[u8]) -> Result<Vec<u8>, SecureError> {
        self.ensure_lifetime()?;
        let parsed = crate::RtpPacket::parse(plain).map_err(|_| SecureError::InvalidPacket)?;
        if parsed.header.ssrc != self.ssrc {
            return Err(SecureError::WrongSource);
        }
        let index = match rtp_index(self.highest, parsed.header.sequence) {
            Err(SecureError::TooOld) => return Err(SecureError::IndexReuse),
            Err(SecureError::KeyExpired) => {
                self.context = None;
                return Err(SecureError::KeyExpired);
            }
            other => other?,
        };
        if self.highest.is_some_and(|highest| index <= highest) {
            return Err(SecureError::IndexReuse);
        }
        let packet = ProviderRtp::parse(plain).map_err(|_| SecureError::InvalidPacket)?;
        let mut context = self.context.take().ok_or(SecureError::Closed)?;
        let mut output = vec![0; context.protected_rtp_len(&packet)];
        if context.protect(&packet, &mut output).is_err() {
            return Err(SecureError::Provider);
        }
        self.context = Some(context);
        self.highest = Some(index);
        Ok(output)
    }

    /// Encrypt/authenticate structurally bounded compound RTCP with an independent index.
    ///
    /// # Errors
    /// Returns typed packet/source/lifetime errors. No plaintext-control fallback exists.
    pub fn protect_rtcp(&mut self, plain: &[u8]) -> Result<Vec<u8>, SecureError> {
        self.ensure_lifetime()?;
        validate_control(plain)?;
        if source_at(plain, 4) != self.ssrc {
            return Err(SecureError::WrongSource);
        }
        let mut context = self.context.take().ok_or(SecureError::Closed)?;
        let mut output = plain.to_vec();
        if context.protect_rtcp(&mut output).is_err() {
            return Err(SecureError::Provider);
        }
        self.context = Some(context);
        self.control_count += 1;
        Ok(output)
    }
}

fn receiver_session(key: &MasterKey) -> Result<SrtpSession, SecureError> {
    // The public SRTCP receiver API lives on Session, not Context. Empty local
    // material makes accidental transmission fail closed; only receive is used.
    SrtpSession::new(
        SrtpProfile::Aes128Sha1_80,
        SrtpKeyingMaterial::new(Vec::new(), Vec::new()),
        key.provider_material(),
    )
    .map_err(|_| SecureError::Provider)
}

/// One authenticated remote SSRC, separate 64-packet SRTP and SRTCP replay windows.
///
/// No source is learned on failed authentication. A pending provider session is
/// discarded after a failed first packet, so unauthenticated SSRC churn cannot
/// grow its context map. Once bound, other SSRCs are rejected before provider use.
/// Allocates bounded worker storage; never use in a device callback or log bytes.
/// The caller still validates the packet's transport address and signaling policy.
///
/// ```compile_fail
/// use rust_microsip_media::srtp_adapter::SecureReceiver;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<SecureReceiver>();
/// ```
pub struct SecureReceiver {
    material: Option<MasterKey>,
    session: Option<SrtpSession>,
    ssrc: Option<u32>,
    rtp: ReplayWindow,
    rtcp: ReplayWindow,
    closed: bool,
}

impl fmt::Debug for SecureReceiver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecureReceiver([REDACTED])")
    }
}

impl SecureReceiver {
    /// Consume a remote source key; no outbound crypto context is initialized.
    ///
    /// # Errors
    /// Returns [`SecureError::Provider`] on provider initialization failure.
    pub fn new(key: MasterKey) -> Result<Self, SecureError> {
        let session = receiver_session(&key)?;
        Ok(Self {
            material: Some(key),
            session: Some(session),
            ssrc: None,
            rtp: ReplayWindow::default(),
            rtcp: ReplayWindow::default(),
            closed: false,
        })
    }

    /// Return the single authenticated source, never a provisional source.
    pub fn authenticated_ssrc(&self) -> Option<u32> {
        self.ssrc
    }

    fn check_source(&self, ssrc: u32) -> Result<(), SecureError> {
        if self.closed {
            return Err(SecureError::Closed);
        }
        if self.ssrc.is_some_and(|known| known != ssrc) {
            return Err(SecureError::WrongSource);
        }
        Ok(())
    }

    fn receive(
        &mut self,
        ssrc: u32,
        operation: impl FnOnce(&mut SrtpSession) -> Result<Vec<u8>, SecureError>,
    ) -> Result<Vec<u8>, SecureError> {
        self.check_source(ssrc)?;
        let mut session = match self.session.take() {
            Some(session) => session,
            None => receiver_session(self.material.as_ref().ok_or(SecureError::Closed)?)?,
        };
        match operation(&mut session) {
            Ok(clear) => {
                self.session = Some(session);
                self.ssrc = Some(ssrc);
                self.material = None;
                Ok(clear)
            }
            Err(error) => {
                if error == SecureError::Provider {
                    self.closed = true;
                    self.material = None;
                } else if self.ssrc.is_some() {
                    self.session = Some(session);
                }
                // An unbound failed session is dropped, not retained per fake SSRC.
                Err(error)
            }
        }
    }

    /// Authenticate/decrypt bounded RTP and commit replay state only on success.
    ///
    /// # Errors
    /// Returns specific [`SecureError::Replay`]/[`SecureError::TooOld`], source,
    /// packet, authentication or provider errors. Returned media must not be logged.
    /// ROC overflow returns [`SecureError::KeyExpired`]; a terminal provider
    /// failure makes subsequent valid input return [`SecureError::Closed`].
    pub fn unprotect_rtp(&mut self, wire: &[u8]) -> Result<Vec<u8>, SecureError> {
        if wire.len() < 12 + TAG || wire.len() > MAX_PLAINTEXT + TAG || wire[0] >> 6 != 2 {
            return Err(SecureError::InvalidPacket);
        }
        let ssrc = source_at(wire, 8);
        self.check_source(ssrc)?;
        let sequence = u16::from_be_bytes([wire[2], wire[3]]);
        let index = rtp_index(self.rtp.highest, sequence)?;
        let next = self.rtp.propose(index)?;
        let clear = self.receive(ssrc, |session| {
            let packet = SrtpPacket::parse(wire.into()).map_err(|_| SecureError::InvalidPacket)?;
            let packet = session.unprotect_rtp(packet).map_err(provider_error)?;
            let clear = packet.marshal().map_err(|_| SecureError::Provider)?;
            if clear.len() != wire.len() - TAG {
                return Err(SecureError::Provider);
            }
            crate::RtpPacket::parse(&clear).map_err(|_| SecureError::Provider)?;
            Ok(clear)
        })?;
        self.rtp = next;
        Ok(clear)
    }

    /// Authenticate/decrypt encrypted SRTCP with its own explicit-index replay window.
    ///
    /// # Errors
    /// Returns typed packet/replay/source/authentication errors, including rejection
    /// of a clear E-bit. This is structural validation, not an RTCP report decoder.
    /// A terminal [`SecureError::Provider`] failure closes this receiver; later
    /// valid input returns [`SecureError::Closed`].
    pub fn unprotect_rtcp(&mut self, wire: &[u8]) -> Result<Vec<u8>, SecureError> {
        if wire.len() < 8 + CONTROL_TRAILER
            || wire.len() > MAX_PLAINTEXT + CONTROL_TRAILER
            || wire[0] >> 6 != 2
        {
            return Err(SecureError::InvalidPacket);
        }
        let ssrc = source_at(wire, 4);
        self.check_source(ssrc)?;
        let offset = wire.len() - CONTROL_TRAILER;
        let tagged_index = source_at(wire, offset);
        if tagged_index & 0x8000_0000 == 0 {
            return Err(SecureError::InvalidPacket);
        }
        let index = u64::from(tagged_index & MAX_CONTROL_INDEX);
        let next = self.rtcp.propose(index)?;
        let clear = self.receive(ssrc, |session| {
            let mut clear = wire.to_vec();
            session.unprotect_rtcp(&mut clear).map_err(provider_error)?;
            if clear.len() != wire.len() - CONTROL_TRAILER {
                return Err(SecureError::Provider);
            }
            validate_control(&clear)?;
            Ok(clear)
        })?;
        self.rtcp = next;
        Ok(clear)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SSRC: u32 = 0x1020_3040;

    fn key() -> MasterKey {
        MasterKey::from_bytes(&[0x39; 30]).unwrap_or_else(|_| panic!("fictional key rejected"))
    }

    fn rtp(sequence: u16) -> Vec<u8> {
        let mut packet = vec![0x55; 172];
        packet[..2].copy_from_slice(&[0x80, 8]);
        packet[2..4].copy_from_slice(&sequence.to_be_bytes());
        packet[4..8].copy_from_slice(&(u32::from(sequence) * 160).to_be_bytes());
        packet[8..12].copy_from_slice(&SSRC.to_be_bytes());
        packet
    }

    #[test]
    fn authenticated_duplicate_has_specific_replay_error() -> Result<(), SecureError> {
        let mut sender = SecureSender::new(SSRC, key())?;
        let mut receiver = SecureReceiver::new(key())?;
        let wire = sender.protect_rtp(&rtp(10))?;
        assert_eq!(receiver.unprotect_rtp(&wire)?, rtp(10));
        assert!(
            matches!(receiver.unprotect_rtp(&wire), Err(SecureError::Replay)),
            "expected the specific replay error"
        );
        Ok(())
    }

    fn rtcp() -> Vec<u8> {
        let mut packet = vec![0x80, 201, 0, 1];
        packet.extend_from_slice(&SSRC.to_be_bytes());
        packet.extend_from_slice(&[0x81, 202, 0, 3]);
        packet.extend_from_slice(&SSRC.to_be_bytes());
        packet.extend_from_slice(&[1, 3, b'l', b'a', b'b', 0, 0, 0]);
        packet
    }

    #[test]
    fn ciphertext_and_control_tags_have_fixed_overhead() -> Result<(), SecureError> {
        let mut sender = SecureSender::new(SSRC, key())?;
        let mut receiver = SecureReceiver::new(key())?;
        let wire = sender.protect_rtp(&rtp(1))?;
        assert_eq!(wire.len(), rtp(1).len() + TAG);
        assert_ne!(&wire[12..172], &rtp(1)[12..]);
        let control = sender.protect_rtcp(&rtcp())?;
        assert_eq!(control.len(), rtcp().len() + CONTROL_TRAILER);
        assert_ne!(&control[8..24], &rtcp()[8..]);
        assert_eq!(receiver.unprotect_rtcp(&control)?, rtcp());
        assert_eq!(receiver.unprotect_rtp(&wire)?, rtp(1));
        Ok(())
    }

    #[test]
    fn tampered_payload_tag_timestamp_and_sequence_do_not_advance_windows()
    -> Result<(), SecureError> {
        let mut sender = SecureSender::new(SSRC, key())?;
        let mut receiver = SecureReceiver::new(key())?;
        let first = sender.protect_rtp(&rtp(10))?;
        receiver.unprotect_rtp(&first)?;
        let next = sender.protect_rtp(&rtp(11))?;
        for offset in [3, 4, 12, next.len() - 1] {
            let before = receiver.rtp;
            let mut tampered = next.clone();
            tampered[offset] ^= 4;
            assert!(matches!(
                receiver.unprotect_rtp(&tampered),
                Err(SecureError::Authentication)
            ));
            assert_eq!(receiver.rtp, before);
        }
        assert_eq!(receiver.unprotect_rtp(&next)?, rtp(11));
        Ok(())
    }

    #[test]
    fn forged_duplicate_does_not_clear_a_seen_packet() -> Result<(), SecureError> {
        let mut sender = SecureSender::new(SSRC, key())?;
        let mut receiver = SecureReceiver::new(key())?;
        let wire = sender.protect_rtp(&rtp(10))?;
        receiver.unprotect_rtp(&wire)?;
        let before = receiver.rtp;
        let mut forged = wire.clone();
        forged[12] ^= 1;
        assert!(matches!(
            receiver.unprotect_rtp(&forged),
            Err(SecureError::Replay)
        ));
        assert_eq!(receiver.rtp, before);
        assert!(matches!(
            receiver.unprotect_rtp(&wire),
            Err(SecureError::Replay)
        ));
        Ok(())
    }

    #[test]
    fn unauthenticated_source_churn_never_binds_or_retains_provider_sessions()
    -> Result<(), SecureError> {
        let mut sender = SecureSender::new(SSRC, key())?;
        let mut receiver = SecureReceiver::new(key())?;
        let wire = sender.protect_rtp(&rtp(10))?;
        for ssrc in 0_u32..1000 {
            let mut forged = wire.clone();
            forged[8..12].copy_from_slice(&ssrc.to_be_bytes());
            assert!(matches!(
                receiver.unprotect_rtp(&forged),
                Err(SecureError::Authentication)
            ));
            assert!(receiver.session.is_none());
            assert_eq!(receiver.authenticated_ssrc(), None);
            assert_eq!(receiver.rtp, ReplayWindow::default());
            assert_eq!(receiver.rtcp, ReplayWindow::default());
        }
        assert_eq!(receiver.unprotect_rtp(&wire)?, rtp(10));
        assert_eq!(receiver.authenticated_ssrc(), Some(SSRC));
        assert!(receiver.material.is_none());
        Ok(())
    }

    #[test]
    fn authenticated_other_ssrc_cannot_expand_the_bound_receiver() -> Result<(), SecureError> {
        let mut sender = SecureSender::new(SSRC, key())?;
        let mut receiver = SecureReceiver::new(key())?;
        let wire = sender.protect_rtp(&rtp(10))?;
        receiver.unprotect_rtp(&wire)?;
        let before = receiver.rtp;
        let mut other_sender = SecureSender::new(SSRC + 1, key())?;
        let mut other = rtp(11);
        other[8..12].copy_from_slice(&(SSRC + 1).to_be_bytes());
        let protected = other_sender.protect_rtp(&other)?;
        assert!(matches!(
            receiver.unprotect_rtp(&protected),
            Err(SecureError::WrongSource)
        ));
        assert_eq!(receiver.rtp, before);
        assert_eq!(receiver.authenticated_ssrc(), Some(SSRC));
        assert!(receiver.session.is_some());
        Ok(())
    }

    #[test]
    fn sixty_three_packet_reordering_is_accepted_once_but_sixty_four_is_too_old()
    -> Result<(), SecureError> {
        let mut sender = SecureSender::new(SSRC, key())?;
        let mut receiver = SecureReceiver::new(key())?;
        let mut packets = Vec::new();
        for sequence in 10..=74 {
            packets.push(sender.protect_rtp(&rtp(sequence))?);
        }
        receiver.unprotect_rtp(&packets[64])?;
        assert_eq!(receiver.unprotect_rtp(&packets[1])?, rtp(11));
        assert!(matches!(
            receiver.unprotect_rtp(&packets[1]),
            Err(SecureError::Replay)
        ));
        assert!(matches!(
            receiver.unprotect_rtp(&packets[0]),
            Err(SecureError::TooOld)
        ));
        Ok(())
    }

    #[test]
    fn rollover_reordering_and_replay_share_the_correct_extended_index() -> Result<(), SecureError>
    {
        let mut sender = SecureSender::new(SSRC, key())?;
        let mut receiver = SecureReceiver::new(key())?;
        let before = sender.protect_rtp(&rtp(65534))?;
        let late = sender.protect_rtp(&rtp(65535))?;
        let after = sender.protect_rtp(&rtp(0))?;
        receiver.unprotect_rtp(&before)?;
        receiver.unprotect_rtp(&after)?;
        assert_eq!(receiver.unprotect_rtp(&late)?, rtp(65535));
        assert!(matches!(
            receiver.unprotect_rtp(&late),
            Err(SecureError::Replay)
        ));
        assert_eq!(receiver.rtp.highest, Some(65536));
        Ok(())
    }

    #[test]
    fn forged_rollover_does_not_poison_the_roc() -> Result<(), SecureError> {
        let mut sender = SecureSender::new(SSRC, key())?;
        let mut receiver = SecureReceiver::new(key())?;
        let before = sender.protect_rtp(&rtp(65534))?;
        let last = sender.protect_rtp(&rtp(65535))?;
        let rollover = sender.protect_rtp(&rtp(0))?;
        receiver.unprotect_rtp(&before)?;
        let window = receiver.rtp;
        let mut forged = rollover.clone();
        forged[12] ^= 1;
        assert!(matches!(
            receiver.unprotect_rtp(&forged),
            Err(SecureError::Authentication)
        ));
        assert_eq!(receiver.rtp, window);
        assert_eq!(receiver.unprotect_rtp(&last)?, rtp(65535));
        assert_eq!(receiver.unprotect_rtp(&rollover)?, rtp(0));
        Ok(())
    }

    #[test]
    fn sender_and_receiver_agree_on_forward_gap_across_wrap() -> Result<(), SecureError> {
        let mut sender = SecureSender::new(SSRC, key())?;
        let mut receiver = SecureReceiver::new(key())?;
        receiver.unprotect_rtp(&sender.protect_rtp(&rtp(40000))?)?;
        assert_eq!(
            receiver.unprotect_rtp(&sender.protect_rtp(&rtp(5000))?)?,
            rtp(5000)
        );
        assert_eq!(sender.highest, Some(65536 + 5000));
        assert_eq!(receiver.rtp.highest, sender.highest);
        Ok(())
    }

    #[test]
    fn valid_header_oversized_packets_are_rejected_by_size_not_version() -> Result<(), SecureError>
    {
        let mut sender = SecureSender::new(SSRC, key())?;
        let mut receiver = SecureReceiver::new(key())?;
        let mut oversized = rtp(1);
        oversized.resize(MAX_PLAINTEXT + 1, 0x55);
        assert!(matches!(
            sender.protect_rtp(&oversized),
            Err(SecureError::InvalidPacket)
        ));
        let mut wire = sender.protect_rtp(&rtp(1))?;
        wire.resize(MAX_PLAINTEXT + TAG + 1, 0x55);
        assert!(matches!(
            receiver.unprotect_rtp(&wire),
            Err(SecureError::InvalidPacket)
        ));
        assert_eq!(receiver.authenticated_ssrc(), None);
        Ok(())
    }

    #[test]
    fn marker_payload_type_csrc_extension_and_padding_flags_are_authenticated()
    -> Result<(), SecureError> {
        let mut sender = SecureSender::new(SSRC, key())?;
        let mut receiver = SecureReceiver::new(key())?;
        receiver.unprotect_rtp(&sender.protect_rtp(&rtp(10))?)?;
        let mut plain = rtp(11)[..12].to_vec();
        plain[0] = 0xb1;
        plain.extend_from_slice(&[0, 0, 0, 7, 0xbe, 0xde, 0, 1, 0x10, 0xab, 0, 0]);
        plain.extend_from_slice(&rtp(11)[12..]);
        plain.extend_from_slice(&[4; 4]);
        let wire = sender.protect_rtp(&plain)?;
        let window = receiver.rtp;
        for (offset, mask) in [
            (0, 0x20),
            (0, 0x10),
            (0, 1),
            (1, 0x80),
            (1, 1),
            (13, 1),
            (16, 1),
            (20, 1),
        ] {
            let mut forged = wire.clone();
            forged[offset] ^= mask;
            assert!(matches!(
                receiver.unprotect_rtp(&forged),
                Err(SecureError::Authentication)
            ));
            assert_eq!(receiver.rtp, window);
        }
        assert_eq!(receiver.unprotect_rtp(&wire)?, plain);
        Ok(())
    }

    #[test]
    fn sender_rejects_reused_and_backward_indices_but_allows_forward_gaps()
    -> Result<(), SecureError> {
        let mut sender = SecureSender::new(SSRC, key())?;
        sender.protect_rtp(&rtp(10))?;
        for sequence in [10, 9] {
            assert!(matches!(
                sender.protect_rtp(&rtp(sequence)),
                Err(SecureError::IndexReuse)
            ));
        }
        assert!(sender.protect_rtp(&rtp(12)).is_ok());
        Ok(())
    }

    #[test]
    fn invalid_sender_source_does_not_consume_a_nonce() -> Result<(), SecureError> {
        let mut sender = SecureSender::new(SSRC, key())?;
        let mut wrong = rtp(1);
        wrong[8] ^= 1;
        assert!(matches!(
            sender.protect_rtp(&wrong),
            Err(SecureError::WrongSource)
        ));
        assert!(sender.protect_rtp(&rtp(1)).is_ok());
        let mut wrong_control = rtcp();
        wrong_control[4] ^= 1;
        assert!(matches!(
            sender.protect_rtcp(&wrong_control),
            Err(SecureError::WrongSource)
        ));
        assert_eq!(sender.control_count, 0);
        sender.protect_rtcp(&rtcp())?;
        assert_eq!(sender.control_count, 1);
        Ok(())
    }

    #[test]
    fn maximum_rtp_index_closes_both_transmit_paths_before_reuse() -> Result<(), SecureError> {
        let mut sender = SecureSender::new(SSRC, key())?;
        sender.highest = Some(MAX_RTP_INDEX);
        assert!(matches!(
            sender.protect_rtp(&rtp(0)),
            Err(SecureError::KeyExpired)
        ));
        assert!(matches!(
            sender.protect_rtcp(&rtcp()),
            Err(SecureError::Closed)
        ));
        Ok(())
    }

    #[test]
    fn skipped_last_rtp_index_cannot_wrap_the_roc_or_keep_the_sender_open()
    -> Result<(), SecureError> {
        let mut sender = SecureSender::new(SSRC, key())?;
        sender.highest = Some(MAX_RTP_INDEX - 1);
        assert!(matches!(
            sender.protect_rtp(&rtp(0)),
            Err(SecureError::KeyExpired)
        ));
        assert!(matches!(
            sender.protect_rtcp(&rtcp()),
            Err(SecureError::Closed)
        ));
        Ok(())
    }

    #[test]
    fn maximum_control_index_closes_both_transmit_paths_before_reuse() -> Result<(), SecureError> {
        let mut sender = SecureSender::new(SSRC, key())?;
        sender.control_count = MAX_CONTROL_INDEX;
        assert!(matches!(
            sender.protect_rtcp(&rtcp()),
            Err(SecureError::KeyExpired)
        ));
        assert!(matches!(
            sender.protect_rtp(&rtp(1)),
            Err(SecureError::Closed)
        ));
        Ok(())
    }

    #[test]
    fn key_length_and_debug_are_redacted() -> Result<(), SecureError> {
        for length in [0, 16, 29, 31, 1000] {
            assert!(matches!(
                MasterKey::from_bytes(&vec![0x39; length]),
                Err(SecureError::InvalidKey)
            ));
        }
        assert_eq!(format!("{:?}", key()), "MasterKey([REDACTED])");
        assert_eq!(
            format!("{:?}", SecureSender::new(SSRC, key())?),
            "SecureSender([REDACTED])"
        );
        assert_eq!(
            format!("{:?}", SecureReceiver::new(key())?),
            "SecureReceiver([REDACTED])"
        );
        Ok(())
    }

    #[test]
    fn receive_only_provider_session_cannot_accidentally_transmit() -> Result<(), SecureError> {
        let mut session = receiver_session(&key())?;
        let packet = ProviderRtp::parse(&rtp(1)).map_err(|_| SecureError::InvalidPacket)?;
        let mut output = vec![0; session.protected_rtp_len(&packet)];
        assert!(matches!(
            session.protect_rtp(&packet, &mut output),
            Err(SrtpError::UnsupportedProfile)
        ));
        assert!(matches!(
            session.protect_rtcp(&mut rtcp()),
            Err(SrtpError::UnsupportedProfile)
        ));
        Ok(())
    }

    #[test]
    fn maximum_sized_media_and_control_packets_have_bounded_roundtrips() -> Result<(), SecureError>
    {
        let mut sender = SecureSender::new(SSRC, key())?;
        let mut receiver = SecureReceiver::new(key())?;
        let mut plain = rtp(1);
        plain.resize(MAX_PLAINTEXT, 0x55);
        let wire = sender.protect_rtp(&plain)?;
        assert_eq!(wire.len(), MAX_PLAINTEXT + TAG);
        assert_eq!(receiver.unprotect_rtp(&wire)?, plain);
        let mut control = rtcp().repeat(62);
        control.extend_from_slice(&[0x80, 204, 0, 2]);
        control.extend_from_slice(&SSRC.to_be_bytes());
        control.extend_from_slice(b"TEST");
        assert_eq!(control.len(), MAX_PLAINTEXT);
        let wire = sender.protect_rtcp(&control)?;
        assert_eq!(wire.len(), MAX_PLAINTEXT + CONTROL_TRAILER);
        assert_eq!(receiver.unprotect_rtcp(&wire)?, control);
        Ok(())
    }

    #[test]
    fn oversized_and_malformed_packets_never_learn_a_source() -> Result<(), SecureError> {
        let mut sender = SecureSender::new(SSRC, key())?;
        let mut receiver = SecureReceiver::new(key())?;
        let mut bad_extension = rtp(1);
        bad_extension[0] |= 0x10;
        bad_extension[14..16].copy_from_slice(&u16::MAX.to_be_bytes());
        let mut bad_padding = rtp(1);
        bad_padding[0] |= 0x20;
        bad_padding[171] = 0;
        for packet in [
            vec![],
            vec![0; 11],
            vec![0; MAX_PLAINTEXT + 1],
            bad_extension,
            bad_padding,
        ] {
            assert!(matches!(
                sender.protect_rtp(&packet),
                Err(SecureError::InvalidPacket)
            ));
        }
        for packet in [vec![], vec![0; 21], vec![0; MAX_PLAINTEXT + TAG + 1]] {
            assert!(matches!(
                receiver.unprotect_rtp(&packet),
                Err(SecureError::InvalidPacket)
            ));
        }
        assert_eq!(receiver.authenticated_ssrc(), None);
        assert_eq!(receiver.rtp, ReplayWindow::default());
        assert!(sender.protect_rtp(&rtp(1)).is_ok());
        Ok(())
    }

    #[test]
    fn csrc_extensions_and_padding_survive_authenticated_roundtrip() -> Result<(), SecureError> {
        let mut sender = SecureSender::new(SSRC, key())?;
        let mut receiver = SecureReceiver::new(key())?;
        let plain = rtp(1);
        let mut packet = plain[..12].to_vec();
        packet[0] = 0xb1;
        packet.extend_from_slice(&[0, 0, 0, 7]);
        packet.extend_from_slice(&[0xbe, 0xde, 0, 1, 0x10, 0xab, 0, 0]);
        packet.extend_from_slice(&plain[12..]);
        packet.extend_from_slice(&[4, 4, 4, 4]);
        let wire = sender.protect_rtp(&packet)?;
        assert_eq!(receiver.unprotect_rtp(&wire)?, packet);
        Ok(())
    }

    #[test]
    fn wrong_key_cannot_bind_or_poison_the_receiver() -> Result<(), SecureError> {
        let mut sender = SecureSender::new(SSRC, MasterKey::from_bytes(&[0x77; 30])?)?;
        let mut receiver = SecureReceiver::new(key())?;
        assert!(matches!(
            receiver.unprotect_rtp(&sender.protect_rtp(&rtp(1))?),
            Err(SecureError::Authentication)
        ));
        assert_eq!(receiver.authenticated_ssrc(), None);
        let mut good = SecureSender::new(SSRC, key())?;
        assert_eq!(receiver.unprotect_rtp(&good.protect_rtp(&rtp(1))?)?, rtp(1));
        Ok(())
    }

    #[test]
    fn control_replay_window_is_independent_and_accepts_unseen_in_window_packet()
    -> Result<(), SecureError> {
        let mut sender = SecureSender::new(SSRC, key())?;
        let mut receiver = SecureReceiver::new(key())?;
        let mut packets = Vec::new();
        for _ in 0..65 {
            packets.push(sender.protect_rtcp(&rtcp())?);
        }
        receiver.unprotect_rtcp(&packets[64])?;
        assert_eq!(receiver.unprotect_rtcp(&packets[1])?, rtcp());
        assert!(matches!(
            receiver.unprotect_rtcp(&packets[1]),
            Err(SecureError::Replay)
        ));
        assert!(matches!(
            receiver.unprotect_rtcp(&packets[0]),
            Err(SecureError::TooOld)
        ));
        assert_eq!(
            receiver.unprotect_rtp(&sender.protect_rtp(&rtp(2))?)?,
            rtp(2)
        );
        assert_eq!(receiver.rtp.highest, Some(2));
        assert_eq!(receiver.rtcp.highest, Some(65));
        Ok(())
    }

    #[test]
    fn forged_control_index_tag_and_clear_encryption_flag_do_not_change_state()
    -> Result<(), SecureError> {
        let mut sender = SecureSender::new(SSRC, key())?;
        let mut receiver = SecureReceiver::new(key())?;
        let wire = sender.protect_rtcp(&rtcp())?;
        for offset in [8, wire.len() - 1, wire.len() - CONTROL_TRAILER + 3] {
            let mut forged = wire.clone();
            forged[offset] ^= 8;
            assert!(matches!(
                receiver.unprotect_rtcp(&forged),
                Err(SecureError::Authentication)
            ));
            assert_eq!(receiver.authenticated_ssrc(), None);
            assert_eq!(receiver.rtcp, ReplayWindow::default());
        }
        let mut clear_flag = wire.clone();
        clear_flag[wire.len() - CONTROL_TRAILER] &= 0x7f;
        assert!(matches!(
            receiver.unprotect_rtcp(&clear_flag),
            Err(SecureError::InvalidPacket)
        ));
        assert_eq!(receiver.unprotect_rtcp(&wire)?, rtcp());
        assert!(matches!(
            receiver.unprotect_rtcp(&wire),
            Err(SecureError::Replay)
        ));
        Ok(())
    }

    #[test]
    fn authenticated_malformed_control_does_not_bind_or_consume_replay_state()
    -> Result<(), SecureError> {
        let mut provider = SrtpContext::new(
            SSRC,
            SrtpProfile::Aes128Sha1_80,
            key().provider_material(),
            SrtpDirection::Sender,
        )
        .map_err(|_| SecureError::Provider)?;
        let mut malformed = rtcp();
        malformed[2..4].copy_from_slice(&u16::MAX.to_be_bytes());
        provider
            .protect_rtcp(&mut malformed)
            .map_err(|_| SecureError::Provider)?;
        let mut receiver = SecureReceiver::new(key())?;
        assert!(matches!(
            receiver.unprotect_rtcp(&malformed),
            Err(SecureError::InvalidPacket)
        ));
        assert_eq!(receiver.authenticated_ssrc(), None);
        assert_eq!(receiver.rtcp, ReplayWindow::default());
        let mut sender = SecureSender::new(SSRC, key())?;
        assert_eq!(
            receiver.unprotect_rtcp(&sender.protect_rtcp(&rtcp())?)?,
            rtcp()
        );
        let window = receiver.rtcp;
        let mut malformed = rtcp();
        malformed[2..4].copy_from_slice(&u16::MAX.to_be_bytes());
        provider
            .protect_rtcp(&mut malformed)
            .map_err(|_| SecureError::Provider)?;
        assert!(matches!(
            receiver.unprotect_rtcp(&malformed),
            Err(SecureError::InvalidPacket)
        ));
        assert_eq!(receiver.rtcp, window);
        assert_eq!(receiver.authenticated_ssrc(), Some(SSRC));
        assert_eq!(
            receiver.unprotect_rtcp(&sender.protect_rtcp(&rtcp())?)?,
            rtcp()
        );
        Ok(())
    }

    #[test]
    fn malformed_control_does_not_consume_transmit_index() -> Result<(), SecureError> {
        let mut sender = SecureSender::new(SSRC, key())?;
        let mut zero_length_first = rtcp();
        zero_length_first[2..4].copy_from_slice(&[0, 0]);
        let mut zero_padding = rtcp();
        zero_padding[8] |= 0x20;
        for packet in [
            vec![0; 7],
            vec![0; MAX_PLAINTEXT + 1],
            zero_length_first,
            zero_padding,
        ] {
            assert!(matches!(
                sender.protect_rtcp(&packet),
                Err(SecureError::InvalidPacket)
            ));
        }
        assert_eq!(sender.control_count, 0);
        assert!(sender.protect_rtcp(&rtcp()).is_ok());
        Ok(())
    }

    #[test]
    fn index_estimation_does_not_wrap_the_key_lifetime() {
        assert_eq!(
            rtp_index(Some(MAX_RTP_INDEX), 0),
            Err(SecureError::KeyExpired)
        );
        assert_eq!(rtp_index(Some(10), 65535), Err(SecureError::TooOld));
        assert_eq!(rtp_index(Some(65534), 0), Ok(65536));
        assert_eq!(rtp_index(Some(65536), 65535), Ok(65535));
    }

    #[test]
    fn replay_bitmap_matches_a_bounded_set_model_under_seeded_reordering_and_duplicates() {
        use std::collections::BTreeSet;
        for base in [0_u64, 65500, MAX_RTP_INDEX - 20000] {
            let mut window = ReplayWindow::default();
            let mut model = BTreeSet::new();
            let mut highest = None;
            let mut random = 7_u64; // Test scheduling only, never cryptographic randomness.
            let mut cursor = base + 130;
            for _ in 0..10000 {
                random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                cursor += random % 3;
                let index = cursor - ((random >> 8) % 130);
                let expected = match highest {
                    Some(h) if index <= h && h - index >= 64 => Err(SecureError::TooOld),
                    _ if model.contains(&index) => Err(SecureError::Replay),
                    _ => Ok(()),
                };
                let proposed = window.propose(index);
                assert_eq!(proposed.map(|_| ()), expected);
                if let Ok(next) = proposed {
                    window = next;
                    highest = Some(highest.map_or(index, |h: u64| h.max(index)));
                    model.insert(index);
                    let h = highest.unwrap_or(index);
                    model.retain(|v| h - *v < 64);
                    assert_eq!(window.highest, highest);
                    assert!(model.len() <= 64);
                }
            }
        }
    }
}

/// Maximum retained authenticated remote SSRCs in one receive-side key epoch.
/// Contexts are never evicted: forgetting one would erase replay history.
pub const MAX_REMOTE_SOURCES: usize = 4;
const AUTH_PER_SECOND: u16 = 256;
const NEW_SOURCE_AUTH_PER_SECOND: u16 = 16;

struct AuthBudget {
    start: Instant,
    remaining: u16,
    new_sources: u16,
}
impl AuthBudget {
    fn admit(&mut self, unknown: bool, now: Instant) -> Result<(), SecureError> {
        let elapsed = now
            .checked_duration_since(self.start)
            .ok_or(SecureError::RateLimited)?;
        if elapsed >= Duration::from_secs(1) {
            self.start = now;
            self.remaining = AUTH_PER_SECOND;
            self.new_sources = NEW_SOURCE_AUTH_PER_SECOND;
        }
        if self.remaining == 0 || (unknown && self.new_sources == 0) {
            return Err(SecureError::RateLimited);
        }
        self.remaining -= 1;
        if unknown {
            self.new_sources -= 1;
        }
        Ok(())
    }
}
struct RemoteSource {
    ssrc: u32,
    receiver: SecureReceiver,
}

/// A receive-side SDES key epoch with a strict authenticated-source bound.
///
/// Retains at most four per-SSRC contexts and both replay histories until close.
/// RTCP may arrive first or use a reporter SSRC distinct from the media source.
/// No eviction, same-key reset or rekey API is exposed. At capacity, a new source
/// returns a typed error while known-source state remains intact. The call owner
/// must handle that condition explicitly, never reconstruct this object to retry.
/// Caller-supplied time must come from its monotonic clock, not the network.
/// Fixed one-second windows admit at most 256 packets/16 unknown-source attempts;
/// failed authentication consumes budget, but never learns a source. This bounds
/// provider work, not UDP reception, prefilter CPU or real-time scheduling.
/// Validate the negotiated transport address before invoking this worker API.
/// Own master bytes are zeroized on close/drop; provider copies are not guaranteed.
///
/// ```compile_fail
/// use rust_microsip_media::srtp_adapter::ReceiverBank;
/// fn requires_clone<T:Clone>() {}
/// requires_clone::<ReceiverBank>();
/// ```
pub struct ReceiverBank {
    material: Option<MasterKey>,
    sources: Vec<RemoteSource>,
    budget: AuthBudget,
}
impl fmt::Debug for ReceiverBank {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ReceiverBank([REDACTED])")
    }
}
impl ReceiverBank {
    /// Consume one remote epoch key, without creating provisional provider contexts.
    pub fn new(key: MasterKey, now: Instant) -> Self {
        Self {
            material: Some(key),
            sources: Vec::with_capacity(MAX_REMOTE_SOURCES),
            budget: AuthBudget {
                start: now,
                remaining: AUTH_PER_SECOND,
                new_sources: NEW_SOURCE_AUTH_PER_SECOND,
            },
        }
    }
    /// Count only sources with authenticated, structurally valid packets.
    pub fn authenticated_sources(&self) -> usize {
        self.sources.len()
    }
    /// Permanently discard all contexts and own master material for this epoch.
    pub fn close(&mut self) {
        self.sources.clear();
        self.material = None;
    }
    fn receive(
        &mut self,
        wire: &[u8],
        now: Instant,
        kind: PacketKind,
    ) -> Result<Vec<u8>, SecureError> {
        if self.material.is_none() {
            return Err(SecureError::Closed);
        }
        let (minimum, maximum, source_offset) = match kind {
            PacketKind::Rtp => (12 + TAG, MAX_PLAINTEXT + TAG, 8),
            PacketKind::Rtcp => (8 + CONTROL_TRAILER, MAX_PLAINTEXT + CONTROL_TRAILER, 4),
        };
        if wire.len() < minimum || wire.len() > maximum || wire[0] >> 6 != 2 {
            return Err(SecureError::InvalidPacket);
        }
        let ssrc = source_at(wire, source_offset);
        let known = self.sources.iter().position(|source| source.ssrc == ssrc);
        if known.is_none() && self.sources.len() == MAX_REMOTE_SOURCES {
            return Err(SecureError::SourceCapacity);
        }
        self.budget.admit(known.is_none(), now)?;
        let result = if let Some(index) = known {
            kind.unprotect(&mut self.sources[index].receiver, wire)
        } else {
            let key = self.material.as_ref().ok_or(SecureError::Closed)?;
            match SecureReceiver::new(MasterKey::from_bytes(key.0.as_ref())?) {
                Ok(mut receiver) => match kind.unprotect(&mut receiver, wire) {
                    Ok(clear) if receiver.authenticated_ssrc() == Some(ssrc) => {
                        self.sources.push(RemoteSource { ssrc, receiver });
                        Ok(clear)
                    }
                    Ok(_) => Err(SecureError::Provider),
                    Err(error) => Err(error),
                },
                Err(error) => Err(error),
            }
        };
        if matches!(
            result,
            Err(SecureError::Provider | SecureError::Closed | SecureError::KeyExpired)
        ) {
            self.close();
        }
        result
    }
    /// Authenticate/decrypt RTP, preserving replay state independently per SSRC.
    ///
    /// # Errors
    /// Returns packet/auth/replay errors, rate/capacity errors or terminal closure.
    pub fn unprotect_rtp(&mut self, wire: &[u8], now: Instant) -> Result<Vec<u8>, SecureError> {
        self.receive(wire, now, PacketKind::Rtp)
    }
    /// Authenticate/decrypt SRTCP, permitting RTCP-first and distinct reporters.
    ///
    /// # Errors
    /// Returns packet/auth/replay errors, rate/capacity errors or terminal closure.
    pub fn unprotect_rtcp(&mut self, wire: &[u8], now: Instant) -> Result<Vec<u8>, SecureError> {
        self.receive(wire, now, PacketKind::Rtcp)
    }
}
#[derive(Clone, Copy)]
enum PacketKind {
    Rtp,
    Rtcp,
}
impl PacketKind {
    fn unprotect(self, receiver: &mut SecureReceiver, wire: &[u8]) -> Result<Vec<u8>, SecureError> {
        match self {
            Self::Rtp => receiver.unprotect_rtp(wire),
            Self::Rtcp => receiver.unprotect_rtcp(wire),
        }
    }
}

#[cfg(test)]
mod bank_tests {
    use super::*;
    fn sender(ssrc: u32) -> Result<SecureSender, SecureError> {
        SecureSender::new(ssrc, MasterKey::from_bytes(&[0x31; 30])?)
    }
    fn rtp(ssrc: u32, sequence: u16) -> Result<Vec<u8>, SecureError> {
        let mut bytes = [0; 32];
        let n = crate::RtpHeader {
            marker: false,
            payload_type: 8,
            sequence,
            timestamp: u32::from(sequence) * 160,
            ssrc,
        }
        .write(&[0x55; 20], &mut bytes)
        .map_err(|_| SecureError::InvalidPacket)?;
        Ok(bytes[..n].to_vec())
    }
    fn control(ssrc: u32) -> Vec<u8> {
        let mut bytes = vec![0x80, 201, 0, 1];
        bytes.extend(ssrc.to_be_bytes());
        bytes
    }
    fn bank(now: Instant) -> Result<ReceiverBank, SecureError> {
        Ok(ReceiverBank::new(MasterKey::from_bytes(&[0x31; 30])?, now))
    }
    #[test]
    fn multiple_authenticated_sources_keep_their_replay_history() -> Result<(), SecureError> {
        let now = Instant::now();
        let mut bank = bank(now)?;
        let a = sender(7)?.protect_rtp(&rtp(7, 1)?)?;
        let b = sender(8)?.protect_rtp(&rtp(8, 1)?)?;
        assert!(bank.unprotect_rtp(&a, now).is_ok());
        assert!(
            bank.unprotect_rtp(&b, now).is_ok(),
            "a different authentic source must not lose its stream"
        );
        assert_eq!(bank.unprotect_rtp(&a, now), Err(SecureError::Replay));
        assert_eq!(bank.authenticated_sources(), 2);
        Ok(())
    }
    #[test]
    fn rtcp_first_and_distinct_reporter_are_retained() -> Result<(), SecureError> {
        let now = Instant::now();
        let mut bank = bank(now)?;
        let rtcp = sender(11)?.protect_rtcp(&control(11))?;
        let rtp = sender(12)?.protect_rtp(&rtp(12, 1)?)?;
        assert!(bank.unprotect_rtcp(&rtcp, now).is_ok());
        assert!(bank.unprotect_rtp(&rtp, now).is_ok());
        assert_eq!(bank.unprotect_rtcp(&rtcp, now), Err(SecureError::Replay));
        assert_eq!(bank.authenticated_sources(), 2);
        Ok(())
    }
    #[test]
    fn one_source_keeps_separate_media_control_windows() -> Result<(), SecureError> {
        let now = Instant::now();
        let mut bank = bank(now)?;
        let mut sender = sender(7)?;
        let rtcp = sender.protect_rtcp(&control(7))?;
        let rtp = sender.protect_rtp(&rtp(7, 1)?)?;
        assert!(bank.unprotect_rtcp(&rtcp, now).is_ok());
        assert!(bank.unprotect_rtp(&rtp, now).is_ok());
        assert_eq!(bank.authenticated_sources(), 1);
        assert_eq!(bank.unprotect_rtp(&rtp, now), Err(SecureError::Replay));
        assert_eq!(bank.unprotect_rtcp(&rtcp, now), Err(SecureError::Replay));
        Ok(())
    }
    #[test]
    fn capacity_does_not_evict_old_replay_state() -> Result<(), SecureError> {
        let now = Instant::now();
        let mut bank = bank(now)?;
        let mut first = sender(1)?;
        let original = first.protect_rtp(&rtp(1, 1)?)?;
        bank.unprotect_rtp(&original, now)?;
        for ssrc in 2..=4 {
            let wire = sender(ssrc)?.protect_rtp(&rtp(ssrc, 1)?)?;
            bank.unprotect_rtp(&wire, now)?;
        }
        let fifth = sender(5)?.protect_rtp(&rtp(5, 1)?)?;
        assert_eq!(
            bank.unprotect_rtp(&fifth, now),
            Err(SecureError::SourceCapacity)
        );
        assert_eq!(bank.unprotect_rtp(&original, now), Err(SecureError::Replay));
        assert!(
            bank.unprotect_rtp(&first.protect_rtp(&rtp(1, 2)?)?, now)
                .is_ok()
        );
        assert_eq!(bank.authenticated_sources(), 4);
        Ok(())
    }
    #[test]
    fn forged_source_flood_is_rate_limited_without_retention() -> Result<(), SecureError> {
        let now = Instant::now();
        let mut bank = bank(now)?;
        let original = sender(7)?.protect_rtp(&rtp(7, 1)?)?;
        let mut authentication = 0;
        let mut limited = 0;
        for ssrc in 100..1100u32 {
            let mut forged = original.clone();
            forged[8..12].copy_from_slice(&ssrc.to_be_bytes());
            match bank.unprotect_rtp(&forged, now) {
                Err(SecureError::Authentication) => authentication += 1,
                Err(SecureError::RateLimited) => limited += 1,
                _ => return Err(SecureError::Provider),
            }
        }
        assert_eq!((authentication, limited), (16, 984));
        assert_eq!(bank.authenticated_sources(), 0);
        assert!(
            bank.unprotect_rtp(&original, now + Duration::from_secs(1))
                .is_ok()
        );
        Ok(())
    }
    #[test]
    fn known_source_flood_preserves_state_across_refill() -> Result<(), SecureError> {
        let now = Instant::now();
        let mut bank = bank(now)?;
        let mut tx = sender(7)?;
        let first = tx.protect_rtp(&rtp(7, 1)?)?;
        bank.unprotect_rtp(&first, now)?;
        for _ in 1..AUTH_PER_SECOND {
            assert_eq!(bank.unprotect_rtp(&first, now), Err(SecureError::Replay));
        }
        let next = tx.protect_rtp(&rtp(7, 2)?)?;
        assert_eq!(
            bank.unprotect_rtp(&next, now),
            Err(SecureError::RateLimited)
        );
        assert!(
            bank.unprotect_rtp(&next, now + Duration::from_secs(1))
                .is_ok()
        );
        assert_eq!(
            bank.unprotect_rtp(&first, now + Duration::from_secs(1)),
            Err(SecureError::Replay)
        );
        Ok(())
    }
    #[test]
    fn backwards_time_never_refills_or_accepts_a_packet() -> Result<(), SecureError> {
        let now = Instant::now();
        let mut bank = bank(now)?;
        let wire = sender(7)?.protect_rtp(&rtp(7, 1)?)?;
        let backwards = now
            .checked_sub(Duration::from_millis(1))
            .ok_or(SecureError::Provider)?;
        assert_eq!(
            bank.unprotect_rtp(&wire, backwards),
            Err(SecureError::RateLimited)
        );
        assert_eq!(bank.authenticated_sources(), 0);
        assert!(bank.unprotect_rtp(&wire, now).is_ok());
        Ok(())
    }
    #[test]
    fn close_is_terminal_and_discards_owned_epoch_material() -> Result<(), SecureError> {
        let now = Instant::now();
        let mut bank = bank(now)?;
        let wire = sender(7)?.protect_rtp(&rtp(7, 1)?)?;
        bank.unprotect_rtp(&wire, now)?;
        bank.close();
        assert!(bank.material.is_none());
        assert_eq!(bank.authenticated_sources(), 0);
        assert_eq!(bank.unprotect_rtp(&wire, now), Err(SecureError::Closed));
        Ok(())
    }
    #[test]
    fn bank_debug_is_redacted() -> Result<(), SecureError> {
        assert_eq!(
            format!("{:?}", bank(Instant::now())?),
            "ReceiverBank([REDACTED])"
        );
        Ok(())
    }
    #[test]
    fn oversize_and_short_headers_never_authenticate() -> Result<(), SecureError> {
        let now = Instant::now();
        let mut bank = bank(now)?;
        assert_eq!(
            bank.unprotect_rtp(&[0; 21], now),
            Err(SecureError::InvalidPacket)
        );
        assert_eq!(
            bank.unprotect_rtcp(&vec![0x80; 1515], now),
            Err(SecureError::InvalidPacket)
        );
        assert_eq!(bank.authenticated_sources(), 0);
        Ok(())
    }
}
