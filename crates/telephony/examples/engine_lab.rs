//! End-to-end check of the production engine against the disposable lab PBX.
//! Fictional fixtures only (`/lab` mount); prints numeric/boolean JSON lines.
use rust_microsip_core::{
    AccountSettings, CallPhase, CallSnapshot, Command, DialTarget, Direction, DtmfKey, EndReason,
    Event, FRAME_SAMPLES, MediaIo, MediaSecurity, RegistrationState, TransferState, Transport,
};
use rust_microsip_telephony::engine::{EngineHandle, TrustAnchors};
use secrecy::SecretString;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Tone source plus a two-frequency Goertzel meter over received audio.
struct Probe {
    hz: f64,
    phase: u64,
    meter: Arc<Mutex<Meter>>,
}
#[derive(Default)]
struct Meter {
    frames: u64,
    energy: f64,
    targets: Vec<(f64, f64)>,
}
impl Meter {
    fn reset(&mut self, targets: &[f64]) {
        *self = Self {
            frames: 0,
            energy: 0.0,
            targets: targets.iter().map(|hz| (*hz, 0.0)).collect(),
        };
    }
    fn ratio(&self) -> f64 {
        match self.targets.as_slice() {
            [(_, wanted), (_, other)] => wanted / other.max(1.0),
            _ => 0.0,
        }
    }
    fn rms(&self) -> f64 {
        if self.frames == 0 {
            0.0
        } else {
            (self.energy / (self.frames as f64 * FRAME_SAMPLES as f64)).sqrt()
        }
    }
}
fn goertzel(frame: &[i16; FRAME_SAMPLES], hz: f64) -> f64 {
    let coefficient = 2.0 * (std::f64::consts::TAU * hz / 8000.0).cos();
    let (mut s1, mut s2) = (0.0, 0.0);
    for sample in frame {
        let s0 = f64::from(*sample) + coefficient * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    s1 * s1 + s2 * s2 - coefficient * s1 * s2
}
impl MediaIo for Probe {
    fn capture(&mut self, frame: &mut [i16; FRAME_SAMPLES]) -> bool {
        for sample in frame.iter_mut() {
            *sample = (8000.0
                * (std::f64::consts::TAU * self.hz * (self.phase % 8000) as f64 / 8000.0).sin())
                as i16;
            self.phase += 1;
        }
        true
    }
    fn play(&mut self, frame: &[i16; FRAME_SAMPLES]) {
        if frame.iter().all(|s| *s == 0) {
            return;
        }
        if let Ok(mut meter) = self.meter.lock() {
            meter.frames += 1;
            meter.energy += frame.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>();
            for target in &mut meter.targets {
                target.1 += goertzel(frame, target.0);
            }
        }
    }
}

struct Side {
    engine: EngineHandle,
    meter: Arc<Mutex<Meter>>,
    events: Vec<Event>,
    registration: RegistrationState,
    call: Option<CallSnapshot>,
    ended: Vec<CallSnapshot>,
}
impl Side {
    fn new(hz: f64) -> Result<Self, String> {
        let meter = Arc::new(Mutex::new(Meter::default()));
        let engine = EngineHandle::spawn(
            Box::new(Probe {
                hz,
                phase: 0,
                meter: meter.clone(),
            }),
            Arc::new(|| {}),
        )
        .map_err(|_| "spawn")?;
        Ok(Self {
            engine,
            meter,
            events: Vec::new(),
            registration: RegistrationState::Disabled,
            call: None,
            ended: Vec::new(),
        })
    }
    fn pump(&mut self) {
        for event in self.engine.try_events() {
            match &event {
                Event::Registration(state) => self.registration = *state,
                Event::Call(snapshot) if snapshot.phase.is_final() => {
                    self.call = None;
                    self.ended.push(snapshot.clone());
                }
                Event::Call(snapshot) => self.call = Some(snapshot.clone()),
                _ => {}
            }
            self.events.push(event);
        }
    }
    fn send(&self, command: Command) -> Result<(), String> {
        self.engine.send(command).map_err(|_| "send".to_owned())
    }
    fn id(&self) -> Result<rust_microsip_core::CallId, String> {
        self.call
            .as_ref()
            .map(|c| c.id)
            .ok_or_else(|| "no call".to_owned())
    }
    fn measure(&self, targets: &[f64]) {
        if let Ok(mut meter) = self.meter.lock() {
            meter.reset(targets);
        }
    }
    fn result(&self) -> (u64, f64, f64) {
        self.meter
            .lock()
            .map(|m| (m.frames, m.rms(), m.ratio()))
            .unwrap_or_default()
    }
}

fn wait(
    sides: &mut [&mut Side],
    seconds: u64,
    done: impl Fn(&[&mut Side]) -> bool,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        for side in sides.iter_mut() {
            side.pump();
        }
        if done(sides) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err("timeout".into())
}
fn settle(sides: &mut [&mut Side], seconds: f64) {
    let deadline = Instant::now() + Duration::from_secs_f64(seconds);
    while Instant::now() < deadline {
        for side in sides.iter_mut() {
            side.pump();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn phase(side: &Side) -> Option<CallPhase> {
    side.call.as_ref().map(|c| c.phase)
}
fn voice(side: &Side, label: &str) -> Result<serde_json::Value, String> {
    let (frames, rms, ratio) = side.result();
    if frames < 70 || !(1000.0..10000.0).contains(&rms) || ratio < 20.0 {
        return Err(format!(
            "{label}: frames {frames} rms {rms:.0} ratio {ratio:.1}"
        ));
    }
    Ok(serde_json::json!({"frames":frames,"rms":rms,"ratio":ratio}))
}

fn account(user: &str, transport: Transport, media: MediaSecurity) -> AccountSettings {
    AccountSettings {
        display_name: format!("Lab {user}"),
        username: user.into(),
        auth_username: None,
        domain: "pbx".into(),
        server: "pbx".into(),
        port: transport.default_port(),
        transport,
        media,
        expires: 120,
    }
}

fn scenario(
    label: &str,
    users: (&str, &str),
    third: Option<&str>,
    transport: Transport,
    media: MediaSecurity,
    passwords: &BTreeMap<String, String>,
    ca: &[u8],
) -> Result<serde_json::Value, String> {
    let (mut a, mut b) = (Side::new(440.0)?, Side::new(880.0)?);
    for (side, user) in [(&a, users.0), (&b, users.1)] {
        side.engine.set_trust(TrustAnchors::Custom(ca.to_vec()));
        let password = passwords.get(user).ok_or("password")?;
        side.send(Command::Register(
            Box::new(account(user, transport, media)),
            SecretString::from(password.clone()),
        ))?;
    }
    wait(&mut [&mut a, &mut b], 15, |s| {
        s.iter()
            .all(|s| s.registration == RegistrationState::Registered)
    })?;
    let target = |user: &str| DialTarget::parse(user).map_err(|_| "target".to_owned());

    // 1. Call, ringing, answer, two-way audio, DTMF-free hangup by callee.
    a.send(Command::Dial(target(users.1)?))?;
    wait(&mut [&mut a, &mut b], 10, |s| {
        phase(s[1]) == Some(CallPhase::Ringing)
            && s[1]
                .call
                .as_ref()
                .is_some_and(|c| c.direction == Direction::Incoming)
    })?;
    let ringing_seen = a
        .events
        .iter()
        .any(|e| matches!(e, Event::Call(c) if c.phase == CallPhase::Ringing));
    b.send(Command::Answer(b.id()?))?;
    wait(&mut [&mut a, &mut b], 10, |s| {
        s.iter().all(|s| phase(s) == Some(CallPhase::Established))
    })?;
    let secure =
        a.call.as_ref().is_some_and(|c| c.secure) && b.call.as_ref().is_some_and(|c| c.secure);
    settle(&mut [&mut a, &mut b], 0.5);
    a.measure(&[880.0, 440.0]);
    b.measure(&[440.0, 880.0]);
    settle(&mut [&mut a, &mut b], 2.0);
    let before = [voice(&a, "a before")?, voice(&b, "b before")?];

    // 2. Hold: the holder hears nothing; resume restores both ways.
    a.send(Command::Hold(a.id()?))?;
    wait(&mut [&mut a, &mut b], 10, |s| {
        phase(s[0]) == Some(CallPhase::Held)
    })?;
    settle(&mut [&mut a, &mut b], 0.3);
    a.measure(&[880.0, 440.0]);
    settle(&mut [&mut a, &mut b], 1.0);
    let held_frames = a.result().0;
    a.send(Command::Resume(a.id()?))?;
    wait(&mut [&mut a, &mut b], 10, |s| {
        phase(s[0]) == Some(CallPhase::Established)
    })?;
    settle(&mut [&mut a, &mut b], 0.5);
    a.measure(&[880.0, 440.0]);
    b.measure(&[440.0, 880.0]);
    settle(&mut [&mut a, &mut b], 2.0);
    let resumed = [voice(&a, "a resumed")?, voice(&b, "b resumed")?];
    if held_frames != 0 {
        return Err(format!("held frames {held_frames}"));
    }

    // 3. Mute: the peer stops hearing the caller.
    a.send(Command::SetMute(a.id()?, true))?;
    settle(&mut [&mut a, &mut b], 0.3);
    b.measure(&[440.0, 880.0]);
    settle(&mut [&mut a, &mut b], 1.0);
    // Muted audio keeps flowing as silence; the peer must hear no energy.
    let muted_rms = b.result().1;
    a.send(Command::SetMute(a.id()?, false))?;
    if muted_rms > 50.0 {
        return Err(format!("muted rms {muted_rms:.1}"));
    }

    // 4. Failed blind transfer keeps the call; then hang up from the callee.
    a.send(Command::Transfer(a.id()?, target("699")?))?;
    wait(&mut [&mut a, &mut b], 15, |s| {
        s[0].call
            .as_ref()
            .is_some_and(|c| matches!(c.transfer, Some(TransferState::Failed(_))))
    })?;
    let failed_status = match a.call.as_ref().and_then(|c| c.transfer) {
        Some(TransferState::Failed(code)) => code,
        _ => 0,
    };
    settle(&mut [&mut a, &mut b], 0.5);
    b.measure(&[440.0, 880.0]);
    settle(&mut [&mut a, &mut b], 1.5);
    let after_failed_transfer = voice(&b, "b after failed transfer")?;
    b.send(Command::Hangup(b.id()?))?;
    wait(&mut [&mut a, &mut b], 10, |s| {
        s.iter().all(|s| s.call.is_none())
    })?;
    let first_end = a.ended.last().map(|c| c.phase);

    // 5. Successful blind transfer of B to the 1 kHz route 602.
    a.send(Command::Dial(target(users.1)?))?;
    wait(&mut [&mut a, &mut b], 10, |s| {
        phase(s[1]) == Some(CallPhase::Ringing)
    })?;
    b.send(Command::Answer(b.id()?))?;
    wait(&mut [&mut a, &mut b], 10, |s| {
        s.iter().all(|s| phase(s) == Some(CallPhase::Established))
    })?;
    settle(&mut [&mut a, &mut b], 0.5);
    a.send(Command::Transfer(a.id()?, target("602")?))?;
    wait(&mut [&mut a, &mut b], 15, |s| s[0].call.is_none())?;
    let transferred = a
        .ended
        .last()
        .is_some_and(|c| c.phase == CallPhase::Ended(EndReason::Transferred));
    settle(&mut [&mut a, &mut b], 0.8);
    b.measure(&[1000.0, 440.0]);
    settle(&mut [&mut a, &mut b], 2.0);
    let destination = voice(&b, "b destination tone")?;
    b.send(Command::Hangup(b.id()?))?;
    wait(&mut [&mut a, &mut b], 10, |s| s[1].call.is_none())?;

    // 6. DTMF 123 to IVR 601 must select the 1 kHz branch.
    a.send(Command::Dial(target("601")?))?;
    wait(&mut [&mut a], 10, |s| {
        phase(s[0]) == Some(CallPhase::Established)
    })?;
    settle(&mut [&mut a], 0.8);
    for key in ['1', '2', '3'] {
        a.send(Command::Dtmf(
            a.id()?,
            DtmfKey::new(key).map_err(|_| "key")?,
        ))?;
        settle(&mut [&mut a], 0.35);
    }
    settle(&mut [&mut a], 0.4);
    a.measure(&[1000.0, 400.0]);
    settle(&mut [&mut a], 1.8);
    let ivr = voice(&a, "ivr branch")?;
    a.send(Command::Hangup(a.id()?))?;
    wait(&mut [&mut a], 10, |s| s[0].call.is_none())?;

    // 6b. Busy: a second incoming call (TCP account) is refused with 486 while
    // the active call keeps two-way audio.
    let mut busy_case = serde_json::Value::Null;
    if let Some(third_user) = third {
        let mut c = Side::new(660.0)?;
        let password = passwords.get(third_user).ok_or("password")?;
        c.send(Command::Register(
            Box::new(account(third_user, Transport::Tcp, MediaSecurity::PlainRtp)),
            SecretString::from(password.clone()),
        ))?;
        wait(&mut [&mut c], 15, |s| {
            s[0].registration == RegistrationState::Registered
        })?;
        a.send(Command::Dial(target(users.1)?))?;
        wait(&mut [&mut a, &mut b], 10, |s| {
            phase(s[1]) == Some(CallPhase::Ringing)
        })?;
        b.send(Command::Answer(b.id()?))?;
        wait(&mut [&mut a, &mut b], 10, |s| {
            s.iter().all(|s| phase(s) == Some(CallPhase::Established))
        })?;
        let b_calls_before = b.events.len();
        let c_ended_before = c.ended.len();
        c.send(Command::Dial(target(users.1)?))?;
        wait(&mut [&mut a, &mut b, &mut c], 15, |s| {
            s[2].ended.len() > c_ended_before
        })?;
        let refused = c.ended.last().map(|x| x.phase);
        // DND switched on during the active call must not interrupt it.
        b.send(Command::SetDnd(true))?;
        let b_saw_second = b.events[b_calls_before..].iter().any(
            |e| matches!(e, Event::Call(snap) if Some(snap.id) != b.call.as_ref().map(|x| x.id)),
        );
        settle(&mut [&mut a, &mut b, &mut c], 0.3);
        a.measure(&[880.0, 440.0]);
        b.measure(&[440.0, 880.0]);
        settle(&mut [&mut a, &mut b, &mut c], 1.5);
        let kept = [voice(&a, "a during busy")?, voice(&b, "b during busy")?];
        b.send(Command::SetDnd(false))?;
        b.send(Command::Hangup(b.id()?))?;
        wait(&mut [&mut a, &mut b], 10, |s| {
            s.iter().all(|s| s.call.is_none())
        })?;
        let _ = c.engine.shutdown(Duration::from_secs(5));
        if refused != Some(CallPhase::Ended(EndReason::Busy)) || b_saw_second {
            return Err(format!(
                "busy: refused {refused:?} second_seen {b_saw_second}"
            ));
        }
        busy_case = serde_json::json!({"third_party_transport":"tcp","refused":"busy","active_call_kept":kept});
    }

    // 6d. Reverse direction: B calls A, A answers, two-way audio, A hangs up.
    b.send(Command::Dial(target(users.0)?))?;
    wait(&mut [&mut a, &mut b], 10, |s| {
        phase(s[0]) == Some(CallPhase::Ringing)
    })?;
    a.send(Command::Answer(a.id()?))?;
    wait(&mut [&mut a, &mut b], 10, |s| {
        s.iter().all(|s| phase(s) == Some(CallPhase::Established))
    })?;
    settle(&mut [&mut a, &mut b], 0.5);
    a.measure(&[880.0, 440.0]);
    b.measure(&[440.0, 880.0]);
    settle(&mut [&mut a, &mut b], 1.5);
    let reverse = [voice(&a, "a reverse")?, voice(&b, "b reverse")?];
    a.send(Command::Hangup(a.id()?))?;
    wait(&mut [&mut a, &mut b], 10, |s| {
        s.iter().all(|s| s.call.is_none())
    })?;
    if b.ended.last().map(|x| x.phase) != Some(CallPhase::Ended(EndReason::Normal)) {
        return Err("reverse: callee hangup not seen as normal".into());
    }

    // 6c. CANCEL/200 race: the caller hangs up while the callee answers.
    // Whatever wins, neither side may keep a call.
    let mut race_outcomes = Vec::new();
    for _ in 0..3 {
        a.send(Command::Dial(target(users.1)?))?;
        wait(&mut [&mut a, &mut b], 10, |s| {
            phase(s[1]) == Some(CallPhase::Ringing)
        })?;
        let (caller, callee) = (a.id()?, b.id()?);
        b.send(Command::Answer(callee))?;
        a.send(Command::Hangup(caller))?;
        wait(&mut [&mut a, &mut b], 40, |s| {
            s.iter().all(|s| s.call.is_none())
        })?;
        settle(&mut [&mut a, &mut b], 1.0);
        if a.call.is_some() || b.call.is_some() {
            return Err("race left a call".into());
        }
        race_outcomes.push(serde_json::json!({
            "caller": format!("{:?}", a.ended.last().map(|x| x.phase)),
            "callee": format!("{:?}", b.ended.last().map(|x| x.phase)),
        }));
    }

    // 7. Reject, caller cancel and DND busy.
    a.send(Command::Dial(target(users.1)?))?;
    wait(&mut [&mut a, &mut b], 10, |s| {
        phase(s[1]) == Some(CallPhase::Ringing)
    })?;
    b.send(Command::Reject(b.id()?))?;
    wait(&mut [&mut a, &mut b], 10, |s| {
        s.iter().all(|s| s.call.is_none())
    })?;
    let rejected = a.ended.last().map(|c| c.phase);
    a.send(Command::Dial(target(users.1)?))?;
    wait(&mut [&mut a, &mut b], 10, |s| {
        phase(s[1]) == Some(CallPhase::Ringing)
    })?;
    a.send(Command::Hangup(a.id()?))?;
    wait(&mut [&mut a, &mut b], 10, |s| {
        s.iter().all(|s| s.call.is_none())
    })?;
    let canceled = (
        a.ended.last().map(|c| c.phase),
        b.ended.last().map(|c| c.phase),
    );
    b.send(Command::SetDnd(true))?;
    settle(&mut [&mut a, &mut b], 0.2);
    let incoming_before = b.ended.len();
    let outgoing_before = a.ended.len();
    a.send(Command::Dial(target(users.1)?))?;
    wait(&mut [&mut a], 10, |s| s[0].ended.len() > outgoing_before)?;
    let dnd = (
        a.ended.last().map(|c| c.phase),
        b.ended.len() == incoming_before,
    );

    let stopped = [
        a.engine.shutdown(Duration::from_secs(6)),
        b.engine.shutdown(Duration::from_secs(6)),
    ];
    if !(ringing_seen
        && secure == (media == MediaSecurity::Srtp)
        && first_end == Some(CallPhase::Ended(EndReason::Normal))
        && failed_status >= 400
        && transferred
        && rejected == Some(CallPhase::Ended(EndReason::Busy))
        && canceled.0 == Some(CallPhase::Ended(EndReason::Canceled))
        && canceled.1 == Some(CallPhase::Ended(EndReason::Canceled))
        && dnd.0 == Some(CallPhase::Ended(EndReason::Busy))
        && dnd.1
        && stopped == [true, true])
    {
        return Err(format!(
            "{label}: ringing {ringing_seen} secure {secure} first {first_end:?} failed {failed_status} transferred {transferred} rejected {rejected:?} canceled {canceled:?} dnd {dnd:?} stopped {stopped:?}"
        ));
    }
    Ok(
        serde_json::json!({"scenario":label,"secure":secure,"before":before,"held_frames":held_frames,"resumed":resumed,
        "muted_rms_at_peer":muted_rms,"failed_transfer_status":failed_status,"after_failed_transfer":after_failed_transfer,
        "transferred":transferred,"destination_tone":destination,"busy":busy_case,"reverse_direction":reverse,"cancel_answer_race":race_outcomes,"ivr_123_branch":ivr,"reject":"busy","cancel":"canceled","dnd":"busy","stopped":true}),
    )
}

/// TLS must fail closed with a typed certificate failure, never register.
fn tls_negatives(
    passwords: &BTreeMap<String, String>,
    ca: &[u8],
    untrusted: &[u8],
) -> Result<serde_json::Value, String> {
    let password = passwords.get("1201").ok_or("password")?;
    let address = std::net::ToSocketAddrs::to_socket_addrs(&("pbx", 5061))
        .map_err(|_| "resolve")?
        .find(std::net::SocketAddr::is_ipv4)
        .ok_or("resolve")?;
    let mut results = Vec::new();
    for (label, trust, server) in [
        ("untrusted_ca", untrusted.to_vec(), "pbx".to_owned()),
        ("wrong_hostname", ca.to_vec(), address.ip().to_string()),
        (
            "garbage_ca",
            b"-----BEGIN CERTIFICATE-----\nnot base64\n-----END CERTIFICATE-----\n".to_vec(),
            "pbx".to_owned(),
        ),
    ] {
        let mut side = Side::new(440.0)?;
        side.engine.set_trust(TrustAnchors::Custom(trust));
        let mut settings = account("1201", Transport::Tls, MediaSecurity::Srtp);
        settings.server = server;
        side.send(Command::Register(
            Box::new(settings),
            SecretString::from(password.clone()),
        ))?;
        wait(&mut [&mut side], 15, |s| {
            matches!(s[0].registration, RegistrationState::Failed(_))
        })?;
        let state = side.registration;
        settle(&mut [&mut side], 1.0);
        let never_registered = !side
            .events
            .contains(&Event::Registration(RegistrationState::Registered));
        let typed = state
            == RegistrationState::Failed(rust_microsip_core::RegistrationFailure::Certificate);
        let _ = side.engine.shutdown(Duration::from_secs(5));
        if !(typed && never_registered) {
            return Err(format!(
                "{label}: {state:?} never_registered {never_registered}"
            ));
        }
        results.push(
            serde_json::json!({"case":label,"failed_certificate":true,"never_registered":true}),
        );
    }
    Ok(serde_json::json!({"scenario":"tls-negatives","cases":results}))
}

/// AUD-05: no downgrade in either direction; the call never establishes.
fn downgrades(
    passwords: &BTreeMap<String, String>,
    ca: &[u8],
) -> Result<serde_json::Value, String> {
    let mut cases = Vec::new();
    for (label, user, media, target) in [
        // An SDES-mandatory account offering plaintext RTP.
        (
            "plaintext_offer_to_sdes_endpoint",
            "1201",
            MediaSecurity::PlainRtp,
            "1202",
        ),
        // An SRTP-mandatory client reaching an endpoint without SRTP.
        (
            "srtp_offer_to_plain_endpoint",
            "1301",
            MediaSecurity::Srtp,
            "1302",
        ),
    ] {
        let mut side = Side::new(440.0)?;
        side.engine.set_trust(TrustAnchors::Custom(ca.to_vec()));
        let password = passwords.get(user).ok_or("password")?;
        side.send(Command::Register(
            Box::new(account(user, Transport::Tls, media)),
            SecretString::from(password.clone()),
        ))?;
        wait(&mut [&mut side], 15, |s| {
            s[0].registration == RegistrationState::Registered
        })?;
        side.send(Command::Dial(
            DialTarget::parse(target).map_err(|_| "target")?,
        ))?;
        wait(&mut [&mut side], 15, |s| !s[0].ended.is_empty())?;
        let end = side.ended.last().map(|c| c.phase);
        let established = side
            .events
            .iter()
            .any(|e| matches!(e, Event::Call(c) if c.phase == CallPhase::Established));
        let _ = side.engine.shutdown(Duration::from_secs(5));
        if established || end != Some(CallPhase::Ended(EndReason::NotAcceptable)) {
            return Err(format!("{label}: end {end:?} established {established}"));
        }
        cases.push(serde_json::json!({"case":label,"ended":"not_acceptable","established":false}));
    }
    Ok(serde_json::json!({"scenario":"downgrade-rejection","cases":cases}))
}

/// A wrong password is a typed `Unauthorized` failure and is never retried.
fn wrong_password() -> Result<serde_json::Value, String> {
    let mut side = Side::new(440.0)?;
    side.send(Command::Register(
        Box::new(account("1001", Transport::Udp, MediaSecurity::PlainRtp)),
        SecretString::from("fictional-wrong-password".to_owned()),
    ))?;
    wait(&mut [&mut side], 20, |s| {
        matches!(s[0].registration, RegistrationState::Failed(_))
    })?;
    let state = side.registration;
    settle(&mut [&mut side], 15.0);
    let attempts = side
        .events
        .iter()
        .filter(|e| **e == Event::Registration(RegistrationState::Registering))
        .count();
    let _ = side.engine.shutdown(Duration::from_secs(5));
    let unauthorized =
        state == RegistrationState::Failed(rust_microsip_core::RegistrationFailure::Unauthorized);
    if !unauthorized || attempts != 1 {
        return Err(format!("wrong password: {state:?} attempts {attempts}"));
    }
    Ok(
        serde_json::json!({"scenario":"wrong-password","failed":"unauthorized","register_attempts_in_15s":attempts}),
    )
}

fn main() {
    let read = |path: &str| std::fs::read(path).map_err(|_| "fixture".to_owned());
    let result = (|| -> Result<(), String> {
        let passwords: BTreeMap<String, String> =
            serde_json::from_slice(&read("/lab/credentials.json")?).map_err(|_| "credentials")?;
        let ca = read("/lab/ca.crt")?;
        println!("{}", wrong_password()?);
        println!("{}", downgrades(&passwords, &read("/lab/ca.crt")?)?);
        println!(
            "{}",
            tls_negatives(&passwords, &ca, &read("/lab/untrusted-ca.crt")?)?
        );
        for (label, users, third, transport, media) in [
            (
                "udp-rtp",
                ("1001", "1002"),
                Some("1101"),
                Transport::Udp,
                MediaSecurity::PlainRtp,
            ),
            (
                "tls-srtp",
                ("1201", "1202"),
                None,
                Transport::Tls,
                MediaSecurity::Srtp,
            ),
        ] {
            let row = scenario(label, users, third, transport, media, &passwords, &ca)?;
            println!("{row}");
        }
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("engine lab failed: {error}");
        std::process::exit(1);
    }
}
