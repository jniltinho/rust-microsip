//! Initial TLS/SDES synthetic-call experiment, not a production call actor.
//! Built only after explicit copying into the inventoried candidate workspace.
//! Re-INVITE/REFER, changing sources, SRTCP and audio devices are not enabled.

#[path = "sdes.rs"]
mod sdes;

use super::{
    CallProbeError, CallProbeReport, CallWire, MediaMeasurement, PacketTracker, Phase, ToneMeter,
};
use crate::{OwnedRegistration, ProbeTasks, RegistrationConfig, Transport};
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
    transport::{SipAddr, SipConnection, TransportLayer, tls::TlsConfig},
};
use rust_microsip_media::{
    Codec, RtpHeader, RtpPacket,
    srtp_adapter::{SecureError, SecureReceiver, SecureSender},
};
use secrecy::ExposeSecret;
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::UdpSocket,
    sync::{mpsc, watch},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

fn signaling<E>(_: E) -> CallProbeError {
    CallProbeError::Signaling
}
fn media<E>(_: E) -> CallProbeError {
    CallProbeError::Media
}
fn description<E>(_: E) -> CallProbeError {
    CallProbeError::UnsupportedSdp
}

struct Inspector(Arc<CallWire>, Arc<AtomicBool>);
impl MessageInspector for Inspector {
    fn before_send(&self, message: SipMessage, _: Option<&SipAddr>) -> SipMessage {
        message
    }
    fn after_received(&self, message: SipMessage, _: Option<&SipAddr>) -> SipMessage {
        match &message {
            SipMessage::Request(r) if r.method == Method::Invite => {
                self.1.store(true, Ordering::SeqCst)
            }
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
        message
    }
}
#[derive(Default)]
struct Admission(AtomicBool);
impl Admission {
    // No other fields are published: only one initial dialog may use this key.
    fn claim(&self) -> bool {
        !self.0.swap(true, Ordering::Relaxed)
    }
}

struct Incoming {
    dialog: InviteDialog,
    target: sdes::Target,
}
struct EndpointOwner {
    owner: ProbeTasks,
    endpoint: Endpoint,
    layer: Arc<DialogLayer>,
    registration: OwnedRegistration,
    server: Uri,
    contact: Uri,
    username: String,
    rtp: Arc<UdpSocket>,
    local_key: Option<sdes::LocalKey>,
    admission: Arc<Admission>,
    saw_invite: Arc<AtomicBool>,
    incoming: mpsc::Receiver<Incoming>,
    phase: watch::Receiver<Phase>,
    states: rsipstack::dialog::dialog::DialogStateSender,
    wire: Arc<CallWire>,
}
impl EndpointOwner {
    async fn connect(
        config: RegistrationConfig,
        codec: Codec,
        stop: CancellationToken,
    ) -> Result<Self, CallProbeError> {
        let Transport::Tls {
            ca_pem,
            server_name,
        } = config.transport
        else {
            return Err(CallProbeError::InvalidConfig);
        };
        let mut owner = ProbeTasks {
            stop: stop.child_token(),
            tasks: JoinSet::new(),
        };
        // Only a TLS client connection is installed: no UDP/TCP listener/fallback.
        let transport = TransportLayer::new(owner.stop.clone());
        transport.set_tls_config(TlsConfig {
            ca_certs: Some(ca_pem),
            sni_hostname: Some(server_name),
            ..Default::default()
        });
        let server: Uri = format!("sip:{};transport=tls", config.address)
            .parse()
            .map_err(signaling)?;
        let destination = SipAddr::try_from(&server).map_err(signaling)?;
        let (connection, _) = transport
            .lookup(&destination, None)
            .await
            .map_err(signaling)?;
        if !matches!(connection, SipConnection::Tls(_))
            || connection
                .get_remote_addr()
                .and_then(|a| a.get_socketaddr().ok())
                != Some(config.address)
        {
            return Err(CallProbeError::Signaling);
        }
        let local = connection.get_addr().get_socketaddr().map_err(signaling)?;
        let contact: Uri = format!("sip:{}@{local};transport=tls", config.username)
            .parse()
            .map_err(signaling)?;
        let wire = Arc::new(CallWire::default());
        let saw_invite = Arc::new(AtomicBool::new(false));
        let endpoint = EndpointBuilder::new()
            .with_cancel_token(owner.stop.clone())
            .with_transport_layer(transport)
            .with_user_agent("rust-microsip isolated SDES experiment")
            .with_inspector(Box::new(Inspector(wire.clone(), saw_invite.clone())))
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
            UdpSocket::bind(SocketAddr::new(local.ip(), 0))
                .await
                .map_err(media)?,
        );
        let local_rtp = rtp.local_addr().map_err(media)?;
        let local_key = sdes::LocalKey::generate().map_err(description)?;
        // SDK signaling bodies necessarily copy the key: zeroization is not guaranteed there.
        let answer_key = local_key.description(local_rtp, codec, 1);
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
            let mut count = 0u8;
            loop {
                let event =
                    tokio::select! {biased;_=stopped.cancelled()=>break,event=events.recv()=>event};
                let Some(event) = event else { break };
                count = count.saturating_add(1);
                if count > 64 {
                    phase_tx.send_modify(|p| p.failed = true);
                    stopped.cancel();
                    break;
                }
                if let DialogState::Updated(_, _, handle) = &event {
                    // No reset of crypto/replay state or plaintext response on renegotiation.
                    if handle.reply(StatusCode::NotAcceptableHere).await.is_err() {
                        phase_tx.send_modify(|p| p.failed = true);
                        stopped.cancel();
                        break;
                    }
                }
                phase_tx.send_modify(|p| match event {
                    DialogState::Confirmed(..) => p.confirmed = true,
                    DialogState::Terminated(..) => p.terminated = true,
                    _ => (),
                });
            }
        });
        let mut transactions = endpoint.incoming_transactions().map_err(signaling)?;
        let (incoming_tx, incoming) = mpsc::channel(1);
        let incoming_layer = layer.clone();
        let incoming_states = states.clone();
        let incoming_contact = contact.clone();
        let stopped = owner.stop.clone();
        let pbx = config.address;
        let admission = Arc::new(Admission::default());
        let incoming_admission = admission.clone();
        owner.tasks.spawn(async move {
            let result=async {
                let mut count=0u16;
                loop {tokio::select! {biased;_=stopped.cancelled()=>return Ok::<(),CallProbeError>(()),item=transactions.recv()=>{
                    let Some(mut tx)=item else {return Err(CallProbeError::Signaling)};
                    count+=1;if count>64 {return Err(CallProbeError::Signaling);}
                    if !tx.connection.as_ref().is_some_and(|c|matches!(c,SipConnection::Tls(_)) && c.get_remote_addr().and_then(|a|a.get_socketaddr().ok())==Some(pbx)) {return Err(CallProbeError::Signaling);}
                    if let Some(Dialog::Invite(mut dialog))=incoming_layer.match_dialog(&tx) {dialog.handle(&mut tx).await.map_err(signaling)?;}
                    else if tx.original.method==Method::Invite && !incoming_admission.0.load(Ordering::Relaxed) {
                        let target=match sdes::parse(&tx.original.body,codec,pbx.ip(),None) {Ok(t)=>t,Err(_)=>{tx.reply(StatusCode::NotAcceptableHere).await.map_err(signaling)?;continue;}};
                        // The narrow initial fixture accepts the single tag emitted by this PBX.
                        // A nonmatching tag is rejected, never silently answered with a new tag.
                        if target.tag!=1 {tx.reply(StatusCode::NotAcceptableHere).await.map_err(signaling)?;continue;}
                        if !incoming_admission.claim() {tx.reply(StatusCode::BusyHere).await.map_err(signaling)?;continue;}
                        let mut dialog=incoming_layer.get_or_create_server_invite(&tx,incoming_states.clone(),None,Some(incoming_contact.clone())).map_err(signaling)?;
                        dialog.ringing(None,None).map_err(signaling)?;
                        dialog.accept(Some(vec![Header::ContentType("application/sdp".into())]),Some(answer_key.to_vec())).map_err(signaling)?;
                        incoming_tx.try_send(Incoming {dialog:dialog.clone(),target}).map_err(signaling)?;
                        dialog.handle(&mut tx).await.map_err(signaling)?;
                    } else {
                        let status=if tx.original.method==Method::Options {StatusCode::OK} else if tx.original.method==Method::Invite {StatusCode::BusyHere} else {StatusCode::CallTransactionDoesNotExist};
                        tx.reply(status).await.map_err(signaling)?;
                    }
                }}}
            }.await;
            if result.is_err() {failed.send_modify(|p|p.failed=true);stopped.cancel();}
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
            .map_err(signaling)?;
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
            local_key: Some(local_key),
            admission,
            saw_invite,
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
            self.phase.changed().await.map_err(signaling)?;
        }
    }
    async fn unregister(&mut self) -> Result<(), CallProbeError> {
        let response = self
            .registration
            .0
            .register(self.server.clone(), Some(0))
            .await
            .map_err(signaling)?;
        if response.status_code != StatusCode::OK {
            return Err(CallProbeError::Signaling);
        }
        self.endpoint.shutdown();
        self.owner.stop.cancel();
        Ok(())
    }
    fn invite(&self, callee: &Self, body: Vec<u8>) -> Result<InviteOption, CallProbeError> {
        if !self.admission.claim() {
            return Err(CallProbeError::Signaling);
        }
        Ok(InviteOption {
            caller: format!("sip:{}@lab.invalid", self.username)
                .parse()
                .map_err(signaling)?,
            callee: format!("sip:{}@lab.invalid", callee.username)
                .parse()
                .map_err(signaling)?,
            contact: self.contact.clone(),
            destination: Some(SipAddr::try_from(&self.server).map_err(signaling)?),
            credential: self.registration.0.credential.clone(),
            content_type: Some("application/sdp".into()),
            offer: Some(body),
            ..Default::default()
        })
    }
}

async fn measure(
    socket: Arc<UdpSocket>,
    target: sdes::Target,
    codec: Codec,
    local_key: sdes::LocalKey,
    own_hz: f64,
    remote_hz: f64,
    ssrc: u32,
) -> Result<MediaMeasurement, CallProbeError> {
    let mut sender =
        SecureSender::new(ssrc, local_key.into_master().map_err(media)?).map_err(media)?;
    let mut receiver = SecureReceiver::new(target.key).map_err(media)?;
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
    let mut receive = [0; 1515];
    let mut sample_index = 0u32;
    let mut sequence = 65500u16;
    let mut timestamp = u32::MAX - 3200;
    let mut tracker = PacketTracker::default();
    let mut meter = ToneMeter::new(own_hz, remote_hz);
    let mut checked_negatives = false;
    loop {
        tokio::select! {biased;_=tokio::time::sleep_until(end)=>break,_=tick.tick()=>{
            let mut payload=[0;160];for octet in &mut payload {let phase=std::f64::consts::TAU*own_hz*f64::from(sample_index%8000)/8000.0;*octet=codec.encode_sample((8000.0*phase.sin()) as i16);sample_index=sample_index.wrapping_add(1);}
            let length=RtpHeader {marker:stats.sent_packets==0,payload_type:codec.payload_type(),sequence,timestamp,ssrc}.write(&payload,&mut send).map_err(media)?;
            let encrypted=sender.protect_rtp(&send[..length]).map_err(media)?;
            if encrypted.len()!=length+10 || encrypted[12..length]==send[12..length] {return Err(CallProbeError::Media);}
            if socket.send_to(&encrypted,target.address).await.map_err(media)?!=encrypted.len() {return Err(CallProbeError::Media);}
            stats.sent_packets+=1;sequence=sequence.wrapping_add(1);timestamp=timestamp.wrapping_add(160);
        }, result=socket.recv_from(&mut receive)=>{
            let (length,source)=result.map_err(media)?;
            if source!=target.address || length!=182 {return Err(CallProbeError::Media);}
            if !checked_negatives {
                let mut wrong=SecureReceiver::new(sdes::LocalKey::generate().map_err(media)?.into_master().map_err(media)?).map_err(media)?;
                if wrong.unprotect_rtp(&receive[..length])!=Err(SecureError::Authentication) || wrong.authenticated_ssrc().is_some() {return Err(CallProbeError::Media);}
                let mut forged=receive[..length].to_vec();forged[length-1]^=1;
                if receiver.unprotect_rtp(&forged)!=Err(SecureError::Authentication) || receiver.authenticated_ssrc().is_some() {return Err(CallProbeError::Media);}
                forged.copy_from_slice(&receive[..length]);forged[2]^=0x40;
                if receiver.unprotect_rtp(&forged)!=Err(SecureError::Authentication) || receiver.authenticated_ssrc().is_some() {return Err(CallProbeError::Media);}
                // A valid-shaped clear packet must not enter a mandatory-SRTP receiver.
                if receiver.unprotect_rtp(&send).is_ok() || receiver.authenticated_ssrc().is_some() {return Err(CallProbeError::Media);}
            }
            let clear=receiver.unprotect_rtp(&receive[..length]).map_err(media)?;
            if !checked_negatives {if receiver.unprotect_rtp(&receive[..length])!=Err(SecureError::Replay) {return Err(CallProbeError::Media);}checked_negatives=true;}
            let packet=RtpPacket::parse(&clear).map_err(media)?;
            if packet.header.payload_type!=codec.payload_type() || packet.payload.len()!=160 || clear[12..]==receive[12..172] {return Err(CallProbeError::Media);}
            let Some(lost)=tracker.observe(packet.header)? else {stats.late_packets+=1;continue;};stats.lost_packets+=lost;stats.received_packets+=1;meter.observe(codec,packet.payload);
        }}
    }
    if stats.sent_packets < 240 || stats.received_packets < 200 || !checked_negatives {
        return Err(CallProbeError::Media);
    }
    (stats.rms, stats.remote_to_own_power_ratio) = meter.finish()?;
    Ok(stats)
}

fn fixtures(first: &RegistrationConfig, second: &RegistrationConfig) -> Result<(), CallProbeError> {
    if first.address != second.address
        || first.address.port() != 5061
        || first.username != "1201"
        || second.username != "1202"
        || !matches!(first.transport, Transport::Tls { .. })
        || !matches!(second.transport, Transport::Tls { .. })
    {
        return Err(CallProbeError::InvalidConfig);
    }
    Ok(())
}

/// Exercise one five-second initial TLS/SDES G.711 call between fictional peers.
/// No plaintext fallback, profile discovery, devices, recording or rekey is used.
/// The first authentic PBX packet also drives local tamper/clear/replay negatives.
/// This is initial RTP interoperability only, not SRTCP/hold/transfer acceptance.
///
/// # Errors
/// Returns a sanitized error unless signaling, encrypted media and teardown pass.
pub async fn probe_secure_call_pair(
    first: RegistrationConfig,
    second: RegistrationConfig,
    codec: Codec,
    reverse: bool,
    cancel: CancellationToken,
) -> Result<CallProbeReport, CallProbeError> {
    fixtures(&first, &second)?;
    if cancel.is_cancelled() {
        return Err(CallProbeError::Cancelled);
    }
    let stop = cancel.child_token();
    tokio::select! {biased;_=cancel.cancelled()=>Err(CallProbeError::Cancelled),result=tokio::time::timeout(Duration::from_secs(30),async {
        let mut a=EndpointOwner::connect(first,codec,stop.clone()).await?;let mut b=EndpointOwner::connect(second,codec,stop.clone()).await?;
        let (caller,callee)=if reverse {(&mut b,&mut a)} else {(&mut a,&mut b)};
        let offer=caller.local_key.as_ref().ok_or(CallProbeError::Media)?.description(caller.rtp.local_addr().map_err(media)?,codec,1);
        let (dialog,response)=caller.layer.do_invite(caller.invite(callee,offer.to_vec())?,caller.states.clone()).await.map_err(signaling)?;
        let response=response.ok_or(CallProbeError::Signaling)?;if response.status_code!=StatusCode::OK {return Err(CallProbeError::Signaling);}
        let pbx=caller.server.host_with_port.host.to_string().parse().map_err(signaling)?;
        let remote=sdes::parse(&response.body,codec,pbx,Some(1)).map_err(description)?;
        let inbound=callee.incoming.recv().await.ok_or(CallProbeError::Signaling)?;
        caller.wait_phase(false).await?;callee.wait_phase(false).await?;callee.wire.wait(1).await;
        if !inbound.dialog.state().is_confirmed() || remote.address==inbound.target.address {return Err(CallProbeError::Signaling);}
        // Move each non-cloneable local key exactly once into its transmitter.
        let caller_key=caller.local_key.take().ok_or(CallProbeError::Media)?;
        let callee_key=callee.local_key.take().ok_or(CallProbeError::Media)?;
        let (own_hz,remote_hz)=if reverse {(880.0,440.0)} else {(440.0,880.0)};
        let (one,two)=tokio::join!(measure(caller.rtp.clone(),remote,codec,caller_key,own_hz,remote_hz,0x12345678),measure(callee.rtp.clone(),inbound.target,codec,callee_key,remote_hz,own_hz,0x87654321));
        let one=one?;let two=two?;dialog.bye().await.map_err(signaling)?;caller.wire.wait(4).await;callee.wire.wait(2).await;
        caller.wait_phase(true).await?;callee.wait_phase(true).await?;caller.unregister().await?;callee.unregister().await?;
        let (first,second)=if reverse {(two,one)} else {(one,two)};
        Ok(CallProbeReport {transport:"tls",codec:codec.name(),reverse,first,second,ack_and_termination_verified:true,bye_200_verified:true,unregistered:true})
    })=>result.map_err(|_|CallProbeError::Timeout)?,}
}

/// The exact certificate-validation failure required by a negative TLS probe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertificateRejection {
    /// A syntactically valid unrelated CA does not authenticate the PBX leaf.
    UntrustedAuthority,
    /// A trusted PBX leaf does not authenticate the requested DNS identity.
    WrongIdentity,
}
fn certificate_rejection(error: &rsipstack::Error) -> Option<CertificateRejection> {
    let rsipstack::Error::IoError(io) = error else {
        return None;
    };
    let tls = io.get_ref()?.downcast_ref::<rustls::Error>()?;
    match tls {
        rustls::Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer) => {
            Some(CertificateRejection::UntrustedAuthority)
        }
        rustls::Error::InvalidCertificate(
            rustls::CertificateError::NotValidForName
            | rustls::CertificateError::NotValidForNameContext { .. },
        ) => Some(CertificateRejection::WrongIdentity),
        _ => None,
    }
}

/// Require an exact typed TLS certificate error, without constructing a SIP endpoint.
/// No REGISTER/INVITE API is reached: unrelated I/O or signaling failures fail the probe.
///
/// # Errors
/// Returns a sanitized error on acceptance, wrong failure category, timeout or cancellation.
pub async fn probe_tls_certificate_rejection(
    config: RegistrationConfig,
    expected: CertificateRejection,
    cancel: CancellationToken,
) -> Result<(), CallProbeError> {
    if config.username != "1201" || config.address.port() != 5061 {
        return Err(CallProbeError::InvalidConfig);
    }
    let Transport::Tls {
        ca_pem,
        server_name,
    } = config.transport
    else {
        return Err(CallProbeError::InvalidConfig);
    };
    if cancel.is_cancelled() {
        return Err(CallProbeError::Cancelled);
    }
    let stop = cancel.child_token();
    let transport = TransportLayer::new(stop.clone());
    transport.set_tls_config(TlsConfig {
        ca_certs: Some(ca_pem),
        sni_hostname: Some(server_name),
        ..Default::default()
    });
    let uri: Uri = format!("sip:{};transport=tls", config.address)
        .parse()
        .map_err(signaling)?;
    let destination = SipAddr::try_from(&uri).map_err(signaling)?;
    tokio::select! {biased;_=cancel.cancelled()=>Err(CallProbeError::Cancelled),result=tokio::time::timeout(Duration::from_secs(10),transport.lookup(&destination,None))=>{
        stop.cancel();
        match result {Ok(Err(error)) if certificate_rejection(&error)==Some(expected)=>Ok(()),Ok(_)=>Err(CallProbeError::Signaling),Err(_)=>Err(CallProbeError::Timeout)}
    }}
}

/// A specific malformed/downgraded offer sent only to the disposable PBX.
#[derive(Clone, Copy, Debug)]
pub enum NegativeOffer {
    /// Propose clear RTP to an endpoint with mandatory encryption.
    Plaintext,
    /// Propose SAVP but omit the crypto attribute.
    MissingCrypto,
    /// Offer only an unsupported crypto suite, without a supported alternative.
    UnsupportedSuite,
}

/// Require a real 488 response to a forbidden offer and unregister both peers.
///
/// # Errors
/// Returns a sanitized failure for acceptance, timeout or incomplete teardown.
pub async fn probe_rejected_offer(
    first: RegistrationConfig,
    second: RegistrationConfig,
    kind: NegativeOffer,
    cancel: CancellationToken,
) -> Result<(), CallProbeError> {
    fixtures(&first, &second)?;
    if cancel.is_cancelled() {
        return Err(CallProbeError::Cancelled);
    }
    let stop = cancel.child_token();
    tokio::select! {biased;_=cancel.cancelled()=>Err(CallProbeError::Cancelled),result=tokio::time::timeout(Duration::from_secs(30),async {
        let mut a=EndpointOwner::connect(first,Codec::Pcma,stop.clone()).await?;let mut b=EndpointOwner::connect(second,Codec::Pcma,stop.clone()).await?;
        let good=a.local_key.as_ref().ok_or(CallProbeError::Media)?.description(a.rtp.local_addr().map_err(media)?,Codec::Pcma,1);
        let text=Zeroizing::new(std::str::from_utf8(&good).map_err(description)?.to_owned());
        let bad=Zeroizing::new(match kind {
            NegativeOffer::Plaintext=>text.split_inclusive("\r\n").filter(|l|!l.starts_with("a=crypto:")).collect::<String>().replace("RTP/SAVP","RTP/AVP"),
            NegativeOffer::MissingCrypto=>text.split_inclusive("\r\n").filter(|l|!l.starts_with("a=crypto:")).collect(),
            NegativeOffer::UnsupportedSuite=>text.replace("AES_CM_128_HMAC_SHA1_80","UNSUPPORTED_TEST_SUITE"),
        });
        let (_,response)=a.layer.do_invite(a.invite(&b,bad.as_bytes().to_vec())?,a.states.clone()).await.map_err(signaling)?;
        if response.ok_or(CallProbeError::Signaling)?.status_code!=StatusCode::NotAcceptableHere || b.saw_invite.load(Ordering::SeqCst) {return Err(CallProbeError::Signaling);}
        a.unregister().await?;b.unregister().await?;Ok(())
    })=>result.map_err(|_|CallProbeError::Timeout)?,}
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::SecretString;
    fn config(username: &str, transport: Transport) -> RegistrationConfig {
        RegistrationConfig {
            address: SocketAddr::from(([192, 0, 2, 1], 5061)),
            transport,
            username: username.into(),
            password: SecretString::from("fictional-test-secret"),
            deadline: Duration::from_secs(30),
        }
    }
    fn tls() -> Transport {
        Transport::Tls {
            ca_pem: vec![1],
            server_name: "pbx".into(),
        }
    }
    #[test]
    fn tls_negative_oracle_rejects_unrelated_failures() {
        let typed = |e| {
            rsipstack::Error::IoError(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                rustls::Error::InvalidCertificate(e),
            ))
        };
        assert_eq!(
            certificate_rejection(&typed(rustls::CertificateError::UnknownIssuer)),
            Some(CertificateRejection::UntrustedAuthority)
        );
        assert_eq!(
            certificate_rejection(&typed(rustls::CertificateError::NotValidForName)),
            Some(CertificateRejection::WrongIdentity)
        );
        assert_eq!(
            certificate_rejection(&typed(rustls::CertificateError::Expired)),
            None
        );
        assert_eq!(
            certificate_rejection(&rsipstack::Error::IoError(std::io::Error::from(
                std::io::ErrorKind::ConnectionRefused
            ))),
            None
        );
        assert_eq!(
            certificate_rejection(&rsipstack::Error::Error("unrelated fixture failure".into())),
            None
        );
    }
    #[test]
    fn one_initial_dialog_per_local_key() {
        let admission = Admission::default();
        assert!(admission.claim());
        assert!(!admission.claim());
        assert!(!admission.claim());
    }
    #[test]
    fn rejects_plaintext_transport_and_nonfixture_accounts() {
        assert_eq!(
            fixtures(&config("1201", Transport::Udp), &config("1202", tls())),
            Err(CallProbeError::InvalidConfig)
        );
        assert_eq!(
            fixtures(&config("1201", tls()), &config("1202", Transport::Tcp)),
            Err(CallProbeError::InvalidConfig)
        );
        assert_eq!(
            fixtures(&config("1301", tls()), &config("1302", tls())),
            Err(CallProbeError::InvalidConfig)
        );
        assert!(fixtures(&config("1201", tls()), &config("1202", tls())).is_ok());
    }
    #[tokio::test]
    async fn precancelled_probe_does_not_connect() {
        let stop = CancellationToken::new();
        stop.cancel();
        assert!(matches!(
            probe_secure_call_pair(
                config("1201", tls()),
                config("1202", tls()),
                Codec::Pcma,
                false,
                stop
            )
            .await,
            Err(CallProbeError::Cancelled)
        ));
    }
}
