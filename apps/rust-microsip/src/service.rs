//! Headless service (AG-01, AG-04): `rust-microsip service --grant-file FILE`.
//!
//! Owns the profile like the window — single-instance lock, SQLite, SIP and
//! audio — without a display, and serves only what the single-use bootstrap
//! grant exported from the window allows. It never prompts: anything else is
//! `grant_required`. A window or link opened meanwhile is turned away. It
//! needs an explicit password source (environment or file; no keyring
//! prompt), reports microphone activity on stderr and stops cleanly on
//! SIGTERM/SIGINT.

use crate::{fail, open_storage};
use rust_microsip::cli::read_bootstrap;
use rust_microsip::owner::{ControlOwner, capability_hash};
use rust_microsip::ui::{Backend, UiSettings, UiState};
use rust_microsip_audio::OnDemandMedia;
use rust_microsip_config::{self as config, CliCommand, ResolveContext};
use rust_microsip_control::OwnerMode;
use rust_microsip_core::{Command, Event, MediaIo, Notice};
use rust_microsip_platform::{ControlServer, acquire_owner, default_runtime_dir};
use rust_microsip_telephony::engine::{EngineHandle, TrustAnchors};
use std::io::Write;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

struct EngineOnly<'a>(&'a EngineHandle);
impl Backend for EngineOnly<'_> {
    fn send(&self, command: Command) -> Result<(), Notice> {
        self.0.send(command)
    }
    fn poll(&self) -> Vec<Event> {
        Vec::new()
    }
}

fn log(message: &str) {
    let _ = writeln!(std::io::stderr().lock(), "rust-microsip: {message}");
}

/// Set `flag` on the first SIGTERM/SIGINT.
fn watch_termination(flag: Arc<AtomicBool>) {
    let _ = std::thread::Builder::new()
        .name("rust-microsip-service-signals".into())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .build()
            else {
                return;
            };
            runtime.block_on(async {
                use tokio::signal::unix::{SignalKind, signal};
                if let (Ok(mut term), Ok(mut int)) = (
                    signal(SignalKind::terminate()),
                    signal(SignalKind::interrupt()),
                ) {
                    tokio::select! {
                        _ = term.recv() => {}
                        _ = int.recv() => {}
                    }
                }
                flag.store(true, Ordering::SeqCst);
            });
        });
}

/// Run the service; `raw` starts with `service`.
pub fn run(raw: &[String]) -> ExitCode {
    let mut rest = Vec::new();
    let mut grant_file = None;
    let mut iter = raw.iter().skip(1);
    while let Some(arg) = iter.next() {
        if arg == "--grant-file" {
            grant_file = iter.next().cloned();
        } else {
            rest.push(arg.clone());
        }
    }
    let Some(grant_file) = grant_file else {
        return fail(
            "rust-microsip: usage: rust-microsip service --grant-file FILE [--config PATH] [--env-file PATH]",
            2,
        );
    };
    let args = match config::parse_args(rest.iter().map(Into::into)) {
        Ok(args) if args.command == CliCommand::Run && args.uri.is_none() => args,
        _ => return fail("rust-microsip: invalid service arguments; see --help", 2),
    };
    let (bootstrap_id, capability) = match read_bootstrap(std::path::Path::new(&grant_file)) {
        Ok(pair) => pair,
        Err(message) => return fail(&format!("rust-microsip: {message}"), 2),
    };
    let Ok(dir) = default_runtime_dir() else {
        return fail(
            "rust-microsip: runtime_unavailable: XDG_RUNTIME_DIR is not set",
            4,
        );
    };
    let listener = match acquire_owner(&dir) {
        Ok(Some(listener)) => listener,
        Ok(None) => {
            return fail(
                "rust-microsip: another rust-microsip instance owns the profile; quit it first",
                4,
            );
        }
        Err(error) => return fail(&format!("rust-microsip: runtime_unavailable: {error}"), 4),
    };
    listener.set_service_mode();
    let mut ctx = match ResolveContext::from_process(&args) {
        Ok(ctx) => ctx,
        Err(error) => return fail(&format!("rust-microsip: {error}"), 2),
    };
    let (storage, saved) = open_storage(&ctx.env, true);
    let (Some(mut store), Some(saved)) = (storage.store, saved) else {
        return fail(
            "rust-microsip: storage_unavailable: the settings database is needed",
            4,
        );
    };
    ctx.sqlite = config::sqlite_layer(&saved);
    let effective = match config::resolve(&ctx) {
        Ok(effective) => effective,
        Err(error) => return fail(&format!("rust-microsip: configuration: {error}"), 2),
    };
    let Some(password) = effective.password else {
        return fail(
            "rust-microsip: secrets_unavailable: set password_env or password_file (the service never prompts)",
            4,
        );
    };
    let stored = effective.config;
    let epoch = match store.control_begin_epoch() {
        Ok(epoch) => epoch,
        Err(_) => {
            return fail(
                "rust-microsip: storage_unavailable: the ledger is unavailable",
                4,
            );
        }
    };
    // The bootstrap is consumed once, in this session; the file is left as is.
    match store.bootstrap_consume(&bootstrap_id, &capability_hash(&capability), epoch) {
        Ok(Some(_)) => {}
        Ok(None) if store.bootstrap_consumed(&bootstrap_id).unwrap_or(false) => {
            return fail(
                "rust-microsip: bootstrap_consumed: export a new grant from the window for a new service session",
                3,
            );
        }
        _ => {
            return fail(
                "rust-microsip: the bootstrap grant is unknown, expired or revoked",
                3,
            );
        }
    }
    // No capture before an authorized call: devices open only while a call's
    // media runs and are released when it ends.
    let media: Box<dyn MediaIo> = Box::new(
        OnDemandMedia::new(
            stored.audio.input_device.clone(),
            stored.audio.output_device.clone(),
        )
        // Reported from the devices themselves, not from the call phase.
        .with_observer(Arc::new(|open| {
            log(if open {
                "microphone active (audio devices opened for a call)"
            } else {
                "microphone released"
            });
        })),
    );
    let Ok(engine) = EngineHandle::spawn(media, Arc::new(|| {})) else {
        return fail("rust-microsip: cannot start the telephony engine", 1);
    };
    if let Some(path) = &stored.ca_file {
        match config::read_ca_file(path, ctx.uid) {
            Ok(pem) => engine.set_trust(TrustAnchors::Custom(pem)),
            Err(error) => {
                let _ = engine.shutdown(Duration::from_secs(2));
                return fail(&format!("rust-microsip: {error}"), 2);
            }
        }
    }
    let Ok(owner) = ControlOwner::new(OwnerMode::Service, epoch, true) else {
        return fail("rust-microsip: no system randomness", 1);
    };
    let server = match ControlServer::start(
        &listener,
        OwnerMode::Service,
        u64::try_from(epoch).unwrap_or(0),
    ) {
        Ok(server) => server,
        Err(error) => {
            return fail(
                &format!("rust-microsip: control socket unavailable: {error}"),
                4,
            );
        }
    };
    let now = Instant::now();
    let mut state = UiState::new(UiSettings {
        account: Some(stored.account.clone()),
        ..UiSettings::default()
    });
    state.attach_store(Some(store), None, true, now);
    state.attach_control(owner);
    let _ = engine.send(Command::Register(
        Box::new(stored.account.clone()),
        password,
    ));
    if stored.ui.dnd {
        let _ = engine.send(Command::SetDnd(true));
    }
    let terminating = Arc::new(AtomicBool::new(false));
    watch_termination(Arc::clone(&terminating));
    log(&format!("service ready (epoch {epoch})"));
    let backend = EngineOnly(&engine);
    while !terminating.load(Ordering::SeqCst) {
        let now = Instant::now();
        for event in engine.try_events() {
            state.apply_event(event, now);
        }
        while let Some(incoming) = server.recv_timeout(Duration::from_millis(20)) {
            state.control_request(incoming, Instant::now());
        }
        state.control_tick(&backend, Instant::now());
    }
    drop(server);
    let stopped = engine.shutdown(Duration::from_secs(4));
    drop(listener);
    log(if stopped {
        "service stopped"
    } else {
        "service stopped (engine did not finish in time)"
    });
    ExitCode::SUCCESS
}
