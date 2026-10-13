//! Isolated RFC 4733 IVR proof; not the production call-control actor.
use super::{
    AudioTarget, CallProbeError, LabEndpoint, PacketTracker, ToneMeter, offer, parse_offer,
};
use crate::{RegistrationConfig, Transport};
use rsipstack::{
    dialog::invitation::InviteOption,
    sip::{StatusCode, Uri},
    transport::SipAddr,
};
use rust_microsip_media::{
    Codec, RtpHeader, RtpPacket,
    dtmf::{Digit, Transmission},
};
use std::{net::SocketAddr, time::Duration};
use tokio::{
    net::UdpSocket,
    time::{Instant, Interval, MissedTickBehavior},
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy)]
struct NegotiatedEvents {
    payload_type: u8,
    mask: u16,
}
fn parse_mask(value: &str) -> Result<u16, CallProbeError> {
    if value.is_empty() || value.len() > 512 || value.bytes().any(|b| b.is_ascii_whitespace()) {
        return Err(CallProbeError::UnsupportedDtmf);
    }
    let mut mask = 0;
    for part in value.split(',') {
        let (start, end) = part.split_once('-').unwrap_or((part, part));
        let start: u8 = start.parse().map_err(|_| CallProbeError::UnsupportedDtmf)?;
        let end: u8 = end.parse().map_err(|_| CallProbeError::UnsupportedDtmf)?;
        if start > end {
            return Err(CallProbeError::UnsupportedDtmf);
        }
        for event in start..=end.min(15) {
            mask |= 1u16 << event;
        }
    }
    Ok(mask)
}
fn negotiated_events(bytes: &[u8], offered_pt: u8) -> Result<NegotiatedEvents, CallProbeError> {
    if bytes.is_empty() || bytes.len() > 8192 || !(96..=127).contains(&offered_pt) {
        return Err(CallProbeError::UnsupportedDtmf);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| CallProbeError::UnsupportedDtmf)?;
    let (mut listed, mut mapped, mut mask) = (false, false, None);
    for (i, line) in text.lines().enumerate() {
        if i >= 128 || line.len() > 512 || line.contains('\0') {
            return Err(CallProbeError::UnsupportedDtmf);
        }
        if let Some(value) = line.strip_prefix("m=audio ") {
            listed = value
                .split_ascii_whitespace()
                .skip(2)
                .any(|p| p.parse::<u8>().ok() == Some(offered_pt));
        }
        if let Some(value) = line.strip_prefix("a=rtpmap:") {
            let (pt, encoding) = value
                .split_once(' ')
                .ok_or(CallProbeError::UnsupportedDtmf)?;
            if pt.parse::<u8>().ok() == Some(offered_pt) {
                if mapped
                    || (!encoding.eq_ignore_ascii_case("telephone-event/8000")
                        && !encoding.eq_ignore_ascii_case("telephone-event/8000/1"))
                {
                    return Err(CallProbeError::UnsupportedDtmf);
                }
                mapped = true;
            }
        }
        if let Some(value) = line.strip_prefix("a=fmtp:") {
            let (pt, events) = value
                .split_once(' ')
                .ok_or(CallProbeError::UnsupportedDtmf)?;
            if pt.parse::<u8>().ok() == Some(offered_pt) {
                if mask.is_some() {
                    return Err(CallProbeError::UnsupportedDtmf);
                }
                mask = Some(parse_mask(events)?);
            }
        }
    }
    if !listed || !mapped {
        return Err(CallProbeError::UnsupportedDtmf);
    }
    Ok(NegotiatedEvents {
        payload_type: offered_pt,
        mask: mask.unwrap_or(u16::MAX),
    })
}
fn dtmf_offer(address: SocketAddr, codec: Codec) -> Vec<u8> {
    let mut sdp = String::from_utf8_lossy(&offer(address, codec)).into_owned();
    sdp = sdp.replace(
        &format!("RTP/AVP {}\r\n", codec.payload_type()),
        &format!("RTP/AVP {} 101\r\n", codec.payload_type()),
    );
    sdp.push_str("a=rtpmap:101 telephone-event/8000\r\na=fmtp:101 0-15\r\n");
    sdp.into_bytes()
}

/// Fixed fictional IVR cases, never caller-provided arbitrary dial strings.
#[derive(Clone, Copy, Debug)]
pub enum IvrCase {
    /// Three digits 123 select the PBX's 1 kHz success branch.
    Accepted,
    /// Three digits 124 select the PBX's 400 Hz rejection branch.
    Rejected,
}
impl IvrCase {
    fn digits(self) -> &'static str {
        match self {
            Self::Accepted => "123",
            Self::Rejected => "124",
        }
    }
    fn frequencies(self) -> (f64, f64) {
        match self {
            Self::Accepted => (1000.0, 400.0),
            Self::Rejected => (400.0, 1000.0),
        }
    }
}
/// Sanitized measurements from one completed fictional IVR call.
#[derive(Debug)]
pub struct IvrReport {
    /// Static SIP signaling label; all media in this fixture is plaintext UDP RTP.
    pub transport: &'static str,
    /// Static negotiated G.711 codec label.
    pub codec: &'static str,
    /// The fixed positive or negative IVR input case.
    pub case: IvrCase,
    /// Dynamic telephone-event payload number accepted in the SDP answer.
    pub event_payload_type: u8,
    /// Number of RFC 4733 event packets sent, including repeated end reports.
    pub event_packets_sent: u32,
    /// Valid new audio packets included in the two-second tone measurement.
    pub received_audio_packets: u32,
    /// RMS decoded amplitude; not audio samples.
    pub rms: f64,
    /// Expected IVR branch tone power relative to the opposite branch.
    pub expected_to_other_tone_ratio: f64,
    /// Successful ACK emission, BYE/200, exact dialog termination and unregister.
    /// In-process checks only, not an independent packet capture.
    pub signaling_completed: bool,
}

struct RtpSender {
    sequence: u16,
    clock: u32,
    ssrc: u32,
    tick: Interval,
    bytes: [u8; 172],
}
impl RtpSender {
    fn new() -> Self {
        let mut tick = tokio::time::interval(Duration::from_millis(20));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        Self {
            sequence: u16::MAX - 10,
            clock: u32::MAX - 1600,
            ssrc: 0x44544d46,
            tick,
            bytes: [0; 172],
        }
    }
    async fn send(
        &mut self,
        socket: &UdpSocket,
        target: SocketAddr,
        pt: u8,
        marker: bool,
        timestamp: u32,
        payload: &[u8],
    ) -> Result<(), CallProbeError> {
        self.tick.tick().await;
        let len = RtpHeader {
            marker,
            payload_type: pt,
            sequence: self.sequence,
            timestamp,
            ssrc: self.ssrc,
        }
        .write(payload, &mut self.bytes)
        .map_err(|_| CallProbeError::Media)?;
        if socket
            .send_to(&self.bytes[..len], target)
            .await
            .map_err(|_| CallProbeError::Media)?
            != len
        {
            return Err(CallProbeError::Media);
        }
        self.sequence = self.sequence.wrapping_add(1);
        self.clock = self.clock.wrapping_add(160);
        Ok(())
    }
    async fn silence(
        &mut self,
        socket: &UdpSocket,
        target: AudioTarget,
        frames: u8,
    ) -> Result<(), CallProbeError> {
        for _ in 0..frames {
            self.send(
                socket,
                target.address,
                target.codec.payload_type(),
                false,
                self.clock,
                &[target.codec.encode_sample(0); 160],
            )
            .await?;
        }
        Ok(())
    }
    async fn digit(
        &mut self,
        socket: &UdpSocket,
        target: AudioTarget,
        events: NegotiatedEvents,
        digit: Digit,
    ) -> Result<u32, CallProbeError> {
        if events.mask & (1u16 << digit.event_number()) == 0 {
            return Err(CallProbeError::UnsupportedDtmf);
        }
        let plan = Transmission::new(digit, 10, 960, 160).map_err(|_| CallProbeError::Media)?;
        let timestamp = self.clock;
        let mut count = 0;
        for packet in plan {
            self.send(
                socket,
                target.address,
                events.payload_type,
                packet.marker,
                timestamp,
                &packet.event.to_bytes(),
            )
            .await?;
            count += 1;
        }
        self.silence(socket, target, 3).await?;
        Ok(count)
    }
}
async fn measure_ivr(
    socket: &UdpSocket,
    target: AudioTarget,
    case: IvrCase,
) -> Result<(u32, f64, f64), CallProbeError> {
    let mut bytes = [0; 1501];
    // Discard at most 128 pre-branch datagrams, never drain an unbounded stream.
    let mut drained = false;
    for _ in 0..128 {
        match socket.try_recv_from(&mut bytes) {
            Ok((_, source)) if source == target.address => (),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                drained = true;
                break;
            }
            _ => return Err(CallProbeError::Media),
        }
    }
    if !drained {
        return Err(CallProbeError::Media);
    }
    let (expected, wrong) = case.frequencies();
    let mut meter = ToneMeter::new(wrong, expected);
    let mut tracker = PacketTracker::default();
    let mut received = 0;
    let end = Instant::now() + Duration::from_secs(2);
    loop {
        tokio::select! { biased;
            _=tokio::time::sleep_until(end)=>break,
            result=socket.recv_from(&mut bytes)=>{
                let (length,source)=result.map_err(|_|CallProbeError::Media)?;
                if source!=target.address {return Err(CallProbeError::Media);}
                let packet=RtpPacket::parse(&bytes[..length]).map_err(|_|CallProbeError::Media)?;
                if packet.header.payload_type!=target.codec.payload_type() || packet.payload.len()!=160 {return Err(CallProbeError::Media);}
                if tracker.observe(packet.header)?.is_none() {continue;}
                meter.observe(target.codec,packet.payload);
                received+=1;
            }
        }
    }
    if received < 80 {
        return Err(CallProbeError::Media);
    }
    let (rms, ratio) = meter.finish()?;
    Ok((received, rms, ratio))
}
async fn run_ivr(
    config: RegistrationConfig,
    codec: Codec,
    case: IvrCase,
    cancel: CancellationToken,
) -> Result<IvrReport, CallProbeError> {
    let label = config.transport.as_str();
    let pbx_ip = config.address.ip();
    let mut endpoint = LabEndpoint::connect(config, codec, cancel).await?;
    let caller: Uri = format!("sip:{}@lab.invalid", endpoint.username)
        .parse()
        .map_err(|_| CallProbeError::InvalidConfig)?;
    let callee: Uri = "sip:601@lab.invalid"
        .parse()
        .map_err(|_| CallProbeError::InvalidConfig)?;
    let option = InviteOption {
        caller,
        callee,
        contact: endpoint.contact.clone(),
        destination: Some(
            SipAddr::try_from(&endpoint.server).map_err(|_| CallProbeError::Signaling)?,
        ),
        credential: endpoint.registration.0.credential.clone(),
        content_type: Some("application/sdp".into()),
        offer: Some(dtmf_offer(
            endpoint
                .rtp
                .local_addr()
                .map_err(|_| CallProbeError::Media)?,
            codec,
        )),
        ..Default::default()
    };
    let (dialog, response) = endpoint
        .layer
        .do_invite(option, endpoint.states.clone())
        .await
        .map_err(|_| CallProbeError::Signaling)?;
    let response = response.ok_or(CallProbeError::Signaling)?;
    if response.status_code != StatusCode::OK {
        return Err(CallProbeError::Signaling);
    }
    let target = parse_offer(&response.body, codec, pbx_ip)?;
    let events = negotiated_events(&response.body, 101)?;
    endpoint.wait_phase(false).await?;
    endpoint.wire.wait(8).await;
    if !dialog.state().is_confirmed() {
        return Err(CallProbeError::Signaling);
    }
    // Check the complete sequence before sending even the first event.
    for symbol in case.digits().chars() {
        let digit = Digit::try_from(symbol).map_err(|_| CallProbeError::InvalidConfig)?;
        if events.mask & (1u16 << digit.event_number()) == 0 {
            return Err(CallProbeError::UnsupportedDtmf);
        }
    }
    let mut sender = RtpSender::new();
    sender.silence(&endpoint.rtp, target, 25).await?;
    let mut sent = 0;
    for symbol in case.digits().chars() {
        sent += sender
            .digit(
                &endpoint.rtp,
                target,
                events,
                Digit::try_from(symbol).map_err(|_| CallProbeError::InvalidConfig)?,
            )
            .await?;
    }
    let (received, rms, ratio) = measure_ivr(&endpoint.rtp, target, case).await?;
    dialog.bye().await.map_err(|_| CallProbeError::Signaling)?;
    endpoint.wire.wait(4).await;
    endpoint.wait_phase(true).await?;
    if !dialog.state().is_terminated() {
        return Err(CallProbeError::Signaling);
    }
    endpoint.unregister().await?;
    Ok(IvrReport {
        transport: label,
        codec: codec.name(),
        case,
        event_payload_type: events.payload_type,
        event_packets_sent: sent,
        received_audio_packets: received,
        rms,
        expected_to_other_tone_ratio: ratio,
        signaling_completed: true,
    })
}
/// Call the fixed fictional IVR with negotiated RTP DTMF, then verify its tone.
///
/// Only IPv4 1001/UDP, 1101/TCP and 1301/TLS plaintext-RTP fixtures are allowed.
/// No production configuration/device I/O, SIP INFO fallback or arbitrary
/// destination/digit input is accepted. Cancellation/total deadline is 30 s.
/// This is a short IVR proof, not hold/REFER/SRTP or full SIP-06 acceptance.
///
/// # Errors
/// Returns a sanitized [`CallProbeError`] on invalid fixture input, unavailable
/// telephone-event negotiation, wrong IVR tone, cancellation or signaling failure.
pub async fn probe_ivr(
    config: RegistrationConfig,
    codec: Codec,
    case: IvrCase,
    cancel: CancellationToken,
) -> Result<IvrReport, CallProbeError> {
    if cancel.is_cancelled() {
        return Err(CallProbeError::Cancelled);
    }
    let valid = match &config.transport {
        Transport::Udp => config.username == "1001",
        Transport::Tcp => config.username == "1101",
        Transport::Tls { .. } => config.username == "1301",
    };
    if !valid || !config.address.is_ipv4() {
        return Err(CallProbeError::InvalidConfig);
    }
    let operation = run_ivr(config, codec, case, cancel.child_token());
    tokio::select! {biased;
        _=cancel.cancelled()=>Err(CallProbeError::Cancelled),
        result=tokio::time::timeout(Duration::from_secs(30),operation)=>result.map_err(|_|CallProbeError::Timeout)?,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn admission_and_precancellation_fail_before_network()
    -> Result<(), Box<dyn std::error::Error>> {
        let config = |user: &str, transport| {
            crate::RegistrationConfig::new(
                SocketAddr::from(([127, 0, 0, 1], 5060)),
                transport,
                user.into(),
                secrecy::SecretString::from("fictional"),
                Duration::from_secs(2),
            )
        };
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert!(matches!(
            probe_ivr(
                config("1001", Transport::Udp)?,
                Codec::Pcma,
                IvrCase::Accepted,
                cancelled
            )
            .await,
            Err(CallProbeError::Cancelled)
        ));
        let tls = Transport::Tls {
            ca_pem: vec![1],
            server_name: "pbx".into(),
        };
        assert!(matches!(
            probe_ivr(
                config("1201", tls)?,
                Codec::Pcmu,
                IvrCase::Accepted,
                CancellationToken::new()
            )
            .await,
            Err(CallProbeError::InvalidConfig)
        ));
        Ok(())
    }

    #[tokio::test]
    async fn actual_sender_shares_sequence_clock_ssrc_and_repeats_end_reports()
    -> Result<(), Box<dyn std::error::Error>> {
        use rust_microsip_media::dtmf::TelephoneEvent;
        let receiver = UdpSocket::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
        let sender = UdpSocket::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
        let target = AudioTarget {
            address: receiver.local_addr()?,
            codec: Codec::Pcma,
        };
        let mut source = RtpSender::new();
        let (sent, received) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(
                async {
                    source.silence(&sender, target, 1).await?;
                    source
                        .digit(
                            &sender,
                            target,
                            NegotiatedEvents {
                                payload_type: 111,
                                mask: u16::MAX,
                            },
                            Digit::try_from('1').map_err(|_| CallProbeError::Media)?,
                        )
                        .await
                },
                async {
                    let mut packets = Vec::new();
                    for _ in 0..12 {
                        let mut bytes = [0; 172];
                        let (n, _) = receiver
                            .recv_from(&mut bytes)
                            .await
                            .map_err(|_| CallProbeError::Media)?;
                        let packet =
                            RtpPacket::parse(&bytes[..n]).map_err(|_| CallProbeError::Media)?;
                        let event = if packet.header.payload_type == 111 {
                            Some(
                                TelephoneEvent::parse(packet.payload)
                                    .map_err(|_| CallProbeError::Media)?,
                            )
                        } else {
                            None
                        };
                        packets.push((packet.header, event));
                    }
                    Ok::<_, CallProbeError>(packets)
                }
            )
        })
        .await?;
        assert_eq!(sent?, 8);
        let packets = received?;
        for pair in packets.windows(2) {
            assert_eq!(pair[1].0.sequence, pair[0].0.sequence.wrapping_add(1));
            assert_eq!(pair[1].0.ssrc, pair[0].0.ssrc);
        }
        assert_eq!(packets[11].0.sequence, 0);
        assert_eq!(packets[11].0.timestamp, 159);
        for (i, (header, event)) in packets[1..9].iter().enumerate() {
            assert_eq!(header.payload_type, 111);
            assert_eq!(header.timestamp, packets[1].0.timestamp);
            assert_eq!(header.marker, i == 0);
            assert_eq!(event.map(|e| e.is_end()), Some(i >= 5));
        }
        assert_eq!(
            packets[9].0.timestamp,
            packets[1].0.timestamp.wrapping_add(1280)
        );
        Ok(())
    }

    #[test]
    fn telephone_event_requires_offered_dynamic_pt_and_8khz_mapping() -> Result<(), CallProbeError>
    {
        let sdp = b"m=audio 10000 RTP/AVP 8 111\r\na=rtpmap:111 telephone-event/8000\r\na=fmtp:111 0-15\r\n";
        let event = negotiated_events(sdp, 111)?;
        assert_eq!(event.payload_type, 111);
        assert_eq!(event.mask, u16::MAX);
        for bad in [
            b"m=audio 10000 RTP/AVP 8\r\na=rtpmap:111 telephone-event/8000\r\n".as_slice(),
            b"m=audio 10000 RTP/AVP 8 111\r\na=rtpmap:111 telephone-event/16000\r\n".as_slice(),
            b"m=audio 10000 RTP/AVP 8 111\r\n".as_slice(),
            b"m=audio 10000 RTP/AVP 8 111\r\na=rtpmap:111 telephone-event/8000\r\na=fmtp:111 15-0\r\n".as_slice(),
        ] {
            assert!(negotiated_events(bad, 111).is_err());
        }
        assert!(negotiated_events(sdp, 101).is_err());
        assert!(negotiated_events(&[b'x'; 8193], 111).is_err());
        Ok(())
    }
    #[test]
    fn absent_fmtp_defaults_to_dtmf_only_and_explicit_mask_is_restricted()
    -> Result<(), CallProbeError> {
        let sdp = b"m=audio 10000 RTP/AVP 0 101\r\na=rtpmap:101 telephone-event/8000\r\n";
        assert_eq!(negotiated_events(sdp, 101)?.mask, u16::MAX);
        assert_eq!(parse_mask("1-3,10,16")?, 0b10000001110);
        for bad in ["", "1,", "-1", "16-1", "256", "0-9999", "1, 2"] {
            assert!(parse_mask(bad).is_err());
        }
        Ok(())
    }
}
