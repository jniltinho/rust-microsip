//! Isolated synthetic plain-RTP call proof, never the production call actor.
//!
//! Two independently bound Rust endpoints exchange different tones through an
//! explicitly selected disposable PBX. TLS uses only the separate RTP-only lab
//! accounts; mandatory-SRTP accounts are refused rather than downgraded.
#[path = "g0_controls.rs"]
pub mod controls;

use super::{OwnedRegistration, ProbeTasks, RegistrationConfig, Transport};
use rsipstack::{
    EndpointBuilder,
    dialog::{
        authenticate::Credential,
        dialog::{Dialog, DialogState},
        dialog_layer::DialogLayer,
        invitation::InviteOption,
        invite_dialog::InviteDialog,
        registration::Registration,
    },
    sip::{Header, HeadersExt, Method, SipMessage, StatusCode, Uri},
    transaction::endpoint::{Endpoint, MessageInspector},
    transport::{SipAddr, TransportLayer, tls::TlsConfig, udp::UdpConnection},
};
use rust_microsip_media::{Codec, RtpHeader, RtpPacket};
use secrecy::ExposeSecret;
use std::{
    net::{IpAddr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};
use thiserror::Error;
use tokio::{
    net::UdpSocket,
    sync::{Notify, mpsc, watch},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;

/// Fixed, sanitized G0 failure categories. No raw SDP/auth/error is exposed.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CallProbeError {
    /// Inputs do not select a compatible pair of fictional plain-RTP fixtures.
    #[error("invalid isolated call fixture configuration")]
    InvalidConfig,
    /// Caller cancelled the finite operation.
    #[error("synthetic call cancelled")]
    Cancelled,
    /// Setup/media/teardown exceeded the total finite deadline.
    #[error("synthetic call deadline exceeded")]
    Timeout,
    /// Registration, dialog, ACK or BYE exchange failed.
    #[error("synthetic call signaling failed")]
    Signaling,
    /// The SDP answer does not negotiate the offered 8 kHz telephone-event.
    #[error("negotiated RTP telephone-event DTMF unavailable")]
    UnsupportedDtmf,
    /// Bounded SDP does not offer the required anchored plain G.711 profile.
    #[error("unsupported isolated G.711 SDP")]
    UnsupportedSdp,
    /// RTP/source/tone measurements failed; audio success is not asserted.
    #[error("synthetic bidirectional media verification failed")]
    Media,
}

/// Aggregate synthetic measurements only; never sampled audio or identifiers.
#[derive(Clone, Copy, Debug)]
pub struct MediaMeasurement {
    /// Successfully sent 20 ms G.711 RTP packets.
    pub sent_packets: u32,
    /// Valid, new RTP packets from the negotiated PBX media socket.
    pub received_packets: u32,
    /// Decoded RMS amplitude (signed 16-bit PCM scale).
    pub rms: f64,
    /// Expected remote-tone power divided by this endpoint's own-tone power.
    pub remote_to_own_power_ratio: f64,
    /// Missing sequence positions observed after the first accepted packet.
    pub lost_packets: u32,
    /// Duplicate/late packets excluded from tone measurements.
    pub late_packets: u32,
}

/// Completed lab-only signaling and two independent endpoint measurements.
#[derive(Debug)]
pub struct CallProbeReport {
    /// Static transport label.
    pub transport: &'static str,
    /// Static codec label.
    pub codec: &'static str,
    /// Whether the second fictional endpoint initiated this session.
    pub reverse: bool,
    /// Measurements at the first endpoint (440 Hz source; expects 880 Hz).
    pub first: MediaMeasurement,
    /// Measurements at the second endpoint (880 Hz source; expects 440 Hz).
    pub second: MediaMeasurement,
    /// The receiving endpoint observed ACK and both dialogs terminated.
    pub ack_and_termination_verified: bool,
    /// Sender observed a 200 response to BYE, not merely a local hangup state.
    pub bye_200_verified: bool,
    /// Both authenticated unregistrations returned 200 after the call.
    pub unregistered: bool,
}

#[derive(Clone, Copy)]
struct AudioTarget {
    address: SocketAddr,
    codec: Codec,
}

fn offer(address: SocketAddr, codec: Codec) -> Vec<u8> {
    let family = if address.is_ipv4() { "IP4" } else { "IP6" };
    format!("v=0\r\no=rust-microsip 1 1 IN {family} {}\r\ns=G0 synthetic plain RTP\r\nc=IN {family} {}\r\nt=0 0\r\nm=audio {} RTP/AVP {}\r\na=rtpmap:{} {}/8000\r\na=ptime:20\r\na=sendrecv\r\n",
        address.ip(), address.ip(), address.port(), codec.payload_type(), codec.payload_type(), codec.name()).into_bytes()
}

fn parse_offer(bytes: &[u8], codec: Codec, pbx_ip: IpAddr) -> Result<AudioTarget, CallProbeError> {
    let bad = CallProbeError::UnsupportedSdp;
    if bytes.is_empty() || bytes.len() > 8192 {
        return Err(bad);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| CallProbeError::UnsupportedSdp)?;
    let mut ip = None;
    let mut port = None;
    let mut media = 0;
    let mut lines = 0;
    for line in text.lines() {
        lines += 1;
        if lines > 128 || line.len() > 512 || line.contains('\0') {
            return Err(bad);
        }
        if line.starts_with("a=crypto:")
            || line.starts_with("a=fingerprint:")
            || line.starts_with("a=ice-")
            || line == "a=inactive"
            || line == "a=sendonly"
            || line == "a=recvonly"
        {
            return Err(bad);
        }
        if let Some(value) = line.strip_prefix("c=IN ") {
            let fields: Vec<_> = value.split_ascii_whitespace().collect();
            if fields.len() != 2 || !matches!(fields[0], "IP4" | "IP6") {
                return Err(bad);
            }
            let address: IpAddr = fields[1]
                .parse()
                .map_err(|_| CallProbeError::UnsupportedSdp)?;
            if address != pbx_ip || (fields[0] == "IP4") != address.is_ipv4() {
                return Err(bad);
            }
            ip = Some(address);
        }
        if let Some(value) = line.strip_prefix("m=") {
            media += 1;
            let fields: Vec<_> = value.split_ascii_whitespace().collect();
            if media != 1
                || fields.len() < 4
                || fields[0] != "audio"
                || fields[2] != "RTP/AVP"
                || !fields[3..]
                    .iter()
                    .any(|f| f.parse::<u8>().ok() == Some(codec.payload_type()))
            {
                return Err(bad);
            }
            let value: u16 = fields[1]
                .parse()
                .map_err(|_| CallProbeError::UnsupportedSdp)?;
            if !(10000..=10019).contains(&value) {
                return Err(bad);
            }
            port = Some(value);
        }
        if let Some(value) = line.strip_prefix("a=rtpmap:") {
            let (pt, encoding) = value
                .split_once(' ')
                .ok_or(CallProbeError::UnsupportedSdp)?;
            if pt.parse::<u8>().ok() == Some(codec.payload_type())
                && !encoding.eq_ignore_ascii_case(&format!("{}/8000", codec.name()))
                && !encoding.eq_ignore_ascii_case(&format!("{}/8000/1", codec.name()))
            {
                return Err(bad);
            }
        }
    }
    Ok(AudioTarget {
        address: SocketAddr::new(
            ip.ok_or(CallProbeError::UnsupportedSdp)?,
            port.ok_or(CallProbeError::UnsupportedSdp)?,
        ),
        codec,
    })
}

#[derive(Clone, Copy, Default)]
struct Phase {
    confirmed: bool,
    terminated: bool,
    failed: bool,
}
struct Incoming {
    dialog: InviteDialog,
    target: AudioTarget,
}
#[derive(Default)]
struct CallWire {
    flags: AtomicU8,
    changed: Notify,
}
impl CallWire {
    fn mark(&self, flag: u8) {
        self.flags.fetch_or(flag, Ordering::Relaxed);
        self.changed.notify_one();
    }
    async fn wait(&self, flags: u8) {
        loop {
            let notified = self.changed.notified();
            if self.flags.load(Ordering::Relaxed) & flags == flags {
                return;
            }
            notified.await;
        }
    }
}
struct CallInspector(Arc<CallWire>);
impl MessageInspector for CallInspector {
    fn before_send(&self, msg: SipMessage, _: Option<&SipAddr>) -> SipMessage {
        if matches!(&msg, SipMessage::Request(r) if r.method == Method::Ack) {
            self.0.mark(8);
        }
        msg
    }
    fn after_received(&self, msg: SipMessage, _: Option<&SipAddr>) -> SipMessage {
        match &msg {
            SipMessage::Request(r) if r.method == Method::Ack => self.0.mark(1),
            SipMessage::Request(r) if r.method == Method::Bye => self.0.mark(2),
            SipMessage::Response(r)
                if r.status_code == StatusCode::OK
                    && r.cseq_header().ok().and_then(|h| h.method().ok()) == Some(Method::Bye) =>
            {
                self.0.mark(4)
            }
            _ => (),
        }
        msg
    }
}

struct LabEndpoint {
    owner: ProbeTasks,
    endpoint: Endpoint,
    layer: Arc<DialogLayer>,
    registration: OwnedRegistration,
    server: Uri,
    contact: Uri,
    username: String,
    rtp: Arc<UdpSocket>,
    incoming: mpsc::Receiver<Incoming>,
    phase: watch::Receiver<Phase>,
    states: rsipstack::dialog::dialog::DialogStateSender,
    wire: Arc<CallWire>,
}
impl LabEndpoint {
    async fn connect(
        config: RegistrationConfig,
        codec: Codec,
        stop: CancellationToken,
    ) -> Result<Self, CallProbeError> {
        let mut owner = ProbeTasks {
            stop: stop.child_token(),
            tasks: JoinSet::new(),
        };
        let wildcard = if config.address.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };
        let route = UdpSocket::bind(wildcard)
            .await
            .map_err(|_| CallProbeError::Signaling)?;
        route
            .connect(config.address)
            .await
            .map_err(|_| CallProbeError::Signaling)?;
        let local_ip = route
            .local_addr()
            .map_err(|_| CallProbeError::Signaling)?
            .ip();
        drop(route);
        let transport = TransportLayer::new(owner.stop.clone());
        let udp = UdpConnection::create_connection(
            SocketAddr::new(local_ip, 0),
            None,
            Some(owner.stop.child_token()),
        )
        .await
        .map_err(|_| CallProbeError::Signaling)?;
        transport.add_transport(udp.into());
        let label = config.transport.as_str();
        if let Transport::Tls {
            ca_pem,
            server_name,
        } = config.transport
        {
            transport.set_tls_config(TlsConfig {
                ca_certs: Some(ca_pem),
                sni_hostname: Some(server_name),
                ..Default::default()
            });
        }
        let server: Uri = format!("sip:{};transport={label}", config.address)
            .parse()
            .map_err(|_| CallProbeError::InvalidConfig)?;
        let destination = SipAddr::try_from(&server).map_err(|_| CallProbeError::Signaling)?;
        let (connection, _) = transport
            .lookup(&destination, None)
            .await
            .map_err(|_| CallProbeError::Signaling)?;
        let local = connection
            .get_addr()
            .get_socketaddr()
            .map_err(|_| CallProbeError::Signaling)?;
        if local.ip() != local_ip || local.port() == 0 {
            return Err(CallProbeError::Signaling);
        }
        let contact: Uri = format!("sip:{}@{local};transport={label}", config.username)
            .parse()
            .map_err(|_| CallProbeError::Signaling)?;
        let wire = Arc::new(CallWire::default());
        let endpoint = EndpointBuilder::new()
            .with_cancel_token(owner.stop.clone())
            .with_transport_layer(transport)
            .with_user_agent("rust-microsip G0 synthetic call")
            .with_inspector(Box::new(CallInspector(wire.clone())))
            .with_allows(vec![
                Method::Register,
                Method::Invite,
                Method::Ack,
                Method::Bye,
                Method::Cancel,
                Method::Options,
            ])
            .build();
        let layer = Arc::new(DialogLayer::new(endpoint.inner.clone()));
        let rtp = Arc::new(
            UdpSocket::bind(SocketAddr::new(local_ip, 0))
                .await
                .map_err(|_| CallProbeError::Media)?,
        );
        let answer = offer(rtp.local_addr().map_err(|_| CallProbeError::Media)?, codec);
        let inner = endpoint.inner.clone();
        let stopped = owner.stop.clone();
        owner.tasks.spawn(async move {
            if inner.serve().await.is_err() {
                stopped.cancel();
            }
        });
        let (states, mut events) = layer.new_dialog_state_channel();
        let (phase_tx, phase) = watch::channel(Phase::default());
        let failed = phase_tx.clone();
        let stopped = owner.stop.clone();
        owner.tasks.spawn(async move {
            let mut count=0u8;
            loop {tokio::select! {biased;_ = stopped.cancelled()=>break, event=events.recv()=>{
                let Some(event)=event else{break};count=count.saturating_add(1);
                phase_tx.send_modify(|p| {if count>64 {p.failed=true;}
                    match event {DialogState::Confirmed(..)=>p.confirmed=true,DialogState::Terminated(..)=>p.terminated=true,_=>()}});
                if count>64 {stopped.cancel();break;}
            }}}
        });
        let mut transactions = endpoint
            .incoming_transactions()
            .map_err(|_| CallProbeError::Signaling)?;
        let (incoming_tx, incoming) = mpsc::channel(1);
        let incoming_layer = layer.clone();
        let incoming_states = states.clone();
        let incoming_contact = contact.clone();
        let stopped = owner.stop.clone();
        let pbx_ip = config.address.ip();
        owner.tasks.spawn(async move {
            let result=async {
                let mut admitted=false;
                loop {tokio::select! {biased;_ = stopped.cancelled()=>return Ok::<(),CallProbeError>(()),item=transactions.recv()=>{
                    let Some(mut tx)=item else{return Err(CallProbeError::Signaling)};
                    if let Some(Dialog::Invite(mut dialog))=incoming_layer.match_dialog(&tx){
                        dialog.handle(&mut tx).await.map_err(|_|CallProbeError::Signaling)?;
                    }else if tx.original.method==Method::Invite && !admitted {
                        let target=match parse_offer(&tx.original.body,codec,pbx_ip){Ok(t)=>t,Err(_)=>{tx.reply(StatusCode::NotAcceptableHere).await.map_err(|_|CallProbeError::Signaling)?;continue;}};
                        let mut dialog=incoming_layer.get_or_create_server_invite(&tx,incoming_states.clone(),None,Some(incoming_contact.clone())).map_err(|_|CallProbeError::Signaling)?;
                        dialog.ringing(None,None).map_err(|_|CallProbeError::Signaling)?;
                        dialog.accept(Some(vec![Header::ContentType("application/sdp".into())]),Some(answer.clone())).map_err(|_|CallProbeError::Signaling)?;
                        incoming_tx.try_send(Incoming{dialog:dialog.clone(),target}).map_err(|_|CallProbeError::Signaling)?;
                        admitted=true;
                        dialog.handle(&mut tx).await.map_err(|_|CallProbeError::Signaling)?;
                    }else {
                        let status=if tx.original.method==Method::Options{StatusCode::OK}else if tx.original.method==Method::Invite{StatusCode::BusyHere}else{StatusCode::CallTransactionDoesNotExist};
                        tx.reply(status).await.map_err(|_|CallProbeError::Signaling)?;
                    }
                }}}
            }.await;
            if result.is_err(){failed.send_modify(|p|p.failed=true);stopped.cancel();}
        });
        let credential = Credential {
            username: config.username.clone(),
            password: config.password.expose_secret().into(),
            realm: None,
            auth_username: None,
        };
        let mut registration =
            OwnedRegistration(Registration::new(endpoint.inner.clone(), Some(credential)));
        registration.0.contact = Some(rsipstack::sip::typed::Contact {
            display_name: None,
            uri: contact.clone(),
            params: vec![],
        });
        let response = registration
            .0
            .register(server.clone(), Some(120))
            .await
            .map_err(|_| CallProbeError::Signaling)?;
        if response.status_code != StatusCode::OK {
            return Err(CallProbeError::Signaling);
        }
        Ok(Self {
            owner,
            endpoint,
            layer,
            registration,
            server,
            contact,
            username: config.username,
            rtp,
            incoming,
            phase,
            states,
            wire,
        })
    }
    async fn wait_phase(&mut self, terminated: bool) -> Result<(), CallProbeError> {
        loop {
            let p = *self.phase.borrow_and_update();
            if p.failed {
                return Err(CallProbeError::Signaling);
            }
            if (terminated && p.terminated) || (!terminated && p.confirmed) {
                return Ok(());
            }
            self.phase
                .changed()
                .await
                .map_err(|_| CallProbeError::Signaling)?;
        }
    }
    async fn unregister(&mut self) -> Result<(), CallProbeError> {
        let response = self
            .registration
            .0
            .register(self.server.clone(), Some(0))
            .await
            .map_err(|_| CallProbeError::Signaling)?;
        if response.status_code != StatusCode::OK {
            return Err(CallProbeError::Signaling);
        }
        self.endpoint.shutdown();
        self.owner.stop.cancel();
        Ok(())
    }
}

struct ToneMeter {
    own_hz: f64,
    remote_hz: f64,
    samples: u32,
    energy: f64,
    wanted: f64,
    echo: f64,
}
impl ToneMeter {
    fn new(own_hz: f64, remote_hz: f64) -> Self {
        Self {
            own_hz,
            remote_hz,
            samples: 0,
            energy: 0.0,
            wanted: 0.0,
            echo: 0.0,
        }
    }
    fn observe(&mut self, codec: Codec, payload: &[u8]) {
        let (mut wr, mut wi, mut er, mut ei) = (0.0, 0.0, 0.0, 0.0);
        for (index, &octet) in payload.iter().enumerate() {
            let value = f64::from(codec.decode_sample(octet));
            self.energy += value * value;
            self.samples += 1;
            let phase = std::f64::consts::TAU * index as f64 / 8000.0;
            wr += value * (phase * self.remote_hz).cos();
            wi += value * (phase * self.remote_hz).sin();
            er += value * (phase * self.own_hz).cos();
            ei += value * (phase * self.own_hz).sin();
        }
        self.wanted += wr * wr + wi * wi;
        self.echo += er * er + ei * ei;
    }
    fn finish(&self) -> Result<(f64, f64), CallProbeError> {
        if self.samples == 0 {
            return Err(CallProbeError::Media);
        }
        let rms = (self.energy / f64::from(self.samples)).sqrt();
        let ratio = self.wanted / self.echo.max(1.0);
        if !rms.is_finite()
            || !ratio.is_finite()
            || !(1000.0..10000.0).contains(&rms)
            || ratio < 20.0
        {
            return Err(CallProbeError::Media);
        }
        Ok((rms, ratio))
    }
}

#[derive(Default)]
struct PacketTracker {
    previous: Option<RtpHeader>,
}
impl PacketTracker {
    // A fixed 160-sample synthetic stream, not a general jitter/RTCP receiver.
    fn observe(&mut self, header: RtpHeader) -> Result<Option<u32>, CallProbeError> {
        let mut lost = 0;
        if let Some(previous) = self.previous {
            if header.ssrc != previous.ssrc {
                return Err(CallProbeError::Media);
            }
            let delta = header.sequence.wrapping_sub(previous.sequence);
            if delta == 0 || delta > 32767 {
                return Ok(None);
            }
            if header.timestamp.wrapping_sub(previous.timestamp) != u32::from(delta) * 160 {
                return Err(CallProbeError::Media);
            }
            lost = u32::from(delta - 1);
        }
        self.previous = Some(header);
        Ok(Some(lost))
    }
}

async fn measure(
    socket: Arc<UdpSocket>,
    target: AudioTarget,
    own_hz: f64,
    remote_hz: f64,
    ssrc: u32,
) -> Result<MediaMeasurement, CallProbeError> {
    let mut stats = MediaMeasurement {
        sent_packets: 0,
        received_packets: 0,
        rms: 0.0,
        remote_to_own_power_ratio: 0.0,
        lost_packets: 0,
        late_packets: 0,
    };
    let mut tick = tokio::time::interval(Duration::from_millis(20));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let end = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut send = [0; 172];
    let mut receive = [0; 1501];
    let mut sample_index = 0u32;
    // Deterministic lab values deliberately exercise both sequence/timestamp rollover.
    let mut sequence = 65500u16;
    let mut timestamp = u32::MAX - 3200;
    let mut tracker = PacketTracker::default();
    let mut meter = ToneMeter::new(own_hz, remote_hz);
    loop {
        tokio::select! {biased;
            _=tokio::time::sleep_until(end)=>break,
            _=tick.tick()=>{
                let mut payload=[0;160];
                for octet in &mut payload {let phase=std::f64::consts::TAU*own_hz*f64::from(sample_index%8000)/8000.0;*octet=target.codec.encode_sample((8000.0*phase.sin()) as i16);sample_index=sample_index.wrapping_add(1);}
                let length=RtpHeader{marker:stats.sent_packets==0,payload_type:target.codec.payload_type(),sequence,timestamp,ssrc}.write(&payload,&mut send).map_err(|_|CallProbeError::Media)?;
                let sent=socket.send_to(&send[..length],target.address).await.map_err(|_|CallProbeError::Media)?;
                if sent!=length{return Err(CallProbeError::Media);}stats.sent_packets+=1;sequence=sequence.wrapping_add(1);timestamp=timestamp.wrapping_add(160);
            }
            result=socket.recv_from(&mut receive)=>{
                let (length,source)=result.map_err(|_|CallProbeError::Media)?;
                if source!=target.address{return Err(CallProbeError::Media);}
                let packet=RtpPacket::parse(&receive[..length]).map_err(|_|CallProbeError::Media)?;
                if packet.header.payload_type!=target.codec.payload_type() || packet.payload.len()!=160 {return Err(CallProbeError::Media);}
                let Some(lost)=tracker.observe(packet.header)? else {stats.late_packets+=1;continue;};
                stats.lost_packets+=lost;stats.received_packets+=1;
                meter.observe(target.codec, packet.payload);
            }
        }
    }
    if stats.sent_packets < 240 || stats.received_packets < 200 {
        return Err(CallProbeError::Media);
    }
    (stats.rms, stats.remote_to_own_power_ratio) = meter.finish()?;
    Ok(stats)
}

/// Perform one five-second, synthetic, independent-endpoint plain-RTP call.
///
/// Only fictional 1001/1002 UDP, 1101/1102 TCP or 1301/1302 TLS RTP-only pairs
/// are admitted, with the same explicit registrar. 1201/1202 mandatory-SRTP
/// accounts are rejected: this does not implement SRTP or approve secure media.
/// No device/configuration lookup, logging or recording is installed. There is
/// a 30-second total deadline, owned-task cancellation and explicit unregister.
/// Upstream parser/channel/credential-copy production gates remain in ADR 0004.
///
/// # Errors
/// Returns [`CallProbeError`] unless registration, INVITE/200/ACK, different
/// tones in both directions, BYE/200 and unregistrations are all verified.
pub async fn probe_plain_call_pair(
    first: RegistrationConfig,
    second: RegistrationConfig,
    codec: Codec,
    reverse: bool,
    cancel: CancellationToken,
) -> Result<CallProbeReport, CallProbeError> {
    if cancel.is_cancelled() {
        return Err(CallProbeError::Cancelled);
    }
    let label = first.transport.as_str();
    let allowed = match label {
        "udp" => ("1001", "1002"),
        "tcp" => ("1101", "1102"),
        "tls" => ("1301", "1302"),
        _ => return Err(CallProbeError::InvalidConfig),
    };
    if first.address != second.address
        || label != second.transport.as_str()
        || first.username != allowed.0
        || second.username != allowed.1
    {
        return Err(CallProbeError::InvalidConfig);
    }
    let stop = cancel.child_token();
    tokio::select! {biased;_=cancel.cancelled()=>Err(CallProbeError::Cancelled),result=tokio::time::timeout(Duration::from_secs(30),async {
        let mut a=LabEndpoint::connect(first,codec,stop.clone()).await?;
        let mut b=LabEndpoint::connect(second,codec,stop.clone()).await?;
        let (caller,callee)=if reverse{(&mut b,&mut a)}else{(&mut a,&mut b)};
        let caller_uri:Uri=format!("sip:{}@lab.invalid",caller.username).parse().map_err(|_|CallProbeError::InvalidConfig)?;
        let callee_uri:Uri=format!("sip:{}@lab.invalid",callee.username).parse().map_err(|_|CallProbeError::InvalidConfig)?;
        let option=InviteOption{caller:caller_uri,callee:callee_uri,contact:caller.contact.clone(),
            destination:Some(SipAddr::try_from(&caller.server).map_err(|_|CallProbeError::Signaling)?),credential:caller.registration.0.credential.clone(),
            content_type:Some("application/sdp".into()),offer:Some(offer(caller.rtp.local_addr().map_err(|_|CallProbeError::Media)?,codec)),..Default::default()};
        let (dialog,response)=caller.layer.do_invite(option,caller.states.clone()).await.map_err(|_|CallProbeError::Signaling)?;
        let response=response.ok_or(CallProbeError::Signaling)?;
        if response.status_code!=StatusCode::OK{return Err(CallProbeError::Signaling);}
        let remote=parse_offer(&response.body,codec,caller.server.host_with_port.host.to_string().parse().map_err(|_|CallProbeError::InvalidConfig)?)?;
        let inbound=callee.incoming.recv().await.ok_or(CallProbeError::Signaling)?;
        caller.wait_phase(false).await?;callee.wait_phase(false).await?;callee.wire.wait(1).await;
        if !inbound.dialog.state().is_confirmed()||remote.address==inbound.target.address{return Err(CallProbeError::Signaling);}
        let (own_hz,other_hz)=if reverse{(880.0,440.0)}else{(440.0,880.0)};
        let (caller_media,callee_media)=tokio::join!(measure(caller.rtp.clone(),remote,own_hz,other_hz,0x12345678),measure(callee.rtp.clone(),inbound.target,other_hz,own_hz,0x87654321));
        let caller_media=caller_media?;let callee_media=callee_media?;
        dialog.bye().await.map_err(|_|CallProbeError::Signaling)?;
        caller.wire.wait(4).await;callee.wire.wait(2).await;
        caller.wait_phase(true).await?;callee.wait_phase(true).await?;
        caller.unregister().await?;callee.unregister().await?;
        let (first,second)=if reverse{(callee_media,caller_media)}else{(caller_media,callee_media)};
        Ok(CallProbeReport{transport:label,codec:codec.name(),reverse,first,second,ack_and_termination_verified:true,bye_200_verified:true,unregistered:true})
    })=>result.map_err(|_|CallProbeError::Timeout)?,}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packet_tracker_checks_rollover_loss_late_and_timestamp() -> Result<(), CallProbeError> {
        let header = RtpHeader {
            marker: false,
            payload_type: 8,
            sequence: u16::MAX,
            timestamp: u32::MAX - 79,
            ssrc: 7,
        };
        let mut tracker = PacketTracker::default();
        assert_eq!(tracker.observe(header)?, Some(0));
        let next = RtpHeader {
            sequence: 0,
            timestamp: 80,
            ..header
        };
        assert_eq!(tracker.observe(next)?, Some(0));
        assert_eq!(tracker.observe(next)?, None);
        assert_eq!(tracker.observe(header)?, None);
        assert_eq!(
            tracker.observe(RtpHeader {
                sequence: 2,
                timestamp: 400,
                ..header
            })?,
            Some(1)
        );
        assert_eq!(
            tracker.observe(RtpHeader {
                sequence: 3,
                timestamp: 400,
                ..header
            }),
            Err(CallProbeError::Media)
        );
        assert_eq!(
            tracker.observe(RtpHeader {
                sequence: 3,
                timestamp: 560,
                ssrc: 8,
                ..header
            }),
            Err(CallProbeError::Media)
        );
        Ok(())
    }

    fn fixture(
        user: &str,
        transport: Transport,
    ) -> Result<RegistrationConfig, super::super::RegistrationError> {
        RegistrationConfig::new(
            SocketAddr::from(([127, 0, 0, 1], 5060)),
            transport,
            user.into(),
            secrecy::SecretString::from("fictional"),
            Duration::from_secs(2),
        )
    }

    #[tokio::test]
    async fn rejects_secure_accounts_and_cancellation_before_network()
    -> Result<(), Box<dyn std::error::Error>> {
        let tls = || Transport::Tls {
            ca_pem: vec![1],
            server_name: "pbx".into(),
        };
        let result = probe_plain_call_pair(
            fixture("1201", tls())?,
            fixture("1202", tls())?,
            Codec::Pcma,
            false,
            CancellationToken::new(),
        )
        .await;
        assert!(matches!(result, Err(CallProbeError::InvalidConfig)));
        let stop = CancellationToken::new();
        stop.cancel();
        let result = probe_plain_call_pair(
            fixture("1001", Transport::Udp)?,
            fixture("1002", Transport::Udp)?,
            Codec::Pcma,
            false,
            stop,
        )
        .await;
        assert!(matches!(result, Err(CallProbeError::Cancelled)));
        let result = probe_plain_call_pair(
            fixture("1001", Transport::Udp)?,
            fixture("1002", Transport::Tcp)?,
            Codec::Pcmu,
            true,
            CancellationToken::new(),
        )
        .await;
        assert!(matches!(result, Err(CallProbeError::InvalidConfig)));
        Ok(())
    }

    #[test]
    fn tone_measurement_accepts_only_the_other_endpoint_tone() {
        for codec in [Codec::Pcma, Codec::Pcmu] {
            for frequency in [0.0, 440.0, 880.0, 1700.0] {
                let mut meter = ToneMeter::new(440.0, 880.0);
                for frame in 0..50 {
                    let payload: [u8; 160] = std::array::from_fn(|i| {
                        let phase =
                            std::f64::consts::TAU * frequency * (frame * 160 + i) as f64 / 8000.0;
                        codec.encode_sample((8000.0 * phase.sin()) as i16)
                    });
                    meter.observe(codec, &payload);
                }
                assert_eq!(meter.finish().is_ok(), frequency == 880.0);
            }
        }
    }

    #[test]
    fn sdp_is_bounded_codec_specific_and_pbx_anchored() -> Result<(), Box<dyn std::error::Error>> {
        let pbx = "192.0.2.10".parse()?;
        let address = "192.0.2.10:10000".parse()?;
        assert_eq!(
            parse_offer(&offer(address, Codec::Pcma), Codec::Pcma, pbx)?.address,
            address
        );
        for bytes in [
            offer(address, Codec::Pcmu),
            offer("192.0.2.11:10000".parse()?, Codec::Pcma),
            offer("192.0.2.10:20000".parse()?, Codec::Pcma),
            vec![b'x'; 8193],
            [
                offer(address, Codec::Pcma),
                b"a=crypto:1 forbidden\r\n".to_vec(),
            ]
            .concat(),
        ] {
            assert_eq!(
                parse_offer(&bytes, Codec::Pcma, pbx).err(),
                Some(CallProbeError::UnsupportedSdp)
            );
        }
        Ok(())
    }
}
