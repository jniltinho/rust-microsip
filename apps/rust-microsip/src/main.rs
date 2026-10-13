//! rust-microsip entry point: configuration CLI or the desktop softphone.
//!
//! Without a command it opens the window, starts the telephony engine on its
//! own thread and connects the ALSA audio bridge (falling back to silence with
//! a visible notice). A missing or invalid configuration never blocks the
//! window: settings stay reachable and SIP simply does not start.

use rust_microsip::ui::{self, Backend, Theme, UiSettings};
use rust_microsip_audio::DeviceMedia;
use rust_microsip_config::{
    self as config, CliArgs, CliCommand, ConfigError, DoctorReport, EffectiveConfig,
    ResolveContext, StoredConfig,
};
use rust_microsip_core::{Command, Event, MediaIo, Notice, NullMedia};
use rust_microsip_telephony::engine::{EngineHandle, TrustAnchors};
use std::{
    cell::RefCell,
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
        events
    }
    fn attach_context(&self, ctx: &eframe::egui::Context) {
        let _ = self.repaint.set(ctx.clone());
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

fn desktop(args: &CliArgs) -> ExitCode {
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
    };
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
        config::save(&next, path).map_err(|_| "Settings could not be saved.".to_owned())?;
        base = Some(next);
        Ok(())
    });
    let backend = EngineBackend {
        engine: Some(engine),
        repaint,
        pending: RefCell::new(notices),
    };
    match ui::run(Box::new(backend), settings, save) {
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
