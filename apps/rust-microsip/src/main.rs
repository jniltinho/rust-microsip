//! rust-microsip entry point: configuration CLI or the desktop softphone.
//!
//! Without a command it opens the window, starts the telephony engine on its
//! own thread and connects the ALSA audio bridge (falling back to silence with
//! a visible notice). A missing or invalid configuration never blocks the
//! window: settings stay reachable and SIP simply does not start. History and
//! contacts use SQLite under the XDG data directory; if it cannot be opened,
//! an in-memory store keeps the session working with a visible warning.

use rust_microsip::ui::{self, Backend, External, Storage, Theme, UiSettings};
use rust_microsip_audio::{AudioDevices, DeviceMedia};
use rust_microsip_config::{
    self as config, CliArgs, CliCommand, ConfigError, DoctorReport, EffectiveConfig,
    ResolveContext, StoredConfig,
};
use rust_microsip_core::{
    CallId, CallPhase, Command, Direction, Event, MediaIo, Notice, NullMedia,
};
use rust_microsip_platform::{
    Ack, Acquired, CallAction, InstanceError, Listener, Notifier, Request, acquire,
    default_runtime_dir, parse_dial_uri,
};
use rust_microsip_storage::{DEFAULT_RETENTION_DAYS, Store};
use rust_microsip_telephony::engine::{EngineHandle, TrustAnchors};
use std::{
    cell::{Cell, RefCell},
    io::Write,
    path::PathBuf,
    process::ExitCode,
    sync::{Arc, OnceLock},
    time::Duration,
};

fn print(text: &str) -> ExitCode {
    match std::io::stdout().lock().write_all(text.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}
fn fail(text: &str, code: u8) -> ExitCode {
    let _ = writeln!(std::io::stderr().lock(), "{text}");
    ExitCode::from(code)
}

/// Engine adapter for the window: non-blocking send/poll plus local notices.
struct EngineBackend {
    engine: Option<EngineHandle>,
    repaint: Arc<OnceLock<eframe::egui::Context>>,
    pending: RefCell<Vec<Event>>,
    /// Single-instance listener (requests from later launches).
    listener: Option<Listener>,
    external: RefCell<Vec<External>>,
    notifier: Notifier,
    ringing: Cell<Option<CallId>>,
}
impl EngineBackend {
    /// Mirror incoming-call state into a desktop notification and turn its
    /// actions into commands, only while that same call is still ringing.
    fn notify(&self, events: &[Event]) {
        for event in events {
            let Event::Call(call) = event else { continue };
            let ringing = call.direction == Direction::Incoming && call.phase == CallPhase::Ringing;
            if ringing && self.ringing.get() != Some(call.id) {
                let _ = self.notifier.incoming_call(call.id, &call.remote);
                self.ringing.set(Some(call.id));
            } else if !ringing && self.ringing.get() == Some(call.id) {
                let _ = self.notifier.close(call.id);
                self.ringing.set(None);
            }
        }
        while let Some(action) = self.notifier.try_recv_action() {
            if self.ringing.get() == Some(action.call) {
                let command = match action.action {
                    CallAction::Answer => Command::Answer(action.call),
                    CallAction::Reject => Command::Reject(action.call),
                };
                let _ = self.send(command);
            }
        }
    }
}
impl Backend for EngineBackend {
    fn send(&self, command: Command) -> Result<(), Notice> {
        self.engine
            .as_ref()
            .ok_or(Notice::Overloaded)?
            .send(command)
    }
    fn poll(&self) -> Vec<Event> {
        let mut events = std::mem::take(&mut *self.pending.borrow_mut());
        if let Some(engine) = &self.engine {
            events.extend(engine.try_events());
        }
        self.notify(&events);
        events
    }
    fn external(&self) -> Vec<External> {
        let mut out = std::mem::take(&mut *self.external.borrow_mut());
        if let Some(listener) = &self.listener {
            while let Some(request) = listener.try_recv() {
                out.push(match request {
                    Request::Activate => External::Activate,
                    Request::Dial(target) => External::Dial(target),
                });
            }
        }
        out
    }
    fn attach_context(&self, ctx: &eframe::egui::Context) {
        let window = ctx.clone();
        let waker: rust_microsip_platform::Waker = Arc::new(move || window.request_repaint());
        if let Some(listener) = &self.listener {
            listener.set_waker(waker.clone());
        }
        self.notifier.set_waker(waker);
        let _ = self.repaint.set(ctx.clone());
    }
    fn audio_devices(&self) -> Option<AudioDevices> {
        AudioDevices::enumerate().ok()
    }
}
impl Drop for EngineBackend {
    fn drop(&mut self) {
        if let Some(engine) = self.engine.take() {
            // Bounded: unregister and BYE are best-effort within this time.
            let _ = engine.shutdown(Duration::from_secs(4));
        }
    }
}

fn ui_theme(theme: config::Theme) -> Theme {
    match theme {
        config::Theme::System => Theme::System,
        config::Theme::Light => Theme::Light,
        config::Theme::Dark => Theme::Dark,
    }
}
fn config_theme(theme: Theme) -> config::Theme {
    match theme {
        Theme::System => config::Theme::System,
        Theme::Light => config::Theme::Light,
        Theme::Dark => config::Theme::Dark,
    }
}

/// `$XDG_DATA_HOME/rust-microsip/rust-microsip.sqlite3`, falling back to
/// `$HOME/.local/share`; only absolute paths from the environment snapshot,
/// never the working directory.
fn store_path(env: &std::collections::BTreeMap<String, String>) -> Option<PathBuf> {
    let abs = |name: &str| env.get(name).map(PathBuf::from).filter(|p| p.is_absolute());
    let base = abs("XDG_DATA_HOME").or_else(|| abs("HOME").map(|h| h.join(".local/share")))?;
    Some(base.join("rust-microsip").join("rust-microsip.sqlite3"))
}

/// Open the history store and apply retention. Never deletes the file: on
/// failure the session uses an in-memory store and says so.
fn open_storage(env: &std::collections::BTreeMap<String, String>) -> Storage {
    let opened = store_path(env)
        .ok_or_else(|| "no data directory (XDG_DATA_HOME/HOME) is available".to_owned())
        .and_then(|path| Store::open(&path).map_err(|e| e.to_string()));
    match opened {
        Ok(mut store) => {
            if let Err(error) = store.prune_older_than(DEFAULT_RETENTION_DAYS) {
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "rust-microsip: history retention not applied: {error}"
                );
            }
            Storage {
                store: Some(store),
                warning: None,
            }
        }
        Err(error) => {
            let _ = writeln!(
                std::io::stderr().lock(),
                "rust-microsip: history storage unavailable: {error}"
            );
            Storage {
                store: Store::open_in_memory().ok(),
                warning: Some(
                    "History and contacts are not saved to disk in this session.".to_owned(),
                ),
            }
        }
    }
}

fn desktop(args: &CliArgs) -> ExitCode {
    // A link is validated before anything else; it only ever fills the number.
    let request = match args.uri.as_deref().map(parse_dial_uri) {
        None => Request::Activate,
        Some(Ok(target)) => Request::Dial(target),
        Some(Err(error)) => return fail(&format!("rust-microsip: link not accepted: {error}"), 2),
    };
    // Single instance: a later launch hands its request to the running window
    // and exits before touching SIP, audio or the database.
    let listener = match default_runtime_dir().and_then(|dir| acquire(&dir, &request)) {
        Ok(Acquired::Owner(listener)) => Some(listener),
        Ok(Acquired::Forwarded(Ack::Ok)) => return ExitCode::SUCCESS,
        Ok(Acquired::Forwarded(ack)) => {
            return fail(
                &format!("rust-microsip: the running instance answered {ack:?}"),
                1,
            );
        }
        // Another instance holds the lock but did not answer: never start a
        // second full instance (duplicate registration and database writers).
        Err(error @ (InstanceError::NoResponse | InstanceError::Protocol)) => {
            return fail(
                &format!("rust-microsip: another instance is running but did not respond: {error}"),
                1,
            );
        }
        // An unsafe runtime directory is a configuration problem to fix, not
        // a reason to run unprotected.
        Err(error @ (InstanceError::Insecure(_) | InstanceError::Io { .. })) => {
            return fail(
                &format!("rust-microsip: cannot ensure a single instance: {error}"),
                1,
            );
        }
        // Sessions without XDG_RUNTIME_DIR: documented degraded mode.
        Err(error) => {
            let _ = writeln!(
                std::io::stderr().lock(),
                "rust-microsip: single-instance protection unavailable: {error}"
            );
            None
        }
    };
    let initial = match request {
        Request::Dial(target) => vec![External::Dial(target)],
        Request::Activate => Vec::new(),
    };
    let ctx = match ResolveContext::from_process(args) {
        Ok(ctx) => ctx,
        Err(error) => return fail(&format!("rust-microsip: {error}"), 1),
    };
    let save_path: Option<PathBuf> = args.config.clone().or_else(|| ctx.default_config_path());
    let resolved = config::resolve(&ctx);
    let mut notices = Vec::new();
    let (stored, password) = match resolved {
        Ok(EffectiveConfig {
            config, password, ..
        }) => (Some(config), password),
        // No configuration yet: first run, open settings from the window.
        Err(ConfigError::Missing(_)) => (None, None),
        Err(ConfigError::FileMissing(_)) if args.config.is_none() => (None, None),
        Err(error) => {
            let _ = writeln!(
                std::io::stderr().lock(),
                "rust-microsip: configuration not loaded: {error}"
            );
            notices.push(Event::Notice(Notice::InvalidState));
            (None, None)
        }
    };
    let audio_prefs = stored.as_ref().map(|s| s.audio.clone()).unwrap_or_default();
    let media: Box<dyn MediaIo> = match DeviceMedia::open(
        audio_prefs.input_device.as_deref(),
        audio_prefs.output_device.as_deref(),
    ) {
        Ok(device) => Box::new(device),
        Err(_) => {
            notices.push(Event::Notice(Notice::AudioUnavailable));
            Box::new(NullMedia)
        }
    };
    let repaint: Arc<OnceLock<eframe::egui::Context>> = Arc::new(OnceLock::new());
    let wake = repaint.clone();
    let engine = match EngineHandle::spawn(
        media,
        Arc::new(move || {
            if let Some(ctx) = wake.get() {
                ctx.request_repaint();
            }
        }),
    ) {
        Ok(engine) => engine,
        Err(_) => return fail("rust-microsip: cannot start the telephony engine", 1),
    };
    if let Some(stored) = &stored {
        // A configured CA bundle replaces the system roots; if it cannot be
        // read safely, do not register at all rather than trust other roots.
        let trust_ok = match &stored.ca_file {
            Some(path) => match config::read_ca_file(path, ctx.uid) {
                Ok(pem) => {
                    engine.set_trust(TrustAnchors::Custom(pem));
                    true
                }
                Err(error) => {
                    let _ = writeln!(std::io::stderr().lock(), "rust-microsip: {error}");
                    notices.push(Event::Notice(Notice::InvalidState));
                    // Later registrations from the window also fail closed.
                    engine.set_trust(TrustAnchors::Unusable);
                    false
                }
            },
            None => true,
        };
        if stored.ui.dnd {
            let _ = engine.send(Command::SetDnd(true));
        }
        if let (Some(password), true) = (password, trust_ok) {
            let _ = engine.send(Command::Register(
                Box::new(stored.account.clone()),
                password,
            ));
        }
    }
    watch_signals(repaint.clone());
    let settings = UiSettings {
        account: stored.as_ref().map(|s| s.account.clone()),
        theme: stored
            .as_ref()
            .map_or(Theme::System, |s| ui_theme(s.ui.theme)),
        audio_input: audio_prefs.input_device.clone(),
        audio_output: audio_prefs.output_device.clone(),
    };
    let storage = open_storage(&ctx.env);
    let mut base = stored;
    let save: ui::SaveSettings = Box::new(move |settings: &UiSettings| {
        let Some(path) = save_path.as_ref() else {
            return Err("No configuration directory (HOME/XDG_CONFIG_HOME) is available.".into());
        };
        let Some(account) = settings.account.clone() else {
            return Err("Configure an account before saving.".into());
        };
        let mut next = base.clone().unwrap_or_else(|| StoredConfig {
            account: account.clone(),
            password_env: config::DEFAULT_PASSWORD_ENV.into(),
            password_file: None,
            ca_file: None,
            ui: config::UiSettings::default(),
            audio: config::AudioSettings::default(),
        });
        next.account = account;
        next.ui.theme = config_theme(settings.theme);
        next.audio.input_device.clone_from(&settings.audio_input);
        next.audio.output_device.clone_from(&settings.audio_output);
        config::save(&next, path).map_err(|_| "Settings could not be saved.".to_owned())?;
        base = Some(next);
        Ok(())
    });
    let backend = EngineBackend {
        engine: Some(engine),
        repaint,
        pending: RefCell::new(notices),
        listener,
        external: RefCell::new(initial),
        notifier: Notifier::session(),
        ringing: Cell::new(None),
    };
    match ui::run(Box::new(backend), settings, save, storage) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => fail(
            "rust-microsip: cannot open the window (no usable display or graphics driver)",
            1,
        ),
    }
}

/// SIGTERM/SIGINT close the window through the normal path, so the engine
/// ends calls and unregisters within its bounded shutdown instead of dying
/// mid-call. Without a window yet, the process exits immediately.
fn watch_signals(window: Arc<OnceLock<eframe::egui::Context>>) {
    let spawned = std::thread::Builder::new()
        .name("rust-microsip-signals".into())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .build()
            else {
                return;
            };
            runtime.block_on(async {
                use tokio::signal::unix::{SignalKind, signal};
                let (Ok(mut term), Ok(mut int)) = (
                    signal(SignalKind::terminate()),
                    signal(SignalKind::interrupt()),
                ) else {
                    return;
                };
                tokio::select! {
                    _ = term.recv() => {},
                    _ = int.recv() => {},
                }
            });
            match window.get() {
                Some(ctx) => ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close),
                None => std::process::exit(143),
            }
        });
    if spawned.is_err() {
        let _ = writeln!(
            std::io::stderr().lock(),
            "rust-microsip: signal handling unavailable"
        );
    }
}

fn main() -> ExitCode {
    let args = match config::parse_args(std::env::args_os().skip(1)) {
        Ok(args) => args,
        // Never echo arguments: they may contain secrets.
        Err(_) => return fail("invalid or unsupported arguments; see --help", 2),
    };
    match args.command {
        CliCommand::Help => print(&format!(
            "rust-microsip {}\n\n{}",
            env!("CARGO_PKG_VERSION"),
            config::HELP
        )),
        CliCommand::Version => print(concat!("rust-microsip ", env!("CARGO_PKG_VERSION"), "\n")),
        CliCommand::Doctor { json } => {
            let result = ResolveContext::from_process(&args).and_then(|ctx| config::resolve(&ctx));
            let report = DoctorReport::new(&result);
            print(&if json {
                report.to_json()
            } else {
                report.to_string()
            })
        }
        CliCommand::ConfigValidate => {
            match ResolveContext::from_process(&args).and_then(|ctx| config::resolve(&ctx)) {
                Ok(_) => print("configuration: OK\n"),
                Err(error) => fail(&format!("configuration: INVALID\nerror: {error}"), 1),
            }
        }
        CliCommand::ConfigExport => {
            match ResolveContext::from_process(&args)
                .and_then(|ctx| config::resolve(&ctx))
                .and_then(|effective| effective.config.to_toml())
            {
                Ok(text) => print(&text),
                Err(error) => fail(&format!("configuration: INVALID\nerror: {error}"), 1),
            }
        }
        CliCommand::Run => desktop(&args),
    }
}
