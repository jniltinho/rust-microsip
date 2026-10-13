//! Registration renewal and network-loss recovery of the production engine
//! against the disposable lab PBX. The host script cuts this container's
//! network when it prints the `in_call` phase. Fictional fixtures only.
use rust_microsip_core::{
    AccountSettings, CallPhase, Command, DialTarget, EndReason, Event, MediaSecurity, NullMedia,
    RegistrationState, Transport,
};
use rust_microsip_telephony::engine::EngineHandle;
use secrecy::SecretString;
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

struct Watch {
    engine: EngineHandle,
    events: Vec<(Duration, Event)>,
    start: Instant,
}
impl Watch {
    fn pump(&mut self) {
        for event in self.engine.try_events() {
            self.events.push((self.start.elapsed(), event));
        }
    }
    fn registration(&self) -> Option<RegistrationState> {
        self.events.iter().rev().find_map(|(_, e)| match e {
            Event::Registration(state) => Some(*state),
            _ => None,
        })
    }
    fn wait(&mut self, seconds: u64, done: impl Fn(&Self) -> bool) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < deadline {
            self.pump();
            if done(self) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        // Numeric timeline only: no SIP, SDP or account data.
        let timeline: Vec<String> = self
            .events
            .iter()
            .map(|(at, e)| match e {
                Event::Registration(state) => format!("{}s reg {state:?}", at.as_secs()),
                Event::Call(c) => format!("{}s call {} {:?}", at.as_secs(), c.id.0, c.phase),
                other => format!("{}s {other:?}", at.as_secs()),
            })
            .collect();
        Err(format!("timeout; timeline: {timeline:?}"))
    }
    fn phase(label: &str) {
        println!("{}", serde_json::json!({"phase": label}));
    }
}

fn run() -> Result<serde_json::Value, String> {
    let passwords: BTreeMap<String, String> =
        serde_json::from_slice(&std::fs::read("/lab/credentials.json").map_err(|_| "fixture")?)
            .map_err(|_| "credentials")?;
    let mut watch = Watch {
        engine: EngineHandle::spawn(Box::new(NullMedia), Arc::new(|| {})).map_err(|_| "spawn")?,
        events: Vec::new(),
        start: Instant::now(),
    };
    let settings = AccountSettings {
        display_name: String::new(),
        username: "1001".into(),
        auth_username: None,
        domain: "pbx".into(),
        server: "pbx".into(),
        port: 5060,
        transport: Transport::Udp,
        media: MediaSecurity::PlainRtp,
        expires: 60,
    };
    let password = passwords.get("1001").ok_or("password")?;
    watch
        .engine
        .send(Command::Register(
            Box::new(settings),
            SecretString::from(password.clone()),
        ))
        .map_err(|_| "send")?;
    watch.wait(15, |w| {
        w.registration() == Some(RegistrationState::Registered)
    })?;
    Watch::phase("registered");
    // Renewal: the binding (60 s) must be refreshed before it expires; the
    // host checks the PBX contact after this window.
    std::thread::sleep(Duration::from_secs(70));
    watch.pump();
    let stayed = watch.registration() == Some(RegistrationState::Registered);
    Watch::phase("renewal_window");
    // A call to the echo route, then the host cuts the network.
    watch
        .engine
        .send(Command::Dial(
            DialTarget::parse("600").map_err(|_| "target")?,
        ))
        .map_err(|_| "send")?;
    watch.wait(15, |w| {
        w.events
            .iter()
            .any(|(_, e)| matches!(e, Event::Call(c) if c.phase == CallPhase::Established))
    })?;
    Watch::phase("in_call");
    let cut = watch.start.elapsed();
    // The call must end on its own (RTP inactivity) without any new call.
    watch.wait(90, |w| {
        w.events
            .iter()
            .any(|(_, e)| matches!(e, Event::Call(c) if c.phase.is_final()))
    })?;
    let call_end = watch.events.iter().find_map(|(at, e)| match e {
        Event::Call(c) if c.phase.is_final() => Some((*at, c.phase, c.id)),
        _ => None,
    });
    // Registration must be active again once the network is back.
    std::thread::sleep(Duration::from_secs(45));
    watch.wait(240, |w| {
        w.registration() == Some(RegistrationState::Registered)
    })?;
    Watch::phase("recovered");
    // Let the host inspect the PBX before the bounded shutdown unregisters.
    std::thread::sleep(Duration::from_secs(15));
    watch.pump();
    let still_registered = watch.registration() == Some(RegistrationState::Registered);
    let recovered_at = watch.start.elapsed();
    let calls: std::collections::BTreeSet<_> = watch
        .events
        .iter()
        .filter_map(|(_, e)| match e {
            Event::Call(c) => Some(c.id),
            _ => None,
        })
        .collect();
    let failures = watch
        .events
        .iter()
        .filter(|(at, e)| {
            *at > cut && matches!(e, Event::Registration(RegistrationState::Failed(_)))
        })
        .count();
    let stopped = watch.engine.shutdown(Duration::from_secs(6));
    let Some((ended_at, phase, _)) = call_end else {
        return Err("call never ended".into());
    };
    if !stayed
        || !still_registered
        || phase != CallPhase::Ended(EndReason::Failed)
        || calls.len() != 1
        || !stopped
    {
        return Err(format!(
            "stayed {stayed} phase {phase:?} calls {} stopped {stopped}",
            calls.len()
        ));
    }
    Ok(serde_json::json!({
        "scenario": "renewal-and-network-loss",
        "registered_through_renewal_window": stayed,
        "call_end_after_cut_s": (ended_at - cut).as_secs(),
        "call_end": "failed",
        "calls_created": calls.len(),
        "registration_failures_while_offline": failures,
        "re_registered_after_cut_s": (recovered_at - cut).as_secs(),
    }))
}

fn main() {
    match run() {
        Ok(row) => println!("{row}"),
        Err(error) => {
            eprintln!("engine network test failed: {error}");
            std::process::exit(1);
        }
    }
}
