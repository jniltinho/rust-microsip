//! Isolated SDP hold/resume evaluation; not the production call actor.
use super::{
    AudioTarget, CallProbeError, LabEndpoint, PacketTracker, ToneMeter, offer, parse_offer,
};
use crate::{RegistrationConfig, Transport};
use rsipstack::{
    dialog::invitation::InviteOption,
    sip::{Header, HeadersExt, Method, SipMessage, StatusCode, Uri},
    transport::SipAddr,
};
use rust_microsip_media::{Codec, RtpHeader, RtpPacket};
use std::{
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    net::UdpSocket,
    sync::{Notify, watch},
    time::{Instant, Interval, MissedTickBehavior},
};
use tokio_util::sync::CancellationToken;

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
    fn answer(self) -> Self {
        match self {
            Self::SendOnly => Self::RecvOnly,
            Self::RecvOnly => Self::SendOnly,
            x => x,
        }
    }
    fn can_send(self) -> bool {
        matches!(self, Self::SendRecv | Self::SendOnly)
    }
}
fn control_offer(
    addr: SocketAddr,
    codec: Codec,
    version: u64,
    flow: Flow,
) -> Result<Vec<u8>, CallProbeError> {
    if version == 0 {
        return Err(CallProbeError::UnsupportedSdp);
    }
    let text = String::from_utf8(offer(addr, codec)).map_err(|_| CallProbeError::UnsupportedSdp)?;
    Ok(text
        .replace(
            "o=rust-microsip 1 1 ",
            &format!("o=rust-microsip 1 {version} "),
        )
        .replace("a=sendrecv\r\n", &format!("a={}\r\n", flow.attribute()))
        .into_bytes())
}
fn parse_control(
    bytes: &[u8],
    codec: Codec,
    pbx: IpAddr,
) -> Result<(AudioTarget, Flow), CallProbeError> {
    if bytes.is_empty() || bytes.len() > 8192 {
        return Err(CallProbeError::UnsupportedSdp);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| CallProbeError::UnsupportedSdp)?;
    let mut flow = None;
    let mut base = String::with_capacity(bytes.len());
    for (i, line) in text.lines().enumerate() {
        if i >= 128 || line.len() > 512 || line.contains('\0') {
            return Err(CallProbeError::UnsupportedSdp);
        }
        let direction = match line {
            "a=sendrecv" => Some(Flow::SendRecv),
            "a=sendonly" => Some(Flow::SendOnly),
            "a=recvonly" => Some(Flow::RecvOnly),
            "a=inactive" => Some(Flow::Inactive),
            _ => None,
        };
        if let Some(value) = direction {
            if flow.replace(value).is_some() {
                return Err(CallProbeError::UnsupportedSdp);
            }
        } else {
            base.push_str(line);
            base.push_str("\r\n");
        }
    }
    Ok((
        parse_offer(base.as_bytes(), codec, pbx)?,
        flow.unwrap_or(Flow::SendRecv),
    ))
}
pub(super) fn update_answer(
    bytes: &[u8],
    codec: Codec,
    pbx: IpAddr,
    local: SocketAddr,
    version: u64,
) -> Result<(Vec<u8>, Flow), CallProbeError> {
    let (_, remote) = parse_control(bytes, codec, pbx)?;
    let local_flow = remote.answer();
    Ok((
        control_offer(local, codec, version, local_flow)?,
        local_flow,
    ))
}

#[derive(Default)]
struct WireState {
    call_id: Option<String>,
    last_invite: u32,
    last_ack: u32,
    invites: u8,
    failed: bool,
}
#[derive(Default)]
pub(super) struct Wire {
    state: Mutex<WireState>,
    changed: Notify,
}
impl Wire {
    fn bind(&self, id: &str) -> Result<(), CallProbeError> {
        if id.is_empty() || id.len() > 256 {
            return Err(CallProbeError::Signaling);
        }
        let mut state = self.state.lock().map_err(|_| CallProbeError::Signaling)?;
        if state.call_id.is_some() {
            return Err(CallProbeError::Signaling);
        }
        state.call_id = Some(id.into());
        Ok(())
    }
    pub(super) fn sent(&self, msg: &SipMessage) {
        let SipMessage::Request(r) = msg else {
            return;
        };
        if !matches!(r.method, Method::Invite | Method::Ack) {
            return;
        }
        let Ok(mut s) = self.state.lock() else {
            return;
        };
        let Some(id) = &s.call_id else {
            return;
        };
        if r.call_id_header().ok().map(|h| h.value()) != Some(id.as_str()) {
            return;
        }
        let seq = r
            .cseq_header()
            .ok()
            .and_then(|h| rsipstack::sip::typed::CSeq::parse(h.value()).ok())
            .filter(|h| h.method == r.method)
            .map(|h| h.seq);
        let Some(seq) = seq.filter(|n| *n > 0) else {
            s.failed = true;
            self.changed.notify_one();
            return;
        };
        if r.method == Method::Invite && seq > s.last_invite {
            s.invites = s.invites.saturating_add(1);
            s.failed |= s.invites > 16;
            s.last_invite = seq;
        } else if r.method == Method::Ack && seq == s.last_invite {
            s.last_ack = seq;
        }
        self.changed.notify_one();
    }
    async fn verify(&self, response: &rsipstack::sip::Response) -> Result<u32, CallProbeError> {
        let seq = rsipstack::sip::typed::CSeq::parse(
            response
                .cseq_header()
                .map_err(|_| CallProbeError::Signaling)?
                .value(),
        )
        .map_err(|_| CallProbeError::Signaling)?;
        if seq.method != Method::Invite {
            return Err(CallProbeError::Signaling);
        }
        loop {
            let notified = self.changed.notified();
            {
                let s = self.state.lock().map_err(|_| CallProbeError::Signaling)?;
                if s.failed
                    || s.last_invite != seq.seq
                    || response.call_id_header().ok().map(|h| h.value()) != s.call_id.as_deref()
                {
                    return Err(CallProbeError::Signaling);
                }
                if s.last_ack == seq.seq {
                    return Ok(seq.seq);
                }
            }
            notified.await;
        }
    }
}

/// Bounded aggregate metrics from one synthetic media window; no samples/IDs.
#[derive(Clone, Copy, Debug)]
pub struct Window {
    /// Successfully emitted G.711 frames in this window.
    pub sent_packets: u32,
    /// Accepted audio frames after the window's settling interval.
    pub received_packets: u32,
    /// Decoded PCM RMS, or zero when no frames arrived.
    pub rms: f64,
    /// Expected remote-tone power relative to the local source tone.
    pub remote_to_own_power_ratio: f64,
    /// Estimated peak amplitude of the synthetic remote-frequency component.
    pub remote_tone_amplitude: f64,
}
struct Stream {
    socket: Arc<UdpSocket>,
    target: AudioTarget,
    flow: watch::Receiver<Flow>,
    tick: Interval,
    sequence: u16,
    clock: u32,
    ssrc: u32,
    sample: u32,
    hz: f64,
    remote_hz: f64,
}
impl Stream {
    fn new(
        endpoint: &LabEndpoint,
        target: AudioTarget,
        hz: f64,
        remote_hz: f64,
        ssrc: u32,
    ) -> Self {
        let mut tick = tokio::time::interval(Duration::from_millis(20));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        Self {
            socket: endpoint.rtp.clone(),
            target,
            flow: endpoint.flow.clone(),
            tick,
            sequence: 65500,
            clock: u32::MAX - 3200,
            ssrc,
            sample: 0,
            hz,
            remote_hz,
        }
    }
    async fn window(
        &mut self,
        duration: Duration,
        local_send: bool,
        voice: bool,
        settle: Duration,
    ) -> Result<Window, CallProbeError> {
        let start = Instant::now();
        let end = start + duration;
        let mut sent_packets = 0;
        let mut received_packets = 0;
        let mut send = [0; 172];
        let mut recv = [0; 1501];
        let mut tracker = PacketTracker::default();
        let mut meter = ToneMeter::new(self.hz, self.remote_hz);
        // Bounded discard of queued frames from the previous negotiated phase.
        let mut drained = false;
        for _ in 0..128 {
            match self.socket.try_recv_from(&mut recv) {
                Ok((_, source)) if source == self.target.address => (),
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
        loop {
            tokio::select! {biased;
                _=tokio::time::sleep_until(end)=>break,
                _=self.tick.tick()=>{
                    // Advance each measured audio slot, including muted slots; signaling gaps are not clocked.
                    let timestamp=self.clock;self.clock=self.clock.wrapping_add(160);
                    if !local_send||!self.flow.borrow().can_send(){continue;}
                    let mut payload=[0;160];
                    for octet in &mut payload {let phase=std::f64::consts::TAU*self.hz*f64::from(self.sample%8000)/8000.0;*octet=self.target.codec.encode_sample((8000.0*phase.sin()) as i16);self.sample=self.sample.wrapping_add(1);}
                    let n=RtpHeader{marker:sent_packets==0,payload_type:self.target.codec.payload_type(),sequence:self.sequence,timestamp,ssrc:self.ssrc}.write(&payload,&mut send).map_err(|_|CallProbeError::Media)?;
                    if self.socket.send_to(&send[..n],self.target.address).await.map_err(|_|CallProbeError::Media)?!=n{return Err(CallProbeError::Media);}
                    self.sequence=self.sequence.wrapping_add(1);sent_packets+=1;
                }
                value=self.socket.recv_from(&mut recv)=>{
                    let (n,source)=value.map_err(|_|CallProbeError::Media)?;if source!=self.target.address{return Err(CallProbeError::Media);}
                    let packet=RtpPacket::parse(&recv[..n]).map_err(|_|CallProbeError::Media)?;
                    if packet.header.payload_type!=self.target.codec.payload_type()||packet.payload.len()!=160{return Err(CallProbeError::Media);}
                    if start.elapsed()<settle{continue;}
                    if tracker.observe(packet.header)?.is_some(){received_packets+=1;meter.observe(self.target.codec,packet.payload);}
                }
            }
        }
        let (rms, ratio) = if voice {
            if sent_packets < 80 || received_packets < 80 {
                return Err(CallProbeError::Media);
            }
            meter.finish()?
        } else if meter.samples == 0 {
            (0.0, 0.0)
        } else {
            (
                (meter.energy / f64::from(meter.samples)).sqrt(),
                meter.wanted / meter.echo.max(1.0),
            )
        };
        let amplitude = if meter.samples == 0 {
            0.0
        } else {
            2.0 * (meter.wanted / (f64::from(meter.samples) / 160.0)).sqrt() / 160.0
        };
        Ok(Window {
            sent_packets,
            received_packets,
            rms,
            remote_to_own_power_ratio: ratio,
            remote_tone_amplitude: amplitude,
        })
    }
}

/// Sanitized successful synthetic hold/resume and rejected-offer evidence.
#[derive(Debug)]
pub struct HoldReport {
    /// SIP transport label; all RTP here is explicitly plaintext UDP.
    pub transport: &'static str,
    /// Negotiated G.711 codec label.
    pub codec: &'static str,
    /// Second fictional endpoint originated and holds the call when true.
    pub reverse: bool,
    /// Caller and callee pre-hold voice-tone measurements.
    pub before: [Window; 2],
    /// Caller and callee during hold, after 250 ms settling for reception.
    pub held: [Window; 2],
    /// Caller and callee after successful resume.
    pub resumed: [Window; 2],
    /// Caller and callee after an incompatible re-INVITE was rejected.
    pub after_rejection: [Window; 2],
    /// Hold/resume/rejected re-INVITE CSeqs with matching emitted ACKs.
    pub ack_emitted_cseqs: [u32; 3],
    /// Non-success final code for the incompatible codec offer (488).
    pub rejected_status: u16,
    /// Both exact dialogs ended, BYE/200 and both unregisters completed.
    pub signaling_completed: bool,
}
async fn pair(
    a: &mut Stream,
    b: &mut Stream,
    voice: bool,
    send_a: bool,
) -> Result<[Window; 2], CallProbeError> {
    let duration = Duration::from_secs(if voice { 2 } else { 1 });
    let settle = if voice {
        Duration::ZERO
    } else {
        Duration::from_millis(250)
    };
    let (a, b) = tokio::join!(
        a.window(duration, send_a, voice, settle),
        b.window(duration, true, voice, settle)
    );
    Ok([a?, b?])
}
fn validate_hold(held: &[Window; 2]) -> Result<(), CallProbeError> {
    if held[0].sent_packets != 0
        || held[0].received_packets != 0
        || held[1].sent_packets < 40
        || held[1].remote_tone_amplitude >= 100.0
    {
        return Err(CallProbeError::Media);
    }
    Ok(())
}
async fn run(
    first: RegistrationConfig,
    second: RegistrationConfig,
    codec: Codec,
    reverse: bool,
    stop: CancellationToken,
) -> Result<HoldReport, CallProbeError> {
    let label = first.transport.as_str();
    let pbx = first.address.ip();
    let mut a = LabEndpoint::connect_with_updates(first, codec, stop.clone(), true).await?;
    let mut b = LabEndpoint::connect_with_updates(second, codec, stop, true).await?;
    let (caller, callee) = if reverse {
        (&mut b, &mut a)
    } else {
        (&mut a, &mut b)
    };
    let addr = caller.rtp.local_addr().map_err(|_| CallProbeError::Media)?;
    let option = InviteOption {
        caller: format!("sip:{}@lab.invalid", caller.username)
            .parse::<Uri>()
            .map_err(|_| CallProbeError::InvalidConfig)?,
        callee: format!("sip:{}@lab.invalid", callee.username)
            .parse::<Uri>()
            .map_err(|_| CallProbeError::InvalidConfig)?,
        contact: caller.contact.clone(),
        destination: Some(
            SipAddr::try_from(&caller.server).map_err(|_| CallProbeError::Signaling)?,
        ),
        credential: caller.registration.0.credential.clone(),
        content_type: Some("application/sdp".into()),
        offer: Some(offer(addr, codec)),
        ..Default::default()
    };
    let (dialog, response) = caller
        .layer
        .do_invite(option, caller.states.clone())
        .await
        .map_err(|_| CallProbeError::Signaling)?;
    let response = response.ok_or(CallProbeError::Signaling)?;
    if response.status_code != StatusCode::OK {
        return Err(CallProbeError::Signaling);
    }
    let remote = parse_offer(&response.body, codec, pbx)?;
    let incoming = callee
        .incoming
        .recv()
        .await
        .ok_or(CallProbeError::Signaling)?;
    caller.wait_phase(false).await?;
    callee.wait_phase(false).await?;
    callee.wire.wait(1).await;
    if !dialog.state().is_confirmed() || !incoming.dialog.state().is_confirmed() {
        return Err(CallProbeError::Signaling);
    }
    let id = dialog.id();
    let other_id = incoming.dialog.id();
    caller.hold_wire.bind(&id.call_id)?;
    let (own, other) = if reverse {
        (880.0, 440.0)
    } else {
        (440.0, 880.0)
    };
    let mut source = Stream::new(caller, remote, own, other, 0x484f4c44);
    let mut peer = Stream::new(callee, incoming.target, other, own, 0x50454552);
    let before = pair(&mut source, &mut peer, true, true).await?;
    let headers = || Some(vec![Header::ContentType("application/sdp".into())]);
    let hold = dialog
        .reinvite(
            headers(),
            Some(control_offer(addr, codec, 2, Flow::SendOnly)?),
        )
        .await
        .map_err(|_| CallProbeError::Signaling)?
        .ok_or(CallProbeError::Signaling)?;
    if hold.status_code != StatusCode::OK {
        return Err(CallProbeError::Signaling);
    }
    let (hold_target, hold_flow) = parse_control(&hold.body, codec, pbx)?;
    if hold_flow != Flow::RecvOnly || hold_target.address != source.target.address {
        return Err(CallProbeError::UnsupportedSdp);
    }
    let hold_seq = caller.hold_wire.verify(&hold).await?;
    let held = pair(&mut source, &mut peer, false, false).await?;
    validate_hold(&held)?;
    let resume = dialog
        .reinvite(
            headers(),
            Some(control_offer(addr, codec, 3, Flow::SendRecv)?),
        )
        .await
        .map_err(|_| CallProbeError::Signaling)?
        .ok_or(CallProbeError::Signaling)?;
    if resume.status_code != StatusCode::OK {
        return Err(CallProbeError::Signaling);
    }
    let (target, flow) = parse_control(&resume.body, codec, pbx)?;
    if flow != Flow::SendRecv || target.address != source.target.address {
        return Err(CallProbeError::UnsupportedSdp);
    }
    let resume_seq = caller.hold_wire.verify(&resume).await?;
    let resumed = pair(&mut source, &mut peer, true, true).await?;
    let mut bad = String::from_utf8(control_offer(addr, codec, 4, Flow::SendRecv)?)
        .map_err(|_| CallProbeError::UnsupportedSdp)?;
    bad = bad
        .replace(
            &format!("RTP/AVP {}\r\n", codec.payload_type()),
            "RTP/AVP 127\r\n",
        )
        .replace(
            &format!("a=rtpmap:{} {}/8000", codec.payload_type(), codec.name()),
            "a=rtpmap:127 X-UNSUPPORTED/8000",
        );
    let rejection = dialog
        .reinvite(headers(), Some(bad.into_bytes()))
        .await
        .map_err(|_| CallProbeError::Signaling)?
        .ok_or(CallProbeError::Signaling)?;
    if rejection.status_code != StatusCode::NotAcceptableHere {
        return Err(CallProbeError::Signaling);
    }
    let rejection_seq = caller.hold_wire.verify(&rejection).await?;
    if !(hold_seq < resume_seq && resume_seq < rejection_seq)
        || dialog.id() != id
        || incoming.dialog.id() != other_id
        || !dialog.state().is_confirmed()
        || !incoming.dialog.state().is_confirmed()
    {
        return Err(CallProbeError::Signaling);
    }
    let after_rejection = pair(&mut source, &mut peer, true, true).await?;
    dialog.bye().await.map_err(|_| CallProbeError::Signaling)?;
    caller.wire.wait(4).await;
    callee.wire.wait(2).await;
    caller.wait_phase(true).await?;
    callee.wait_phase(true).await?;
    if !dialog.state().is_terminated() || !incoming.dialog.state().is_terminated() {
        return Err(CallProbeError::Signaling);
    }
    caller.unregister().await?;
    callee.unregister().await?;
    Ok(HoldReport {
        transport: label,
        codec: codec.name(),
        reverse,
        before,
        held,
        resumed,
        after_rejection,
        ack_emitted_cseqs: [hold_seq, resume_seq, rejection_seq],
        rejected_status: 488,
        signaling_completed: true,
    })
}
/// Verify one fixed two-endpoint IPv4 plain-RTP hold/resume call in ≤30 seconds.
///
/// Only fictional 1001/1002 UDP, 1101/1102 TCP and 1301/1302 TLS pairs with the
/// same explicit registrar are admitted. SIP TLS validates CA/SNI but RTP is
/// plaintext. No production settings/devices, arbitrary dialed URI or SRTP
/// downgrade is supported. Credentials/parser/channel/failure cleanup gates
/// in ADR 0004 remain; this is synthetic evidence, not human release acceptance.
///
/// # Errors
/// Returns sanitized [`CallProbeError`] on invalid input, negotiation/media,
/// missing ACK/termination, cancellation or total deadline expiry.
pub async fn probe_hold_pair(
    first: RegistrationConfig,
    second: RegistrationConfig,
    codec: Codec,
    reverse: bool,
    cancel: CancellationToken,
) -> Result<HoldReport, CallProbeError> {
    if cancel.is_cancelled() {
        return Err(CallProbeError::Cancelled);
    }
    let users = match &first.transport {
        Transport::Udp => ("1001", "1002"),
        Transport::Tcp => ("1101", "1102"),
        Transport::Tls { .. } => ("1301", "1302"),
    };
    if !first.address.is_ipv4()
        || first.address != second.address
        || first.transport.as_str() != second.transport.as_str()
        || first.username != users.0
        || second.username != users.1
    {
        return Err(CallProbeError::InvalidConfig);
    }
    tokio::select! {biased;_=cancel.cancelled()=>Err(CallProbeError::Cancelled),result=tokio::time::timeout(Duration::from_secs(30),run(first,second,codec,reverse,cancel.child_token()))=>result.map_err(|_|CallProbeError::Timeout)?,}
}

#[cfg(test)]
mod tests {
    use super::super::{CallProbeError, offer};
    use super::*;
    use rust_microsip_media::Codec;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    #[test]
    fn control_sdp_requires_one_direction_and_preserves_anchored_g711() -> Result<(), CallProbeError>
    {
        let pbx = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1));
        let addr = SocketAddr::new(pbx, 10000);
        for flow in [
            Flow::SendRecv,
            Flow::SendOnly,
            Flow::RecvOnly,
            Flow::Inactive,
        ] {
            let bytes = control_offer(addr, Codec::Pcma, 2, flow)?;
            let (target, parsed) = parse_control(&bytes, Codec::Pcma, pbx)?;
            assert_eq!(parsed, flow);
            assert_eq!(target.address, addr);
        }
        let mut duplicate = offer(addr, Codec::Pcma);
        duplicate.extend_from_slice(b"a=sendonly\r\n");
        assert!(parse_control(&duplicate, Codec::Pcma, pbx).is_err());
        let zero = SocketAddr::from(([0, 0, 0, 0], 10000));
        assert!(parse_control(&offer(zero, Codec::Pcma), Codec::Pcma, pbx).is_err());
        assert!(control_offer(addr, Codec::Pcma, 0, Flow::SendRecv).is_err());
        assert!(parse_control(&[b'x'; 8193], Codec::Pcma, pbx).is_err());
        Ok(())
    }
    #[test]
    fn changed_offers_increment_origin_version_without_replacing_session_identity()
    -> Result<(), CallProbeError> {
        let addr = SocketAddr::from(([192, 0, 2, 2], 20000));
        let hold = String::from_utf8(control_offer(addr, Codec::Pcmu, 2, Flow::SendOnly)?)
            .map_err(|_| CallProbeError::UnsupportedSdp)?;
        let resume = String::from_utf8(control_offer(addr, Codec::Pcmu, 3, Flow::SendRecv)?)
            .map_err(|_| CallProbeError::UnsupportedSdp)?;
        assert!(hold.contains("o=rust-microsip 1 2 IN IP4 192.0.2.2\r\n"));
        assert!(resume.contains("o=rust-microsip 1 3 IN IP4 192.0.2.2\r\n"));
        assert_eq!(Flow::SendOnly.answer(), Flow::RecvOnly);
        assert_eq!(Flow::RecvOnly.answer(), Flow::SendOnly);
        assert_eq!(Flow::Inactive.answer(), Flow::Inactive);
        Ok(())
    }
    fn request(
        method: &str,
        cseq_method: &str,
        id: &str,
        seq: u32,
    ) -> Result<SipMessage, CallProbeError> {
        SipMessage::try_from(format!("{method} sip:1002@lab.invalid SIP/2.0\r\nVia: SIP/2.0/UDP 192.0.2.1:5060;branch=z9hG4bK-fixture\r\nFrom: <sip:1001@lab.invalid>;tag=a\r\nTo: <sip:1002@lab.invalid>;tag=b\r\nCall-ID: {id}\r\nCSeq: {seq} {cseq_method}\r\nContent-Length: 0\r\n\r\n").as_str()).map_err(|_|CallProbeError::Signaling)
    }
    #[test]
    fn ack_evidence_rejects_a_mismatched_cseq_method() -> Result<(), CallProbeError> {
        let wire = Wire::default();
        wire.bind("fixture")?;
        wire.sent(&request("INVITE", "INVITE", "fixture", 2)?);
        wire.sent(&request("ACK", "INVITE", "fixture", 2)?);
        let state = wire.state.lock().map_err(|_| CallProbeError::Signaling)?;
        assert_ne!(state.last_ack, 2);
        assert!(state.failed);
        Ok(())
    }
    #[test]
    fn ack_evidence_binds_call_id_cseq_and_caps_new_invites() -> Result<(), CallProbeError> {
        let wire = Wire::default();
        wire.bind("fixture")?;
        wire.sent(&request("INVITE", "INVITE", "fixture", 2)?);
        wire.sent(&request("ACK", "ACK", "different", 2)?);
        wire.sent(&request("ACK", "ACK", "fixture", 1)?);
        assert_eq!(
            wire.state
                .lock()
                .map_err(|_| CallProbeError::Signaling)?
                .last_ack,
            0
        );
        wire.sent(&request("ACK", "ACK", "fixture", 2)?);
        assert_eq!(
            wire.state
                .lock()
                .map_err(|_| CallProbeError::Signaling)?
                .last_ack,
            2
        );
        for seq in 3..=18 {
            wire.sent(&request("INVITE", "INVITE", "fixture", seq)?);
        }
        assert!(
            wire.state
                .lock()
                .map_err(|_| CallProbeError::Signaling)?
                .failed
        );
        Ok(())
    }

    #[tokio::test]
    async fn mandatory_srtp_and_precancelled_calls_fail_before_network()
    -> Result<(), Box<dyn std::error::Error>> {
        let config = |user: &str| {
            crate::RegistrationConfig::new(
                SocketAddr::from(([127, 0, 0, 1], 5060)),
                Transport::Udp,
                user.into(),
                secrecy::SecretString::from("fictional"),
                Duration::from_secs(2),
            )
        };
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(matches!(
            probe_hold_pair(config("1001")?, config("1002")?, Codec::Pcma, false, cancel).await,
            Err(CallProbeError::Cancelled)
        ));
        let secure = |user: &str| {
            crate::RegistrationConfig::new(
                SocketAddr::from(([127, 0, 0, 1], 5061)),
                Transport::Tls {
                    ca_pem: vec![1],
                    server_name: "pbx".into(),
                },
                user.into(),
                secrecy::SecretString::from("fictional"),
                Duration::from_secs(2),
            )
        };
        assert!(matches!(
            probe_hold_pair(
                secure("1201")?,
                secure("1202")?,
                Codec::Pcma,
                false,
                CancellationToken::new()
            )
            .await,
            Err(CallProbeError::InvalidConfig)
        ));
        Ok(())
    }
    fn response(id: &str, seq: u32) -> Result<rsipstack::sip::Response, CallProbeError> {
        let message = SipMessage::try_from(format!("SIP/2.0 200 OK\r\nVia: SIP/2.0/UDP 192.0.2.1:5060;branch=z9hG4bK-fixture\r\nFrom: <sip:1001@lab.invalid>;tag=a\r\nTo: <sip:1002@lab.invalid>;tag=b\r\nCall-ID: {id}\r\nCSeq: {seq} INVITE\r\nContent-Length: 0\r\n\r\n").as_str()).map_err(|_|CallProbeError::Signaling)?;
        match message {
            SipMessage::Response(value) => Ok(value),
            _ => Err(CallProbeError::Signaling),
        }
    }
    // Signal readiness only after verify has actually registered and returned Pending.
    async fn waiting_verifier(
        wire: Arc<Wire>,
    ) -> Result<tokio::task::JoinHandle<Result<u32, CallProbeError>>, Box<dyn std::error::Error>>
    {
        use std::{
            future::{Future, poll_fn},
            pin::pin,
        };
        let response = response("fixture", 2)?;
        let (ready, started) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let mut ready = Some(ready);
            let mut verifier = pin!(wire.verify(&response));
            poll_fn(|cx| {
                let value = verifier.as_mut().poll(cx);
                if value.is_pending()
                    && let Some(ready) = ready.take()
                {
                    let _ = ready.send(());
                }
                value
            })
            .await
        });
        match tokio::time::timeout(Duration::from_secs(1), started).await {
            Ok(Ok(())) => Ok(task),
            _ => {
                task.abort();
                Err("verifier did not register a wait".into())
            }
        }
    }
    #[tokio::test]
    async fn malformed_ack_wakes_a_blocked_verifier_with_signaling_error()
    -> Result<(), Box<dyn std::error::Error>> {
        let wire = Arc::new(Wire::default());
        wire.bind("fixture")?;
        wire.sent(&request("INVITE", "INVITE", "fixture", 2)?);
        wire.changed.notified().await;
        let mut waiter = waiting_verifier(wire.clone()).await?;
        wire.sent(&request("ACK", "INVITE", "fixture", 2)?);
        let result = tokio::time::timeout(Duration::from_secs(1), &mut waiter).await;
        if result.is_err() {
            waiter.abort();
        }
        assert_eq!(result??, Err(CallProbeError::Signaling));
        Ok(())
    }
    #[tokio::test]
    async fn matching_ack_wakes_a_blocked_verifier_with_exact_cseq()
    -> Result<(), Box<dyn std::error::Error>> {
        let wire = Arc::new(Wire::default());
        wire.bind("fixture")?;
        wire.sent(&request("INVITE", "INVITE", "fixture", 2)?);
        wire.changed.notified().await;
        let mut waiter = waiting_verifier(wire.clone()).await?;
        wire.sent(&request("ACK", "ACK", "fixture", 2)?);
        let result = tokio::time::timeout(Duration::from_secs(1), &mut waiter).await;
        if result.is_err() {
            waiter.abort();
        }
        assert_eq!(result??, Ok(2));
        Ok(())
    }
    #[tokio::test]
    async fn response_with_different_call_id_fails_verification()
    -> Result<(), Box<dyn std::error::Error>> {
        let wire = Wire::default();
        wire.bind("fixture")?;
        wire.sent(&request("INVITE", "INVITE", "fixture", 2)?);
        assert_eq!(
            wire.verify(&response("different", 2)?).await,
            Err(CallProbeError::Signaling)
        );
        Ok(())
    }
    #[test]
    fn hold_requires_no_pbx_audio_to_holder_and_an_actively_sending_peer() {
        let window = Window {
            sent_packets: 0,
            received_packets: 0,
            rms: 0.0,
            remote_to_own_power_ratio: 0.0,
            remote_tone_amplitude: 0.0,
        };
        let mut held = [
            window,
            Window {
                sent_packets: 40,
                ..window
            },
        ];
        assert_eq!(validate_hold(&held), Ok(()));
        held[0].received_packets = 1;
        assert_eq!(validate_hold(&held), Err(CallProbeError::Media));
        held[0].received_packets = 0;
        held[1].sent_packets = 39;
        assert_eq!(validate_hold(&held), Err(CallProbeError::Media));
    }
    #[test]
    fn incoming_update_answers_complement_direction_version_and_reject_bad_codec()
    -> Result<(), CallProbeError> {
        let pbx = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1));
        let remote = SocketAddr::new(pbx, 10000);
        let local = SocketAddr::from(([192, 0, 2, 2], 20000));
        let (body, flow) = update_answer(
            &control_offer(remote, Codec::Pcma, 2, Flow::SendOnly)?,
            Codec::Pcma,
            pbx,
            local,
            2,
        )?;
        assert_eq!(flow, Flow::RecvOnly);
        assert!(
            std::str::from_utf8(&body)
                .map_err(|_| CallProbeError::UnsupportedSdp)?
                .contains("o=rust-microsip 1 2 IN IP4 192.0.2.2\r\n")
        );
        let (body, flow) = update_answer(
            &control_offer(remote, Codec::Pcma, 3, Flow::SendRecv)?,
            Codec::Pcma,
            pbx,
            local,
            3,
        )?;
        assert_eq!(flow, Flow::SendRecv);
        assert!(
            std::str::from_utf8(&body)
                .map_err(|_| CallProbeError::UnsupportedSdp)?
                .contains("o=rust-microsip 1 3 IN IP4 192.0.2.2\r\n")
        );
        assert!(update_answer(&offer(remote, Codec::Pcmu), Codec::Pcma, pbx, local, 4).is_err());
        Ok(())
    }
}
