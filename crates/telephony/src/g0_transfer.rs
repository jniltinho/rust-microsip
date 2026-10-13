//! Isolated REFER/NOTIFY evaluation; not the production transfer actor.
use super::CallProbeError;
use rsipstack::{
    dialog::DialogId,
    sip::{Header, HeadersExt, Method, Request, Response, SipMessage},
    transaction::key::TransactionRole,
};
use std::sync::Mutex;
use tokio::sync::watch;

fn sipfrag(bytes: &[u8]) -> Result<u16, CallProbeError> {
    let bad = CallProbeError::Signaling;
    if bytes.is_empty() || bytes.len() > 1024 {
        return Err(bad);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| CallProbeError::Signaling)?;
    for (n, line) in text.lines().enumerate() {
        if n >= 16
            || line.len() > 256
            || !line.bytes().all(|b| b == b'\t' || (32..=126).contains(&b))
        {
            return Err(bad);
        }
    }
    let line = text.lines().next().ok_or(CallProbeError::Signaling)?;
    let status = line
        .strip_prefix("SIP/2.0 ")
        .ok_or(CallProbeError::Signaling)?;
    if status.len() < 4
        || status.as_bytes()[3] != b' '
        || !status.as_bytes()[..3].iter().all(u8::is_ascii_digit)
    {
        return Err(bad);
    }
    let code = status[..3]
        .parse::<u16>()
        .map_err(|_| CallProbeError::Signaling)?;
    if !(100..=699).contains(&code) {
        return Err(bad);
    }
    Ok(code)
}
fn refer_event(text: &str, seq: u32) -> Result<(), CallProbeError> {
    if text.len() > 256 {
        return Err(CallProbeError::Signaling);
    }
    let mut parts = text.split(';');
    if !parts
        .next()
        .is_some_and(|s| s.trim().eq_ignore_ascii_case("refer"))
    {
        return Err(CallProbeError::Signaling);
    }
    let mut id = false;
    for part in parts {
        let (name, value) = part
            .trim()
            .split_once('=')
            .ok_or(CallProbeError::Signaling)?;
        if id
            || !name.eq_ignore_ascii_case("id")
            || value.is_empty()
            || !value.bytes().all(|b| b.is_ascii_digit())
            || value.parse::<u32>().ok() != Some(seq)
        {
            return Err(CallProbeError::Signaling);
        }
        id = true;
    }
    Ok(())
}
fn terminated(text: &str) -> Result<bool, CallProbeError> {
    if text.len() > 256 {
        return Err(CallProbeError::Signaling);
    }
    let mut parts = text.split(';');
    let state = parts.next().ok_or(CallProbeError::Signaling)?.trim();
    let end = state.eq_ignore_ascii_case("terminated");
    if !end && !state.eq_ignore_ascii_case("active") && !state.eq_ignore_ascii_case("pending") {
        return Err(CallProbeError::Signaling);
    }
    let mut parameter = false;
    for part in parts {
        let (name, value) = part
            .trim()
            .split_once('=')
            .ok_or(CallProbeError::Signaling)?;
        if parameter {
            return Err(CallProbeError::Signaling);
        }
        if end {
            if !name.eq_ignore_ascii_case("reason") || !value.eq_ignore_ascii_case("noresource") {
                return Err(CallProbeError::Signaling);
            }
        } else if !name.eq_ignore_ascii_case("expires")
            || value.is_empty()
            || !value.bytes().all(|b| b.is_ascii_digit())
            || value.parse::<u32>().is_err()
        {
            return Err(CallProbeError::Signaling);
        }
        parameter = true;
    }
    Ok(end)
}
fn content_type(text: &str) -> Result<(), CallProbeError> {
    if text.len() > 128 {
        return Err(CallProbeError::Signaling);
    }
    let mut parts = text.split(';');
    if !parts
        .next()
        .is_some_and(|s| s.trim().eq_ignore_ascii_case("message/sipfrag"))
    {
        return Err(CallProbeError::Signaling);
    }
    if let Some(version) = parts.next()
        && !version.trim().eq_ignore_ascii_case("version=2.0")
    {
        return Err(CallProbeError::Signaling);
    }
    if parts.next().is_some() {
        return Err(CallProbeError::Signaling);
    }
    Ok(())
}
fn field<'a>(
    request: &'a Request,
    pick: fn(&'a Header) -> Option<&'a str>,
) -> Result<&'a str, CallProbeError> {
    let mut values = request.headers.iter().filter_map(pick);
    let value = values.next().ok_or(CallProbeError::Signaling)?;
    if values.next().is_some() {
        return Err(CallProbeError::Signaling);
    }
    Ok(value)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Outcome {
    pub final_status: u16,
    pub notify_count: u8,
    pub provisional_count: u8,
}
#[derive(Clone, Copy, Default)]
struct Update {
    result: Option<Outcome>,
    failed: bool,
}
#[derive(Default)]
struct State {
    dialog: Option<DialogId>,
    refer_seq: u32,
    sent_refer: u8,
    notify_seq: u32,
    acked_seq: u32,
    notify_count: u8,
    provisional: u8,
    status: u16,
    terminal: bool,
    accepted: bool,
    failed: bool,
}
pub(super) struct Subscription {
    state: Mutex<State>,
    changed: watch::Sender<Update>,
}
impl Default for Subscription {
    fn default() -> Self {
        Self {
            state: Mutex::new(State::default()),
            changed: watch::channel(Update::default()).0,
        }
    }
}
impl Subscription {
    fn arm(&self, dialog: DialogId) -> Result<(), CallProbeError> {
        if [&dialog.call_id, &dialog.local_tag, &dialog.remote_tag]
            .iter()
            .any(|s| s.is_empty() || s.len() > 256)
        {
            return Err(CallProbeError::Signaling);
        }
        let mut state = self.state.lock().map_err(|_| CallProbeError::Signaling)?;
        if state.dialog.is_some() {
            return Err(CallProbeError::Signaling);
        }
        state.dialog = Some(dialog);
        Ok(())
    }
    fn publish(&self, state: &State) {
        let result = (state.accepted && state.terminal && state.acked_seq == state.notify_seq)
            .then_some(Outcome {
                final_status: state.status,
                notify_count: state.notify_count,
                provisional_count: state.provisional,
            });
        self.changed.send_replace(Update {
            result,
            failed: state.failed,
        });
    }
    pub(super) fn sent(&self, message: &SipMessage) {
        if let SipMessage::Response(response) = message {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            let seq = response
                .cseq_header()
                .ok()
                .and_then(|h| rsipstack::sip::typed::CSeq::parse(h.value()).ok());
            if response.status_code == rsipstack::sip::StatusCode::OK
                && seq
                    .as_ref()
                    .is_some_and(|h| h.method == Method::Notify && h.seq == state.notify_seq)
                && state.dialog.as_ref().map(|d| d.call_id.as_str())
                    == response.call_id_header().ok().map(|h| h.value())
            {
                state.acked_seq = state.notify_seq;
                self.publish(&state);
            }
            return;
        }
        let SipMessage::Request(request) = message else {
            return;
        };
        if request.method != Method::Refer {
            return;
        }
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some(dialog) = &state.dialog else {
            return;
        };
        if request.call_id_header().ok().map(|h| h.value()) != Some(dialog.call_id.as_str()) {
            return;
        }
        let seq = request
            .cseq_header()
            .ok()
            .and_then(|h| rsipstack::sip::typed::CSeq::parse(h.value()).ok())
            .filter(|h| h.method == Method::Refer && h.seq > 0)
            .map(|h| h.seq);
        if let Some(seq) = seq {
            if seq > state.refer_seq {
                state.sent_refer = state.sent_refer.saturating_add(1);
                state.failed |= state.sent_refer > 4 || state.notify_count > 0;
                state.refer_seq = seq;
            } else if seq != state.refer_seq {
                state.failed = true;
            }
        } else {
            state.failed = true;
        }
        self.publish(&state);
    }
    pub(super) fn consume(
        &self,
        dialog: &DialogId,
        request: &Request,
    ) -> Result<(), CallProbeError> {
        let mut state = self.state.lock().map_err(|_| CallProbeError::Signaling)?;
        if request.method != Method::Notify
            || state.dialog.as_ref() != Some(dialog)
            || state.refer_seq == 0
            || state.failed
        {
            return Err(CallProbeError::Signaling);
        }
        if field(request, |h| {
            if let Header::CallId(v) = h {
                Some(v.value())
            } else {
                None
            }
        })? != dialog.call_id
        {
            return Err(CallProbeError::Signaling);
        }
        field(request, |h| {
            if let Header::From(v) = h {
                Some(v.value())
            } else {
                None
            }
        })?;
        field(request, |h| {
            if let Header::To(v) = h {
                Some(v.value())
            } else {
                None
            }
        })?;
        if DialogId::try_from((request, TransactionRole::Server))
            .ok()
            .as_ref()
            != Some(dialog)
        {
            return Err(CallProbeError::Signaling);
        }
        refer_event(
            field(request, |h| {
                if let Header::Event(v) = h {
                    Some(v.value())
                } else {
                    None
                }
            })?,
            state.refer_seq,
        )?;
        content_type(field(request, |h| {
            if let Header::ContentType(v) = h {
                Some(v.value())
            } else {
                None
            }
        })?)?;
        let terminal = terminated(field(request, |h| {
            if let Header::SubscriptionState(v) = h {
                Some(v.value())
            } else {
                None
            }
        })?)?;
        let seq = rsipstack::sip::typed::CSeq::parse(field(request, |h| {
            if let Header::CSeq(v) = h {
                Some(v.value())
            } else {
                None
            }
        })?)
        .map_err(|_| CallProbeError::Signaling)?;
        let status = sipfrag(&request.body)?;
        if seq.method != Method::Notify
            || seq.seq == 0
            || seq.seq < state.notify_seq
            || terminal && status < 200
        {
            return Err(CallProbeError::Signaling);
        }
        if seq.seq == state.notify_seq {
            return if state.status == status && state.terminal == terminal {
                Ok(())
            } else {
                Err(CallProbeError::Signaling)
            };
        }
        if state.terminal {
            return Err(CallProbeError::Signaling);
        }
        state.notify_count = state.notify_count.saturating_add(1);
        if state.notify_count > 16 {
            state.failed = true;
            self.publish(&state);
            return Err(CallProbeError::Signaling);
        }
        state.notify_seq = seq.seq;
        state.status = status;
        state.terminal = terminal;
        if status < 200 {
            state.provisional = state.provisional.saturating_add(1);
        }
        self.publish(&state);
        Ok(())
    }
    fn accept(&self, response: &Response) -> Result<u32, CallProbeError> {
        let mut state = self.state.lock().map_err(|_| CallProbeError::Signaling)?;
        let seq = rsipstack::sip::typed::CSeq::parse(
            response
                .cseq_header()
                .map_err(|_| CallProbeError::Signaling)?
                .value(),
        )
        .map_err(|_| CallProbeError::Signaling)?;
        if !(200..=299).contains(&u16::from(response.status_code.clone()))
            || seq.method != Method::Refer
            || seq.seq != state.refer_seq
            || state.dialog.as_ref().map(|d| d.call_id.as_str())
                != response.call_id_header().ok().map(|h| h.value())
            || state.failed
        {
            return Err(CallProbeError::Signaling);
        }
        state.accepted = true;
        self.publish(&state);
        Ok(seq.seq)
    }
    async fn wait(&self) -> Result<Outcome, CallProbeError> {
        let mut change = self.changed.subscribe();
        loop {
            let value = *change.borrow_and_update();
            if value.failed {
                return Err(CallProbeError::Signaling);
            }
            if let Some(result) = value.result {
                return Ok(result);
            }
            change
                .changed()
                .await
                .map_err(|_| CallProbeError::Signaling)?;
        }
    }
}

/// Fixed disposable destinations only; never an arbitrary Refer-To URI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Destination {
    /// Asterisk route 602 emits an independently recognizable 1 kHz tone.
    ReferenceTone,
    /// Deliberately absent local route 699 must fail without losing the old call.
    Missing,
}
impl Destination {
    fn extension(self) -> &'static str {
        match self {
            Self::ReferenceTone => "602",
            Self::Missing => "699",
        }
    }
}
/// Safe aggregate evidence from one real synthetic blind-transfer attempt.
#[derive(Debug)]
pub struct TransferReport {
    /// SIP signaling transport; RTP is explicitly plaintext UDP.
    pub transport: &'static str,
    /// Negotiated G.711 codec.
    pub codec: &'static str,
    /// Second fictional endpoint originates and transfers when true.
    pub reverse: bool,
    /// Requested fixed scenario, not a user-controlled destination.
    pub destination: Destination,
    /// REFER response status; never used alone to infer success.
    pub refer_response: u16,
    /// REFER CSeq correlated with any Event id; no identifiers/headers emitted.
    pub refer_cseq: u32,
    /// Final message/sipfrag status from a terminated refer subscription.
    pub final_notify_status: u16,
    /// Valid distinct notifications consumed; the final 200 reply is gated separately.
    pub notify_count: u8,
    /// Observed provisional sipfrag notifications.
    pub provisional_notify_count: u8,
    /// Caller/callee before transfer, with distinct synthetic sources.
    pub before: [super::hold::Window; 2],
    /// Caller/callee after rejected transfer, only if the original call survived.
    pub restored: Option<[super::hold::Window; 2]>,
    /// Transferee's measured 1 kHz PBX route after successful transfer.
    pub reference_tone: Option<super::hold::Window>,
    /// Both original dialogs ended and both authenticated unregisters succeeded.
    pub signaling_completed: bool,
}
async fn settled(
    dialog: &rsipstack::dialog::invite_dialog::InviteDialog,
    endpoint: &mut super::LabEndpoint,
) -> Result<(), CallProbeError> {
    loop {
        let phase = *endpoint.phase.borrow_and_update();
        if phase.failed {
            return Err(CallProbeError::Signaling);
        }
        if dialog.state().is_confirmed() || dialog.state().is_terminated() {
            return Ok(());
        }
        endpoint
            .phase
            .changed()
            .await
            .map_err(|_| CallProbeError::Signaling)?;
    }
}
async fn voice_pair(
    a: &mut super::hold::Stream,
    b: &mut super::hold::Stream,
) -> Result<[super::hold::Window; 2], CallProbeError> {
    let (a, b) = tokio::join!(
        a.window(
            std::time::Duration::from_secs(2),
            true,
            true,
            std::time::Duration::ZERO
        ),
        b.window(
            std::time::Duration::from_secs(2),
            true,
            true,
            std::time::Duration::ZERO
        )
    );
    Ok([a?, b?])
}
fn verify_reference_tone(tone: super::hold::Window) -> Result<super::hold::Window, CallProbeError> {
    // Off-bin 880 Hz leaks into the 1 kHz meter; ratio alone can accept it.
    if !tone.remote_tone_amplitude.is_finite() || tone.remote_tone_amplitude < 4000.0 {
        return Err(CallProbeError::Media);
    }
    Ok(tone)
}
async fn run(
    first: crate::RegistrationConfig,
    second: crate::RegistrationConfig,
    codec: rust_microsip_media::Codec,
    reverse: bool,
    destination: Destination,
    stop: tokio_util::sync::CancellationToken,
) -> Result<TransferReport, CallProbeError> {
    use rsipstack::{dialog::invitation::InviteOption, sip::Uri, transport::SipAddr};
    let label = first.transport.as_str();
    let pbx = first.address.ip();
    let mut a =
        super::LabEndpoint::connect_mode(first, codec, stop.clone(), super::ProbeMode::Transfer)
            .await?;
    let mut b =
        super::LabEndpoint::connect_mode(second, codec, stop, super::ProbeMode::Transfer).await?;
    let (caller, callee) = if reverse {
        (&mut b, &mut a)
    } else {
        (&mut a, &mut b)
    };
    let local = caller.rtp.local_addr().map_err(|_| CallProbeError::Media)?;
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
        offer: Some(super::offer(local, codec)),
        ..Default::default()
    };
    let (dialog, response) = caller
        .layer
        .do_invite(option, caller.states.clone())
        .await
        .map_err(|_| CallProbeError::Signaling)?;
    let response = response.ok_or(CallProbeError::Signaling)?;
    if response.status_code != rsipstack::sip::StatusCode::OK {
        return Err(CallProbeError::Signaling);
    }
    let remote = super::parse_offer(&response.body, codec, pbx)?;
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
    let peer_id = incoming.dialog.id();
    let (own, other) = if reverse {
        (880.0, 440.0)
    } else {
        (440.0, 880.0)
    };
    let mut source = super::hold::Stream::new(caller, remote, own, other, 0x52454631);
    let mut peer = super::hold::Stream::new(callee, incoming.target, other, own, 0x52454632);
    let before = voice_pair(&mut source, &mut peer).await?;
    caller.transfer.arm(id.clone())?;
    // Arm before sending: the first NOTIFY may precede the REFER response.
    let refer_to = format!("<sip:{}@lab.invalid>", destination.extension());
    let response = dialog
        .refer(refer_to, None, None)
        .await
        .map_err(|_| CallProbeError::Signaling)?
        .ok_or(CallProbeError::Signaling)?;
    let refer_response = u16::from(response.status_code.clone());
    let refer_cseq = caller.transfer.accept(&response)?;
    let outcome = caller.transfer.wait().await?;
    let (restored, reference_tone) = match destination {
        Destination::Missing => {
            if outcome.final_status < 400 {
                return Err(CallProbeError::Signaling);
            }
            settled(&dialog, caller).await?;
            settled(&incoming.dialog, callee).await?;
            if dialog.id() != id
                || incoming.dialog.id() != peer_id
                || !dialog.state().is_confirmed()
                || !incoming.dialog.state().is_confirmed()
            {
                return Err(CallProbeError::Signaling);
            }
            let restored = voice_pair(&mut source, &mut peer).await?;
            dialog.bye().await.map_err(|_| CallProbeError::Signaling)?;
            caller.wire.wait(4).await;
            callee.wire.wait(2).await;
            (Some(restored), None)
        }
        Destination::ReferenceTone => {
            if !(200..=299).contains(&outcome.final_status) {
                return Err(CallProbeError::Signaling);
            }
            settled(&dialog, caller).await?;
            if !dialog.state().is_terminated() {
                match dialog.bye().await {
                    Ok(()) => caller.wire.wait(4).await,
                    Err(_) if dialog.state().is_terminated() => caller.wire.wait(2).await,
                    Err(_) => return Err(CallProbeError::Signaling),
                }
            } else {
                caller.wire.wait(2).await;
            }
            if incoming.dialog.id() != peer_id || incoming.dialog.state().is_terminated() {
                return Err(CallProbeError::Signaling);
            }
            peer.expect_remote_tone(1000.0);
            let tone = peer
                .window(
                    std::time::Duration::from_secs(2),
                    true,
                    true,
                    std::time::Duration::ZERO,
                )
                .await?;
            let tone = verify_reference_tone(tone)?;
            incoming
                .dialog
                .bye()
                .await
                .map_err(|_| CallProbeError::Signaling)?;
            callee.wire.wait(4).await;
            (None, Some(tone))
        }
    };
    caller.wait_phase(true).await?;
    callee.wait_phase(true).await?;
    if !dialog.state().is_terminated() || !incoming.dialog.state().is_terminated() {
        return Err(CallProbeError::Signaling);
    }
    caller.unregister().await?;
    callee.unregister().await?;
    Ok(TransferReport {
        transport: label,
        codec: codec.name(),
        reverse,
        destination,
        refer_response,
        refer_cseq,
        final_notify_status: outcome.final_status,
        notify_count: outcome.notify_count,
        provisional_notify_count: outcome.provisional_count,
        before,
        restored,
        reference_tone,
        signaling_completed: true,
    })
}
/// Verify one fixed fictional blind transfer through Asterisk in at most 30 seconds.
///
/// Only matched 1001/1002 UDP, 1101/1102 TCP or 1301/1302 TLS IPv4 fixtures
/// at one explicit registrar are admitted. TLS protects signaling only here;
/// mandatory-SRTP fixtures fail before networking. No arbitrary URI, personal
/// settings or audio devices are used. Caller-provided registrar IP is not a
/// sandbox. Success requires final correlated NOTIFY, its emitted 200 reply,
/// real media outcome, dialog termination and both authenticated unregisters.
/// Existing SDK credential/parser/channel and failure-cleanup gates remain.
///
/// # Errors
/// Returns sanitized [`CallProbeError`] on validation, signaling/media,
/// cancellation or total timeout. Failure drops local tasks/sockets but does
/// not guarantee remote cleanup; disposable PBX teardown is mandatory.
pub async fn probe_transfer_pair(
    first: crate::RegistrationConfig,
    second: crate::RegistrationConfig,
    codec: rust_microsip_media::Codec,
    reverse: bool,
    destination: Destination,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<TransferReport, CallProbeError> {
    if cancel.is_cancelled() {
        return Err(CallProbeError::Cancelled);
    }
    let users = match &first.transport {
        crate::Transport::Udp => ("1001", "1002"),
        crate::Transport::Tcp => ("1101", "1102"),
        crate::Transport::Tls { .. } => ("1301", "1302"),
    };
    if !first.address.is_ipv4()
        || first.address != second.address
        || first.transport.as_str() != second.transport.as_str()
        || first.username != users.0
        || second.username != users.1
    {
        return Err(CallProbeError::InvalidConfig);
    }
    tokio::select! {biased;_=cancel.cancelled()=>Err(CallProbeError::Cancelled),result=tokio::time::timeout(std::time::Duration::from_secs(30),run(first,second,codec,reverse,destination,cancel.child_token()))=>result.map_err(|_|CallProbeError::Timeout)?,}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_tone_amplitude_rejects_weak_and_non_finite_measurements() {
        for amplitude in [0.0, 3999.0, f64::NAN, f64::INFINITY] {
            let tone = super::super::hold::Window {
                sent_packets: 100,
                received_packets: 100,
                rms: 5000.0,
                remote_to_own_power_ratio: 50.0,
                remote_tone_amplitude: amplitude,
            };
            assert!(verify_reference_tone(tone).is_err());
        }
    }
    #[test]
    fn reference_tone_rejects_former_partner_leakage_even_when_the_old_ratio_passes()
    -> Result<(), CallProbeError> {
        for codec in [
            rust_microsip_media::Codec::Pcma,
            rust_microsip_media::Codec::Pcmu,
        ] {
            for hz in [880.0, 1000.0] {
                let mut meter = super::super::ToneMeter::new(440.0, 1000.0);
                for frame in 0..100 {
                    let mut payload = [0u8; 160];
                    for (index, octet) in payload.iter_mut().enumerate() {
                        let phase =
                            std::f64::consts::TAU * hz * (frame * 160 + index) as f64 / 8000.0;
                        *octet = codec.encode_sample((8000.0 * phase.sin()) as i16);
                    }
                    meter.observe(codec, &payload);
                }
                let (rms, ratio) = meter.finish()?;
                let tone = super::super::hold::Window {
                    sent_packets: 100,
                    received_packets: 100,
                    rms,
                    remote_to_own_power_ratio: ratio,
                    remote_tone_amplitude: 2.0 * (meter.wanted / 100.0).sqrt() / 160.0,
                };
                if hz == 880.0 {
                    assert!(
                        verify_reference_tone(tone).is_err(),
                        "former partner tone passed for {codec:?}"
                    );
                } else {
                    assert!(
                        verify_reference_tone(tone).is_ok(),
                        "reference tone failed for {codec:?}"
                    );
                }
            }
        }
        Ok(())
    }
    #[test]
    fn sipfrag_status_is_bounded_and_never_confuses_acceptance_with_success() {
        assert_eq!(sipfrag(b"SIP/2.0 100 Trying\r\n"), Ok(100));
        assert_eq!(sipfrag(b"SIP/2.0 200 OK\r\n"), Ok(200));
        assert_eq!(sipfrag(b"SIP/2.0 404 Not Found\r\n"), Ok(404));
        for bytes in [
            b"".as_slice(),
            b"SIP/2.0 20 OK",
            b"SIP/2.0 700 Invalid",
            b"SIP/2.0 200 OK\0",
        ] {
            assert_eq!(sipfrag(bytes), Err(super::super::CallProbeError::Signaling));
        }
        assert!(sipfrag(&[b'x'; 1025]).is_err());
    }
    #[test]
    fn event_id_matches_the_sent_refer_not_the_notify_cseq() {
        assert_eq!(refer_event("refer", 3), Ok(()));
        assert_eq!(refer_event("refer;id=3", 3), Ok(()));
        for text in [
            "message-summary",
            "refer;id=4",
            "refer;id=3;id=3",
            "refer;id=-3",
        ] {
            assert!(refer_event(text, 3).is_err());
        }
    }
    fn id() -> DialogId {
        DialogId {
            call_id: "fixture".into(),
            local_tag: "local".into(),
            remote_tag: "remote".into(),
        }
    }
    fn message(
        method: &str,
        cseq: u32,
        event: &str,
        subscription: &str,
        body: &str,
    ) -> Result<SipMessage, CallProbeError> {
        let (from, to) = if method == "REFER" {
            ("local", "remote")
        } else {
            ("remote", "local")
        };
        SipMessage::try_from(format!("{method} sip:1001@lab.invalid SIP/2.0\r\nVia: SIP/2.0/UDP 192.0.2.1:5060;branch=z9hG4bK-fixture\r\nFrom: <sip:1002@lab.invalid>;tag={from}\r\nTo: <sip:1001@lab.invalid>;tag={to}\r\nCall-ID: fixture\r\nCSeq: {cseq} {method}\r\nEvent: {event}\r\nSubscription-State: {subscription}\r\nContent-Type: message/sipfrag;version=2.0\r\nContent-Length: {}\r\n\r\n{body}",body.len()).as_str()).map_err(|_|CallProbeError::Signaling)
    }
    fn notify(
        cseq: u32,
        event: &str,
        subscription: &str,
        body: &str,
    ) -> Result<Request, CallProbeError> {
        match message("NOTIFY", cseq, event, subscription, body)? {
            SipMessage::Request(r) => Ok(r),
            _ => Err(CallProbeError::Signaling),
        }
    }
    fn accepted() -> Result<Response, CallProbeError> {
        match SipMessage::try_from("SIP/2.0 202 Accepted\r\nVia: SIP/2.0/UDP 192.0.2.1:5060;branch=z9hG4bK-fixture\r\nFrom: <sip:1001@lab.invalid>;tag=local\r\nTo: <sip:1002@lab.invalid>;tag=remote\r\nCall-ID: fixture\r\nCSeq: 3 REFER\r\nContent-Length: 0\r\n\r\n").map_err(|_|CallProbeError::Signaling)? { SipMessage::Response(r)=>Ok(r),_=>Err(CallProbeError::Signaling) }
    }
    fn notify_ack(seq: u32) -> Result<SipMessage, CallProbeError> {
        SipMessage::try_from(format!("SIP/2.0 200 OK\r\nVia: SIP/2.0/UDP 192.0.2.1:5060;branch=z9hG4bK-fixture\r\nFrom: <sip:1002@lab.invalid>;tag=remote\r\nTo: <sip:1001@lab.invalid>;tag=local\r\nCall-ID: fixture\r\nCSeq: {seq} NOTIFY\r\nContent-Length: 0\r\n\r\n").as_str()).map_err(|_|CallProbeError::Signaling)
    }
    fn armed() -> Result<Subscription, CallProbeError> {
        let subscription = Subscription::default();
        subscription.arm(id())?;
        subscription.sent(&message("REFER", 3, "refer", "active", "")?);
        Ok(subscription)
    }
    #[tokio::test]
    async fn acceptance_and_provisional_notify_never_complete_transfer()
    -> Result<(), Box<dyn std::error::Error>> {
        let sub = armed()?;
        sub.accept(&accepted()?)?;
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), sub.wait())
                .await
                .is_err()
        );
        sub.consume(
            &id(),
            &notify(
                10,
                "refer;id=3",
                "active;expires=600",
                "SIP/2.0 100 Trying\r\n",
            )?,
        )?;
        assert!(sub.changed.borrow().result.is_none());
        sub.consume(
            &id(),
            &notify(11, "refer;id=3", "active;expires=600", "SIP/2.0 200 OK\r\n")?,
        )?;
        assert!(sub.changed.borrow().result.is_none());
        sub.consume(
            &id(),
            &notify(
                12,
                "refer;id=3",
                "terminated;reason=noresource",
                "SIP/2.0 200 OK\r\n",
            )?,
        )?;
        assert!(sub.changed.borrow().result.is_none());
        sub.sent(&notify_ack(12)?);
        assert_eq!(
            sub.wait().await?,
            Outcome {
                final_status: 200,
                notify_count: 3,
                provisional_count: 1
            }
        );
        Ok(())
    }
    #[tokio::test]
    async fn final_notify_can_arrive_before_refer_response_and_retransmits_are_idempotent()
    -> Result<(), Box<dyn std::error::Error>> {
        let sub = armed()?;
        let final_notify = notify(
            10,
            "refer",
            "terminated;reason=noresource",
            "SIP/2.0 404 Not Found\r\n",
        )?;
        sub.consume(&id(), &final_notify)?;
        sub.consume(&id(), &final_notify)?;
        assert!(sub.changed.borrow().result.is_none());
        sub.accept(&accepted()?)?;
        assert!(sub.changed.borrow().result.is_none());
        sub.sent(&notify_ack(10)?);
        assert_eq!(
            sub.wait().await?,
            Outcome {
                final_status: 404,
                notify_count: 1,
                provisional_count: 0
            }
        );
        assert!(
            sub.consume(
                &id(),
                &notify(11, "refer", "active", "SIP/2.0 100 Trying\r\n")?
            )
            .is_err()
        );
        Ok(())
    }
    #[test]
    fn foreign_or_malformed_notify_cannot_resolve_the_bound_subscription()
    -> Result<(), CallProbeError> {
        let sub = armed()?;
        let good = notify(
            10,
            "refer",
            "terminated;reason=noresource",
            "SIP/2.0 200 OK\r\n",
        )?;
        let mut foreign = id();
        foreign.remote_tag = "other".into();
        assert!(sub.consume(&foreign, &good).is_err());
        for bad in [
            notify(10, "refer;id=4", "terminated", "SIP/2.0 200 OK\r\n")?,
            notify(10, "refer", "terminated", "SIP/2.0 100 Trying\r\n")?,
        ] {
            assert!(sub.consume(&id(), &bad).is_err());
        }
        let mut duplicate = good.clone();
        duplicate.headers.push(Header::Event("refer".into()));
        assert!(sub.consume(&id(), &duplicate).is_err());
        let mut wrong_tag = good.clone();
        wrong_tag.headers.retain(|h| !matches!(h, Header::To(_)));
        wrong_tag
            .headers
            .push(Header::To("<sip:1001@lab.invalid>;tag=wrong".into()));
        assert!(sub.consume(&id(), &wrong_tag).is_err());
        assert!(sub.changed.borrow().result.is_none());
        Ok(())
    }
    #[tokio::test]
    async fn finite_notify_cap_fails_closed_and_wakes_waiters()
    -> Result<(), Box<dyn std::error::Error>> {
        let sub = armed()?;
        for seq in 1..=16 {
            sub.consume(
                &id(),
                &notify(seq, "refer", "active", "SIP/2.0 100 Trying\r\n")?,
            )?;
        }
        assert!(
            sub.consume(
                &id(),
                &notify(17, "refer", "active", "SIP/2.0 100 Trying\r\n")?
            )
            .is_err()
        );
        assert_eq!(sub.wait().await, Err(CallProbeError::Signaling));
        Ok(())
    }

    #[tokio::test]
    async fn pre_cancel_and_mandatory_srtp_are_rejected_before_connection()
    -> Result<(), Box<dyn std::error::Error>> {
        let config = |user: &str, secure: bool| {
            crate::RegistrationConfig::new(
                std::net::SocketAddr::from(([127, 0, 0, 1], 5061)),
                if secure {
                    crate::Transport::Tls {
                        ca_pem: vec![1],
                        server_name: "pbx".into(),
                    }
                } else {
                    crate::Transport::Udp
                },
                user.into(),
                secrecy::SecretString::from("fictional"),
                std::time::Duration::from_secs(2),
            )
        };
        let cancel = tokio_util::sync::CancellationToken::new();
        cancel.cancel();
        assert!(matches!(
            probe_transfer_pair(
                config("1001", false)?,
                config("1002", false)?,
                rust_microsip_media::Codec::Pcma,
                false,
                Destination::ReferenceTone,
                cancel
            )
            .await,
            Err(CallProbeError::Cancelled)
        ));
        assert!(matches!(
            probe_transfer_pair(
                config("1201", true)?,
                config("1202", true)?,
                rust_microsip_media::Codec::Pcma,
                false,
                Destination::ReferenceTone,
                tokio_util::sync::CancellationToken::new()
            )
            .await,
            Err(CallProbeError::InvalidConfig)
        ));
        Ok(())
    }
}
