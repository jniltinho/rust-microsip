//! Secure call controls over the isolated TLS/SDES experiment: RFC 4733 DTMF,
//! hold/resume and blind REFER/NOTIFY, all with SRTP media. Not a production
//! call actor; only fixed fictional lab routes are reachable.

use super::super::{CallProbeError, ToneMeter};
use super::{EndpointOwner, description, media, sdes, signaling};
use crate::RegistrationConfig;
use rsipstack::{
    dialog::{DialogId, invitation::InviteOption, invite_dialog::InviteDialog},
    sip::{Header, HeadersExt, Method, Request, Response, SipMessage, StatusCode},
    transport::SipAddr,
};
use rust_microsip_media::{
    Codec, RtpHeader, RtpPacket,
    dtmf::{Digit, Transmission},
    srtp_adapter::{ReceiverBank, SecureError, SecureSender},
};
use std::{
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    net::UdpSocket,
    sync::Notify,
    time::{Instant, Interval, MissedTickBehavior},
};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

const EVENT_PT: u8 = 101;

fn cseq(value: &str) -> Option<(u32, Method)> {
    rsipstack::sip::typed::CSeq::parse(value)
        .ok()
        .map(|h| (h.seq, h.method))
}

/// INVITE CSeq numbers for which this endpoint emitted an ACK (bounded).
#[derive(Default)]
pub(super) struct AckLog(Mutex<Vec<u32>>);
impl AckLog {
    pub(super) fn sent(&self, message: &SipMessage) {
        let SipMessage::Request(request) = message else {
            return;
        };
        if request.method != Method::Ack {
            return;
        }
        let Some((seq, _)) = request.cseq_header().ok().and_then(|h| cseq(h.value())) else {
            return;
        };
        if let Ok(mut acks) = self.0.lock()
            && acks.len() < 16
        {
            acks.push(seq);
        }
    }
    fn contains(&self, seq: u32) -> bool {
        self.0.lock().is_ok_and(|acks| acks.contains(&seq))
    }
}

#[derive(Default)]
struct NotifyState {
    dialog: Option<DialogId>,
    count: u8,
    provisional: u8,
    event_ids: Vec<u32>,
    final_status: Option<u16>,
    failed: bool,
}
/// Correlated `refer` NOTIFY progress for one armed dialog (RFC 3515).
#[derive(Default)]
pub(super) struct NotifyLog(Mutex<NotifyState>, Notify);
fn single<'a>(request: &'a Request, pick: fn(&'a Header) -> Option<&'a str>) -> Option<&'a str> {
    let mut values = request.headers.iter().filter_map(pick);
    let value = values.next()?;
    values.next().is_none().then_some(value)
}
fn sipfrag(body: &[u8]) -> Option<u16> {
    if body.len() > 1024 {
        return None;
    }
    let first = std::str::from_utf8(body).ok()?.lines().next()?;
    let mut parts = first.splitn(3, ' ');
    if parts.next()? != "SIP/2.0" {
        return None;
    }
    let status = parts.next()?;
    (status.len() == 3)
        .then(|| status.parse().ok())
        .flatten()
        .filter(|s| (100..700).contains(s))
}
impl NotifyLog {
    fn arm(&self, dialog: DialogId) -> Result<(), CallProbeError> {
        let mut state = self.0.lock().map_err(signaling)?;
        if state.dialog.replace(dialog).is_some() {
            return Err(CallProbeError::Signaling);
        }
        Ok(())
    }
    /// Validate one NOTIFY; `false` yields 481 and fails the armed transfer.
    pub(super) fn consume(&self, dialog: &DialogId, request: &Request) -> bool {
        let Ok(mut state) = self.0.lock() else {
            return false;
        };
        let parsed = (|| {
            if state.dialog.as_ref() != Some(dialog) || state.final_status.is_some() {
                return None;
            }
            let event = single(request, |h| match h {
                Header::Event(v) => Some(v.value()),
                _ => None,
            })?;
            let (name, params) = event.split_once(';').unwrap_or((event, ""));
            if !name.trim().eq_ignore_ascii_case("refer") {
                return None;
            }
            let id = params
                .split(';')
                .find_map(|p| p.trim().strip_prefix("id="))
                .and_then(|v| v.parse::<u32>().ok());
            let content = single(request, |h| match h {
                Header::ContentType(v) => Some(v.value()),
                _ => None,
            })?;
            let kind = content.split(';').next()?.trim();
            if !kind.eq_ignore_ascii_case("message/sipfrag") {
                return None;
            }
            let subscription = single(request, |h| match h {
                Header::SubscriptionState(v) => Some(v.value()),
                _ => None,
            })?;
            let terminal = subscription
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("terminated");
            let status = sipfrag(&request.body)?;
            (!terminal || status >= 200).then_some((id, terminal, status))
        })();
        state.count = state.count.saturating_add(1);
        match parsed {
            Some((id, terminal, status)) if state.count <= 16 => {
                if let Some(id) = id
                    && !state.event_ids.contains(&id)
                {
                    state.event_ids.push(id);
                }
                if status < 200 {
                    state.provisional += 1;
                }
                if terminal {
                    state.final_status = Some(status);
                }
            }
            _ => state.failed = true,
        }
        let ok = !state.failed;
        drop(state);
        self.1.notify_waiters();
        ok
    }
    /// Wait for the terminal sipfrag status of the REFER with this CSeq.
    async fn wait(&self, refer_seq: u32) -> Result<(u16, u8, u8), CallProbeError> {
        loop {
            let notified = self.1.notified();
            {
                let state = self.0.lock().map_err(signaling)?;
                if state.failed || state.event_ids.iter().any(|id| *id != refer_seq) {
                    return Err(CallProbeError::Signaling);
                }
                if let Some(status) = state.final_status {
                    return Ok((status, state.count, state.provisional));
                }
            }
            notified.await;
        }
    }
}

/// One measured interval of a secure stream.
#[derive(Clone, Copy, Debug, Default)]
pub struct Window {
    /// Encrypted audio frames sent.
    pub sent: u32,
    /// Authenticated audio frames accepted after the settling time.
    pub received: u32,
    /// Decoded RMS, or zero without received audio.
    pub rms: f64,
    /// Expected-to-comparison tone power ratio, or zero without received audio.
    pub ratio: f64,
}
impl Window {
    fn voice(self) -> Result<Self, CallProbeError> {
        if self.sent < 80 || self.received < 80 || self.ratio < 20.0 {
            return Err(CallProbeError::Media);
        }
        Ok(self)
    }
}

struct SecureStream {
    rtp: Arc<UdpSocket>,
    target: SocketAddr,
    codec: Codec,
    sender: SecureSender,
    receiver: ReceiverBank,
    ssrc: u32,
    sequence: u16,
    clock: u32,
    sample: u32,
    tick: Interval,
}
impl SecureStream {
    fn new(
        rtp: Arc<UdpSocket>,
        target: sdes::Target,
        codec: Codec,
        key: sdes::LocalKey,
        ssrc: u32,
    ) -> Result<Self, CallProbeError> {
        let mut tick = tokio::time::interval(Duration::from_millis(20));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        Ok(Self {
            rtp,
            target: target.address,
            codec,
            sender: SecureSender::new(ssrc, key.into_master().map_err(media)?).map_err(media)?,
            receiver: ReceiverBank::new(target.key, std::time::Instant::now()),
            ssrc,
            sequence: 65500,
            clock: u32::MAX - 3200,
            sample: 0,
            tick,
        })
    }
    async fn send(
        &mut self,
        payload_type: u8,
        marker: bool,
        timestamp: u32,
        payload: &[u8],
    ) -> Result<(), CallProbeError> {
        let mut clear = [0; 172];
        let length = RtpHeader {
            marker,
            payload_type,
            sequence: self.sequence,
            timestamp,
            ssrc: self.ssrc,
        }
        .write(payload, &mut clear)
        .map_err(media)?;
        let wire = self.sender.protect_rtp(&clear[..length]).map_err(media)?;
        if wire.len() != length + 10
            || self.rtp.send_to(&wire, self.target).await.map_err(media)? != wire.len()
        {
            return Err(CallProbeError::Media);
        }
        self.sequence = self.sequence.wrapping_add(1);
        Ok(())
    }
    fn frame(&mut self, hz: Option<f64>) -> [u8; 160] {
        let mut payload = [0; 160];
        for octet in &mut payload {
            let value = hz.map_or(0.0, |hz| {
                8000.0 * (std::f64::consts::TAU * hz * f64::from(self.sample % 8000) / 8000.0).sin()
            });
            *octet = self.codec.encode_sample(value as i16);
            self.sample = self.sample.wrapping_add(1);
        }
        payload
    }
    /// Run for `duration`: send `tone` (None = silence) when `send`, accept
    /// authenticated G.711 after `settle`, metering `expected` against `other`.
    async fn window(
        &mut self,
        duration: Duration,
        send: bool,
        tone: Option<f64>,
        settle: Duration,
        (expected, other): (f64, f64),
    ) -> Result<Window, CallProbeError> {
        let start = Instant::now();
        let end = start + duration;
        let mut meter = ToneMeter::new(other, expected);
        let mut window = Window::default();
        let mut bytes = [0; 1515];
        loop {
            tokio::select! {biased;
                _=tokio::time::sleep_until(end)=>break,
                _=self.tick.tick()=>{
                    if send {
                        let payload=self.frame(tone);
                        self.send(self.codec.payload_type(),false,self.clock,&payload).await?;
                        window.sent+=1;
                    }
                    self.clock=self.clock.wrapping_add(160);
                },
                result=self.rtp.recv_from(&mut bytes)=>{
                    let (length,source)=result.map_err(media)?;
                    if source!=self.target || length>1514 {continue;}
                    let clear=match self.receiver.unprotect_rtp(&bytes[..length],std::time::Instant::now()) {
                        Ok(clear)=>clear,
                        Err(SecureError::RateLimited|SecureError::Authentication|SecureError::Replay|SecureError::TooOld|SecureError::InvalidPacket)=>continue,
                        Err(_)=>return Err(CallProbeError::Media),
                    };
                    let packet=RtpPacket::parse(&clear).map_err(media)?;
                    // Authentic non-audio payloads (for example PBX events) are not metered.
                    if packet.header.payload_type!=self.codec.payload_type() || packet.payload.len()!=160 || start.elapsed()<settle {continue;}
                    window.received+=1;meter.observe(self.codec,packet.payload);
                }
            }
        }
        if window.received > 0 {
            (window.rms, window.ratio) = meter.finish()?;
        }
        Ok(window)
    }
    async fn digit(&mut self, digit: Digit) -> Result<u32, CallProbeError> {
        let timestamp = self.clock;
        let mut count = 0;
        for packet in Transmission::new(digit, 10, 960, 160).map_err(media)? {
            self.tick.tick().await;
            self.send(EVENT_PT, packet.marker, timestamp, &packet.event.to_bytes())
                .await?;
            self.clock = self.clock.wrapping_add(160);
            count += 1;
        }
        for _ in 0..3 {
            self.tick.tick().await;
            let payload = self.frame(None);
            self.send(self.codec.payload_type(), false, self.clock, &payload)
                .await?;
            self.clock = self.clock.wrapping_add(160);
        }
        Ok(count)
    }
}

fn key_line(body: &[u8]) -> Zeroizing<Option<Vec<u8>>> {
    Zeroizing::new(
        body.split(|b| *b == b'\n')
            .find(|l| l.starts_with(b"a=crypto:"))
            .map(<[u8]>::to_vec),
    )
}
fn invite_to(
    caller: &EndpointOwner,
    callee: &str,
    body: Vec<u8>,
) -> Result<InviteOption, CallProbeError> {
    if !caller.admission.claim() {
        return Err(CallProbeError::Signaling);
    }
    Ok(InviteOption {
        caller: format!("sip:{}@lab.invalid", caller.username)
            .parse()
            .map_err(signaling)?,
        callee: format!("sip:{callee}@lab.invalid")
            .parse()
            .map_err(signaling)?,
        contact: caller.contact.clone(),
        destination: Some(SipAddr::try_from(&caller.server).map_err(signaling)?),
        credential: caller.registration.0.credential.clone(),
        content_type: Some("application/sdp".into()),
        offer: Some(body),
        ..Default::default()
    })
}
fn pbx_ip(owner: &EndpointOwner) -> Result<IpAddr, CallProbeError> {
    owner
        .server
        .host_with_port
        .host
        .to_string()
        .parse()
        .map_err(signaling)
}
fn ok(response: Option<Response>) -> Result<Response, CallProbeError> {
    let response = response.ok_or(CallProbeError::Signaling)?;
    if response.status_code != StatusCode::OK {
        return Err(CallProbeError::Signaling);
    }
    Ok(response)
}
async fn guarded<T>(
    cancel: CancellationToken,
    work: impl Future<Output = Result<T, CallProbeError>>,
) -> Result<T, CallProbeError> {
    if cancel.is_cancelled() {
        return Err(CallProbeError::Cancelled);
    }
    tokio::select! {biased;
        _=cancel.cancelled()=>Err(CallProbeError::Cancelled),
        result=tokio::time::timeout(Duration::from_secs(30),work)=>result.map_err(|_|CallProbeError::Timeout)?,
    }
}
async fn finish(dialog: &InviteDialog, owner: &mut EndpointOwner) -> Result<(), CallProbeError> {
    if !dialog.state().is_terminated() {
        match dialog.bye().await {
            Ok(()) => owner.wire.wait(4).await,
            Err(_) if dialog.state().is_terminated() => (),
            Err(_) => return Err(CallProbeError::Signaling),
        }
    }
    owner.wait_phase(true).await
}

/// Fixed IVR inputs: `123` selects the 1 kHz branch, `124` the 400 Hz branch.
#[derive(Clone, Copy, Debug)]
pub enum IvrCase {
    /// Three digits 123, expecting the 1 kHz success tone.
    Accepted,
    /// Three digits 124, expecting the 400 Hz rejection tone.
    Rejected,
}
/// Sanitized result of one encrypted DTMF call to IVR route 601.
#[derive(Debug)]
pub struct SecureIvrReport {
    /// Static codec label.
    pub codec: &'static str,
    /// Fixed input case.
    pub case: IvrCase,
    /// Encrypted RFC 4733 packets sent, including repeated end reports.
    pub event_packets_sent: u32,
    /// Measured branch tone after the digits.
    pub tone: Window,
}

/// Send `123` or `124` as SRTP-protected RFC 4733 events to IVR route 601 from
/// fictional 1201 and require the matching decrypted branch tone.
///
/// # Errors
/// Returns a sanitized error for negotiation, signaling, media or tone failure.
pub async fn probe_secure_ivr(
    config: RegistrationConfig,
    codec: Codec,
    case: IvrCase,
    cancel: CancellationToken,
) -> Result<SecureIvrReport, CallProbeError> {
    if config.username != "1201" || config.address.port() != 5061 {
        return Err(CallProbeError::InvalidConfig);
    }
    let stop = cancel.child_token();
    guarded(cancel, async {
        let mut caller = EndpointOwner::connect(config, codec, stop).await?;
        let local = caller.rtp.local_addr().map_err(media)?;
        let key = caller.local_key.take().ok_or(CallProbeError::Media)?;
        let offer = key.described(local, codec, 1, 1, sdes::Flow::SendRecv, true);
        let (dialog, response) = caller
            .layer
            .do_invite(
                invite_to(&caller, "601", offer.to_vec())?,
                caller.states.clone(),
            )
            .await
            .map_err(signaling)?;
        let response = ok(response)?;
        let target =
            sdes::parse(&response.body, codec, pbx_ip(&caller)?, Some(1)).map_err(description)?;
        sdes::telephone_events(&response.body).map_err(|_| CallProbeError::UnsupportedDtmf)?;
        caller.wait_phase(false).await?;
        let mut stream = SecureStream::new(caller.rtp.clone(), target, codec, key, 0x4956_5231)?;
        stream
            .window(
                Duration::from_millis(600),
                true,
                None,
                Duration::ZERO,
                (1000.0, 400.0),
            )
            .await?;
        let mut sent = 0;
        for digit in match case {
            IvrCase::Accepted => [1, 2, 3],
            IvrCase::Rejected => [1, 2, 4],
        } {
            sent += stream
                .digit(Digit::from_event(digit).map_err(media)?)
                .await?;
        }
        let tones = match case {
            IvrCase::Accepted => (1000.0, 400.0),
            IvrCase::Rejected => (400.0, 1000.0),
        };
        let tone = stream
            .window(
                Duration::from_millis(2250),
                true,
                None,
                Duration::from_millis(250),
                tones,
            )
            .await?;
        if tone.received < 80 || !(1000.0..10000.0).contains(&tone.rms) || tone.ratio < 20.0 {
            return Err(CallProbeError::Media);
        }
        finish(&dialog, &mut caller).await?;
        caller.unregister().await?;
        Ok(SecureIvrReport {
            codec: codec.name(),
            case,
            event_packets_sent: sent,
            tone,
        })
    })
    .await
}

struct Pair {
    a: EndpointOwner,
    b: EndpointOwner,
    dialog: InviteDialog,
    callee_dialog: InviteDialog,
    caller_stream: SecureStream,
    callee_stream: SecureStream,
    /// Keyed re-offers prepared before the local key moved into the sender.
    reoffers: [Zeroizing<Vec<u8>>; 2],
    initial_key_line: Zeroizing<Option<Vec<u8>>>,
    remote: (SocketAddr, SocketAddr),
}
async fn establish(
    first: RegistrationConfig,
    second: RegistrationConfig,
    codec: Codec,
    stop: CancellationToken,
) -> Result<Pair, CallProbeError> {
    super::fixtures(&first, &second)?;
    let mut a = EndpointOwner::connect(first, codec, stop.clone()).await?;
    let mut b = EndpointOwner::connect(second, codec, stop).await?;
    let local = a.rtp.local_addr().map_err(media)?;
    let key = a.local_key.take().ok_or(CallProbeError::Media)?;
    let offer = key.description(local, codec, 1, 1);
    let reoffers = [
        key.described(local, codec, 1, 2, sdes::Flow::SendOnly, false),
        key.described(local, codec, 1, 3, sdes::Flow::SendRecv, false),
    ];
    let callee = b.username.clone();
    let (dialog, response) = a
        .layer
        .do_invite(invite_to(&a, &callee, offer.to_vec())?, a.states.clone())
        .await
        .map_err(signaling)?;
    let mut response = ok(response)?;
    let target = sdes::parse(&response.body, codec, pbx_ip(&a)?, Some(1)).map_err(description)?;
    let initial_key_line = key_line(&response.body);
    response.body.clear();
    let inbound = b.incoming.recv().await.ok_or(CallProbeError::Signaling)?;
    a.wait_phase(false).await?;
    b.wait_phase(false).await?;
    b.wire.wait(1).await;
    if !inbound.dialog.state().is_confirmed() || target.address == inbound.target.address {
        return Err(CallProbeError::Signaling);
    }
    let remote = (target.address, target.control);
    let caller_stream = SecureStream::new(a.rtp.clone(), target, codec, key, 0x5345_4331)?;
    let callee_key = b.local_key.take().ok_or(CallProbeError::Media)?;
    let callee_stream = SecureStream::new(
        b.rtp.clone(),
        inbound.target,
        codec,
        callee_key,
        0x5345_4332,
    )?;
    Ok(Pair {
        a,
        b,
        dialog,
        callee_dialog: inbound.dialog,
        caller_stream,
        callee_stream,
        reoffers,
        initial_key_line,
        remote,
    })
}
const CALLER_HZ: f64 = 440.0;
const CALLEE_HZ: f64 = 880.0;
async fn voice(pair: &mut Pair) -> Result<[Window; 2], CallProbeError> {
    let two = Duration::from_secs(2);
    let (a, b) = tokio::join!(
        pair.caller_stream.window(
            two,
            true,
            Some(CALLER_HZ),
            Duration::ZERO,
            (CALLEE_HZ, CALLER_HZ)
        ),
        pair.callee_stream.window(
            two,
            true,
            Some(CALLEE_HZ),
            Duration::ZERO,
            (CALLER_HZ, CALLEE_HZ)
        )
    );
    Ok([a?.voice()?, b?.voice()?])
}
async fn teardown(mut pair: Pair) -> Result<(), CallProbeError> {
    finish(&pair.dialog, &mut pair.a).await?;
    if !pair.callee_dialog.state().is_terminated() {
        pair.b.wire.wait(2).await;
    }
    pair.b.wait_phase(true).await?;
    pair.a.unregister().await?;
    pair.b.unregister().await
}
/// Re-offer with the unchanged local key; require the expected answer
/// direction, the same PBX key line and addresses, and an emitted ACK.
async fn reoffer(
    pair: &mut Pair,
    index: usize,
    expected: sdes::Flow,
    codec: Codec,
) -> Result<u32, CallProbeError> {
    let body = pair.reoffers[index].to_vec();
    let mut answer = ok(pair
        .dialog
        .reinvite(
            Some(vec![Header::ContentType("application/sdp".into())]),
            Some(body),
        )
        .await
        .map_err(signaling)?)?;
    let (target, flow) = sdes::parse_directed(&answer.body, codec, pbx_ip(&pair.a)?, Some(1))
        .map_err(description)?;
    if flow != expected
        || (target.address, target.control) != pair.remote
        || key_line(&answer.body) != pair.initial_key_line
    {
        return Err(CallProbeError::UnsupportedSdp);
    }
    answer.body.clear();
    let seq = answer
        .cseq_header()
        .ok()
        .and_then(|h| cseq(h.value()))
        .filter(|(_, method)| *method == Method::Invite)
        .ok_or(CallProbeError::Signaling)?
        .0;
    // ACK emission is synchronous with the 2xx handling inside the SDK call.
    for _ in 0..50 {
        if pair.a.acks.contains(seq) {
            return Ok(seq);
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Err(CallProbeError::Signaling)
}

/// Sanitized hold/resume result over SRTP with an unchanged key epoch.
#[derive(Debug)]
pub struct SecureHoldReport {
    /// Static codec label.
    pub codec: &'static str,
    /// Caller then callee, before hold.
    pub before: [Window; 2],
    /// Caller (holder, silent) then callee (still sending) while held.
    pub held: [Window; 2],
    /// Caller then callee after resume.
    pub resumed: [Window; 2],
    /// CSeq numbers of the acknowledged hold and resume re-INVITEs.
    pub acked_reinvites: [u32; 2],
}

/// Hold (sendonly → recvonly) and resume (sendrecv) a fictional 1201→1202
/// SRTP call with the same keys and continuing sender/receiver state.
///
/// # Errors
/// Returns a sanitized error unless both re-offers, suppression and restored
/// two-way encrypted audio pass.
pub async fn probe_secure_hold(
    first: RegistrationConfig,
    second: RegistrationConfig,
    codec: Codec,
    cancel: CancellationToken,
) -> Result<SecureHoldReport, CallProbeError> {
    let stop = cancel.child_token();
    guarded(cancel, async {
        let mut pair = establish(first, second, codec, stop).await?;
        let before = voice(&mut pair).await?;
        let hold = reoffer(&mut pair, 0, sdes::Flow::RecvOnly, codec).await?;
        let held_for = Duration::from_secs(1);
        let settle = Duration::from_millis(250);
        let (holder, peer) = tokio::join!(
            pair.caller_stream
                .window(held_for, false, None, settle, (CALLEE_HZ, CALLER_HZ)),
            pair.callee_stream.window(
                held_for,
                true,
                Some(CALLEE_HZ),
                Duration::ZERO,
                (CALLER_HZ, CALLEE_HZ)
            )
        );
        let held = [holder?, peer?];
        // The holder hears nothing after settling; the peer kept transmitting.
        if held[0].sent != 0 || held[0].received != 0 || held[1].sent < 40 {
            return Err(CallProbeError::Media);
        }
        let resume = reoffer(&mut pair, 1, sdes::Flow::SendRecv, codec).await?;
        let resumed = voice(&mut pair).await?;
        teardown(pair).await?;
        Ok(SecureHoldReport {
            codec: codec.name(),
            before,
            held,
            resumed,
            acked_reinvites: [hold, resume],
        })
    })
    .await
}

/// Fixed transfer destinations only, never an arbitrary Refer-To URI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Destination {
    /// Route 602 plays a 1 kHz tone.
    ReferenceTone,
    /// Route 699 does not exist and must fail without losing the call.
    Missing,
}
/// Sanitized blind-transfer result with SRTP media on both legs.
#[derive(Debug)]
pub struct SecureTransferReport {
    /// Static codec label.
    pub codec: &'static str,
    /// Requested fixed destination.
    pub destination: Destination,
    /// REFER response status (2xx required).
    pub refer_status: u16,
    /// Terminal sipfrag status from the correlated NOTIFY.
    pub final_status: u16,
    /// Accepted NOTIFY count and provisional subset.
    pub notifies: (u8, u8),
    /// Two-way audio before the REFER.
    pub before: [Window; 2],
    /// Two-way audio after a failed transfer, on the preserved call.
    pub restored: Option<[Window; 2]>,
    /// Transferee's 1 kHz destination tone versus the former partner's tone.
    pub destination_tone: Option<Window>,
}

/// Blind-transfer the fictional 1202 peer from 1201 to a fixed route over
/// SRTP and verify the correlated NOTIFY outcome and resulting media.
///
/// # Errors
/// Returns a sanitized error unless the outcome and its media match.
pub async fn probe_secure_transfer(
    first: RegistrationConfig,
    second: RegistrationConfig,
    codec: Codec,
    destination: Destination,
    cancel: CancellationToken,
) -> Result<SecureTransferReport, CallProbeError> {
    let stop = cancel.child_token();
    guarded(cancel, async {
        let mut pair = establish(first, second, codec, stop).await?;
        let before = voice(&mut pair).await?;
        pair.a.notifies.arm(pair.dialog.id())?;
        let route = match destination {
            Destination::ReferenceTone => "602",
            Destination::Missing => "699",
        };
        let response = pair
            .dialog
            .refer(format!("<sip:{route}@lab.invalid>"), None, None)
            .await
            .map_err(signaling)?
            .ok_or(CallProbeError::Signaling)?;
        let refer_status = u16::from(response.status_code.clone());
        let refer_seq = response
            .cseq_header()
            .ok()
            .and_then(|h| cseq(h.value()))
            .filter(|(_, method)| *method == Method::Refer)
            .ok_or(CallProbeError::Signaling)?
            .0;
        if !(200..300).contains(&refer_status) {
            return Err(CallProbeError::Signaling);
        }
        let (final_status, count, provisional) =
            tokio::time::timeout(Duration::from_secs(10), pair.a.notifies.wait(refer_seq))
                .await
                .map_err(|_| CallProbeError::Timeout)??;
        let (restored, destination_tone) = match destination {
            Destination::Missing => {
                if final_status < 400
                    || !pair.dialog.state().is_confirmed()
                    || !pair.callee_dialog.state().is_confirmed()
                {
                    return Err(CallProbeError::Signaling);
                }
                (Some(voice(&mut pair).await?), None)
            }
            Destination::ReferenceTone => {
                if !(200..300).contains(&final_status) {
                    return Err(CallProbeError::Signaling);
                }
                finish(&pair.dialog, &mut pair.a).await?;
                if pair.callee_dialog.state().is_terminated() {
                    return Err(CallProbeError::Signaling);
                }
                // Compare against the former partner's tone to reject leakage.
                let tone = pair
                    .callee_stream
                    .window(
                        Duration::from_millis(2250),
                        true,
                        Some(CALLEE_HZ),
                        Duration::from_millis(250),
                        (1000.0, CALLER_HZ),
                    )
                    .await?;
                if tone.received < 80 || !(1000.0..10000.0).contains(&tone.rms) || tone.ratio < 20.0
                {
                    return Err(CallProbeError::Media);
                }
                pair.callee_dialog.bye().await.map_err(signaling)?;
                pair.b.wire.wait(4).await;
                (None, Some(tone))
            }
        };
        teardown(pair).await?;
        Ok(SecureTransferReport {
            codec: codec.name(),
            destination,
            refer_status,
            final_status,
            notifies: (count, provisional),
            before,
            restored,
            destination_tone,
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notify(event: &str, state: &str, body: &str) -> Request {
        let text = format!(
            "NOTIFY sip:1201@192.0.2.1 SIP/2.0\r\nVia: SIP/2.0/TLS 192.0.2.1;branch=z9hG4bK1\r\nFrom: <sip:1202@lab.invalid>;tag=b\r\nTo: <sip:1201@lab.invalid>;tag=a\r\nCall-ID: c\r\nCSeq: 2 NOTIFY\r\nEvent: {event}\r\nSubscription-State: {state}\r\nContent-Type: message/sipfrag;version=2.0\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        match SipMessage::try_from(text.as_str()) {
            Ok(SipMessage::Request(request)) => request,
            _ => unreachable!("fixture parses"),
        }
    }
    fn armed() -> (NotifyLog, DialogId) {
        let id = DialogId {
            call_id: "c".into(),
            local_tag: "a".into(),
            remote_tag: "b".into(),
        };
        let log = NotifyLog::default();
        assert!(log.arm(id.clone()).is_ok());
        (log, id)
    }

    #[tokio::test]
    async fn notify_requires_terminal_final_status_and_matching_refer() -> Result<(), CallProbeError>
    {
        let (log, id) = armed();
        assert!(log.consume(
            &id,
            &notify("refer;id=7", "active", "SIP/2.0 100 Trying\r\n")
        ));
        assert!(log.consume(
            &id,
            &notify(
                "refer;id=7",
                "terminated;reason=noresource",
                "SIP/2.0 200 OK\r\n"
            )
        ));
        assert_eq!(log.wait(7).await?, (200, 2, 1));
        let (log, id) = armed();
        assert!(log.consume(
            &id,
            &notify("refer;id=8", "terminated", "SIP/2.0 404 Not Found\r\n")
        ));
        assert!(log.wait(7).await.is_err());
        Ok(())
    }
    #[test]
    fn notify_rejects_foreign_malformed_and_post_final_messages() {
        let (log, id) = armed();
        let foreign = DialogId {
            call_id: "x".into(),
            ..id.clone()
        };
        assert!(!log.consume(
            &foreign,
            &notify("refer", "active", "SIP/2.0 100 Trying\r\n")
        ));
        for (event, state, body) in [
            ("presence", "active", "SIP/2.0 100 Trying\r\n"),
            ("refer", "terminated", "SIP/2.0 180 Ringing\r\n"),
            ("refer", "active", "HTTP/1.1 200 OK\r\n"),
            ("refer", "active", "SIP/2.0 2000 OK\r\n"),
        ] {
            let (log, id) = armed();
            assert!(
                !log.consume(&id, &notify(event, state, body)),
                "{event} {state}"
            );
        }
        let (log, id) = armed();
        assert!(log.consume(&id, &notify("refer", "terminated", "SIP/2.0 200 OK\r\n")));
        assert!(!log.consume(&id, &notify("refer", "terminated", "SIP/2.0 200 OK\r\n")));
    }
    #[test]
    fn ack_log_records_only_ack_cseq_with_a_bound() {
        let log = AckLog::default();
        for seq in 1..40 {
            let text = format!(
                "ACK sip:1202@192.0.2.1 SIP/2.0\r\nVia: SIP/2.0/TLS 192.0.2.1;branch=z9hG4bK{seq}\r\nFrom: <sip:1201@lab.invalid>;tag=a\r\nTo: <sip:1202@lab.invalid>;tag=b\r\nCall-ID: c\r\nCSeq: {seq} ACK\r\nContent-Length: 0\r\n\r\n"
            );
            if let Ok(message) = SipMessage::try_from(text.as_str()) {
                log.sent(&message);
            }
        }
        assert!(log.contains(3) && log.contains(16) && !log.contains(17));
    }
}
