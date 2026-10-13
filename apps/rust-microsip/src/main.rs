//! rust-microsip entry point: configuration CLI or the desktop softphone.
//!
//! Without a command it opens the window, starts the telephony engine on its
//! own thread and connects the ALSA audio bridge (falling back to silence with
//! a visible notice). A missing or invalid configuration never blocks the
//! window: settings stay reachable and SIP simply does not start.
//!
//! The SQLite database under the XDG data directory holds history, contacts
//! and the settings saved from the window. It is opened before the
//! configuration is resolved, because its values are a configuration layer
//! (below flags, environment, `.env` and TOML). If it cannot be opened, an
//! in-memory store keeps the session usable with a visible warning, but the
//! account is not registered automatically and settings cannot be saved.

use rust_microsip::ui::{
    self, AUDIO_TEST_SECONDS, Backend, DESKTOP_FILE, External, Job, JobResult, Storage, Theme,
    UiSettings, keyring_reference,
};
use rust_microsip_audio::{AudioDevices, DeviceMedia};
use rust_microsip_config::{
    self as config, CliArgs, CliCommand, ConfigError, DoctorReport, Field, ResolveContext, Source,
    StoredConfig,
};
use rust_microsip_core::{
    CallId, CallPhase, Command, Direction, Event, FRAME_SAMPLES, MediaIo, Notice, NullMedia,
};
use rust_microsip_platform::{
    Ack, Acquired, CallAction, InstanceError, Listener, Notifier, Request, SecretStore, Waker,
    acquire, default_runtime_dir, parse_dial_uri,
};
use rust_microsip_storage::{DEFAULT_RETENTION_DAYS, Store};
use rust_microsip_telephony::engine::{EngineHandle, TrustAnchors};
use secrecy::{ExposeSecret, SecretString};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    io::Write,
    path::PathBuf,
    process::ExitCode,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender, TrySendError},
    },
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
    /// Set by the signal thread: quit without asking.
    terminating: Arc<AtomicBool>,
    /// Serialized keyring worker.
    keyring: Option<SyncSender<Job>>,
    results_tx: Sender<JobResult>,
    results: Receiver<JobResult>,
    audio_test_running: Arc<AtomicBool>,
    devices_running: Arc<AtomicBool>,
    wake: Waker,
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
        if self.terminating.swap(false, Ordering::SeqCst) {
            out.push(External::Quit);
        }
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
    fn run_job(&self, job: Job) -> Result<(), &'static str> {
        let (tx, wake) = (self.results_tx.clone(), self.wake.clone());
        let (running, name, work): (_, _, Box<dyn FnOnce() -> JobResult + Send>) = match job {
            Job::AudioTest {
                generation,
                input,
                output,
            } => {
                if self.audio_test_running.swap(true, Ordering::SeqCst) {
                    return Err("the audio test is already running");
                }
                let (tx, wake) = (tx.clone(), wake.clone());
                (
                    &self.audio_test_running,
                    "rust-microsip-audio-test",
                    Box::new(move || JobResult::AudioTestDone {
                        generation,
                        result: audio_test(
                            generation,
                            input.as_deref(),
                            output.as_deref(),
                            &tx,
                            &wake,
                        ),
                    }),
                )
            }
            Job::ListAudioDevices => {
                // A listing already running delivers its result to the window.
                if self.devices_running.swap(true, Ordering::SeqCst) {
                    return Ok(());
                }
                (
                    &self.devices_running,
                    "rust-microsip-audio-devices",
                    Box::new(|| JobResult::AudioDevices(AudioDevices::enumerate().ok())),
                )
            }
            keyring => {
                let keyring_tx = self
                    .keyring
                    .as_ref()
                    .ok_or("the keyring worker is unavailable")?;
                return keyring_tx.try_send(keyring).map_err(|error| match error {
                    TrySendError::Full(_) => "the keyring is busy; try again",
                    TrySendError::Disconnected(_) => "the keyring worker stopped",
                });
            }
        };
        let flag = Arc::clone(running);
        let spawned = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                let result = work();
                flag.store(false, Ordering::SeqCst);
                let _ = tx.send(result);
                wake();
            });
        spawned.map(|_| ()).map_err(|_| {
            running.store(false, Ordering::SeqCst);
            "cannot start a worker thread"
        })
    }
    fn job_results(&self) -> Vec<JobResult> {
        self.results.try_iter().collect()
    }
}

/// Play one second of a 440 Hz tone on the selected output and report the
/// microphone peak, for [`AUDIO_TEST_SECONDS`]. Opens its own streams and
/// closes them when done; runs on a worker thread.
fn audio_test(
    generation: u64,
    input: Option<&str>,
    output: Option<&str>,
    tx: &Sender<JobResult>,
    wake: &Waker,
) -> Result<(), String> {
    const TICK: Duration = Duration::from_millis(20);
    const TONE_TICKS: usize = 50;
    let mut media = DeviceMedia::open(input, output).map_err(|e| e.to_string())?;
    let monitor = media.monitor();
    let (mut tone, mut mic) = ([0_i16; FRAME_SAMPLES], [0_i16; FRAME_SAMPLES]);
    let ticks = usize::try_from(AUDIO_TEST_SECONDS * 50).unwrap_or(150);
    for tick in 0..ticks {
        if tick < TONE_TICKS {
            for (i, sample) in tone.iter_mut().enumerate() {
                let n = (tick * FRAME_SAMPLES + i) as f32;
                let phase = 2.0 * std::f32::consts::PI * 440.0 * n / 8000.0;
                // About -12 dBFS.
                *sample = (8000.0 * phase.sin()) as i16;
            }
            media.play(&tone);
        }
        media.capture(&mut mic);
        if tick % 5 == 4 {
            let _ = tx.send(JobResult::AudioLevel {
                generation,
                peak: monitor.status().input_peak,
            });
            wake();
        }
        std::thread::sleep(TICK);
    }
    if monitor.status().stream_failed {
        return Err("the audio stream stopped with an error".into());
    }
    Ok(())
}

/// Delete the keyring item `reference` (blocking; command-line use only).
fn delete_keyring_item(reference: &str) -> Result<(), String> {
    SecretStore::connect()
        .and_then(|s| s.delete(reference))
        .map_err(|e| e.to_string())
}

/// Run one keyring job. Blocks for up to the Secret Service timeout.
/// `None` for jobs that are not keyring work.
fn keyring_job(job: Job) -> Option<JobResult> {
    let text = |e: rust_microsip_platform::SecretError| e.to_string();
    Some(match job {
        Job::StorePassword {
            reference,
            password,
        } => {
            let label = format!("rust-microsip SIP password ({reference})");
            let result = SecretStore::connect()
                .and_then(|s| s.store(&reference, &label, password.expose_secret().as_bytes()))
                .map_err(text);
            JobResult::PasswordStored { reference, result }
        }
        Job::DeletePassword { reference } => {
            let result = delete_keyring_item(&reference);
            JobResult::PasswordDeleted { reference, result }
        }
        Job::LoadPassword { reference } => {
            let result = SecretStore::connect()
                .and_then(|s| s.retrieve(&reference))
                .map_err(text)
                .and_then(|item| {
                    item.map(|bytes| {
                        std::str::from_utf8(bytes.expose())
                            .map(SecretString::from)
                            .map_err(|_| "the stored password is not valid text".to_owned())
                    })
                    .transpose()
                });
            JobResult::PasswordLoaded { reference, result }
        }
        Job::AudioTest { .. } | Job::ListAudioDevices => return None,
    })
}

/// One thread that runs keyring jobs in order, so a delete queued after a
/// store never overtakes it.
fn spawn_keyring_worker(results: Sender<JobResult>, wake: Waker) -> Option<SyncSender<Job>> {
    let (tx, rx) = mpsc::sync_channel::<Job>(8);
    std::thread::Builder::new()
        .name("rust-microsip-keyring".into())
        .spawn(move || {
            for job in rx {
                let Some(result) = keyring_job(job) else {
                    continue;
                };
                if results.send(result).is_err() {
                    return;
                }
                wake();
            }
        })
        .ok()
        .map(|_| tx)
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

/// `$XDG_DATA_HOME`, falling back to `$HOME/.local/share`; only absolute
/// paths from the environment snapshot, never the working directory.
fn data_home(env: &BTreeMap<String, String>) -> Option<PathBuf> {
    let abs = |name: &str| env.get(name).map(PathBuf::from).filter(|p| p.is_absolute());
    abs("XDG_DATA_HOME").or_else(|| abs("HOME").map(|h| h.join(".local/share")))
}

/// `<data home>/rust-microsip/rust-microsip.sqlite3`.
fn store_path(env: &BTreeMap<String, String>) -> Option<PathBuf> {
    Some(
        data_home(env)?
            .join("rust-microsip")
            .join("rust-microsip.sqlite3"),
    )
}

/// Open the store and read its settings layer.
fn open_store_with_config(
    env: &BTreeMap<String, String>,
) -> Result<(Store, BTreeMap<String, String>), String> {
    let path = store_path(env)
        .ok_or_else(|| "no data directory (XDG_DATA_HOME/HOME) is available".to_owned())?;
    let store = Store::open(&path).map_err(|e| e.to_string())?;
    let values = store.load_config().map_err(|e| e.to_string())?;
    Ok((store, values))
}

/// Configuration context for the command-line commands, including the
/// settings saved in an existing database (none is created here).
fn cli_context(args: &CliArgs) -> Result<ResolveContext, ConfigError> {
    let mut ctx = ResolveContext::from_process(args)?;
    if store_path(&ctx.env).is_some_and(|p| p.is_file()) {
        match open_store_with_config(&ctx.env) {
            Ok((_, values)) => ctx.sqlite = config::sqlite_layer(&values),
            Err(error) => {
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "rust-microsip: settings database not read: {error}"
                );
            }
        }
    }
    Ok(ctx)
}

/// `config import FILE [--apply]`: print the sanitized preview; with
/// `--apply`, replace the stored settings in one transaction. Never touches
/// SIP or audio, and never modifies FILE.
fn config_import(args: &CliArgs, apply: bool) -> ExitCode {
    let Some(file) = args.import.as_deref() else {
        return fail("invalid or unsupported arguments; see --help", 2);
    };
    let ctx = match ResolveContext::from_process(args) {
        Ok(ctx) => ctx,
        Err(error) => return fail(&format!("rust-microsip: {error}"), 1),
    };
    let preview = match config::preview_import(file, ctx.uid) {
        Ok(preview) => preview,
        Err(error) => return fail(&format!("configuration: INVALID\nerror: {error}"), 1),
    };
    let mut text = preview.to_string();
    if apply {
        let Some(path) = store_path(&ctx.env) else {
            return fail(
                "rust-microsip: no data directory (XDG_DATA_HOME/HOME) is available",
                1,
            );
        };
        // The import clears the remembered keyring reference; its item is
        // then deleted too, so no orphaned password stays behind.
        let replaced = Store::open(&path).and_then(|mut store| {
            let old = store.secret_ref().ok().flatten();
            store.replace_config(&preview.sqlite_values())?;
            Ok(old)
        });
        let old = match replaced {
            Ok(old) => old,
            Err(error) => {
                let _ = print(&text);
                return fail(&format!("rust-microsip: nothing imported: {error}"), 1);
            }
        };
        text.push_str(config::IMPORT_APPLIED);
        if let Some(reference) = old
            && let Err(error) = delete_keyring_item(&reference)
        {
            let _ = writeln!(
                std::io::stderr().lock(),
                "rust-microsip: note: the remembered password was not removed from the system \
                 keyring ({error}); remove the rust-microsip item in the desktop keyring"
            );
        }
    }
    print(&text)
}

/// Open the store, apply retention and read the settings layer. Never deletes
/// the file: on failure the session uses an in-memory store and says so.
/// Returns the settings layer only for a durable store. `single` says this
/// process holds the single-instance lock; only then are calls left open by
/// an earlier process closed as interrupted (once, when the window attaches
/// the store).
fn open_storage(
    env: &BTreeMap<String, String>,
    single: bool,
) -> (Storage, Option<BTreeMap<String, String>>) {
    match open_store_with_config(env) {
        Ok((mut store, values)) => {
            if let Err(error) = store.prune_older_than(DEFAULT_RETENTION_DAYS) {
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "rust-microsip: history retention not applied: {error}"
                );
            }
            (
                Storage {
                    store: Some(store),
                    warning: None,
                    recover_interrupted: single,
                },
                Some(values),
            )
        }
        Err(error) => {
            let _ = writeln!(
                std::io::stderr().lock(),
                "rust-microsip: history storage unavailable: {error}"
            );
            (
                Storage {
                    store: Store::open_in_memory().ok(),
                    warning: Some(
                        "History, contacts and settings are not saved to disk in this session."
                            .to_owned(),
                    ),
                    recover_interrupted: false,
                },
                None,
            )
        }
    }
}

/// `key (source)` for every field fixed above the settings database.
fn overridden(sources: &BTreeMap<Field, Source>) -> Vec<String> {
    sources
        .iter()
        .filter(|(_, s)| !matches!(s, Source::Sqlite | Source::Default))
        .map(|(f, s)| format!("{f} ({s})"))
        .collect()
}

/// Fields the settings window edits. Nothing else is ever written from the
/// window, so stored values it does not show (`ca_file`, `password_file`,
/// `password_env`, `expires`, `dnd`) are never replaced or removed, even
/// when the configuration could not be resolved.
const WINDOW_FIELDS: [Field; 11] = [
    Field::DisplayName,
    Field::Username,
    Field::AuthUsername,
    Field::Domain,
    Field::Server,
    Field::Port,
    Field::Transport,
    Field::Media,
    Field::Theme,
    Field::InputDevice,
    Field::OutputDevice,
];

/// The window's settings save: only fields the user changed and no higher
/// source overrides are written, and only to SQLite; the TOML file is never
/// written. `base` is the effective configuration the window started from.
/// `sources` is `None` when provenance could not be computed (unreadable or
/// invalid configuration files): saving is then refused, because a value
/// that a higher source overrides could otherwise be written.
fn settings_saver(
    durable: bool,
    mut sources: Option<BTreeMap<Field, Source>>,
    mut base: Option<StoredConfig>,
) -> ui::SaveSettings {
    Box::new(move |settings: &UiSettings, store: Option<&mut Store>| {
        let Some(store) = store.filter(|_| durable) else {
            return Err("the settings database is unavailable in this session".into());
        };
        let Some(sources) = sources.as_mut() else {
            return Err(
                "the configuration files could not be read, so it is unknown which \
                settings they fix; fix them first (see rust-microsip --doctor)"
                    .into(),
            );
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
        let mut edits =
            config::sqlite_edits(sources, base.as_ref(), &next).map_err(|e| match e {
                ConfigError::Overridden { .. } => e.to_string(),
                _ => "the settings are not valid".to_owned(),
            })?;
        edits.retain(|key, _| Field::from_key(key).is_some_and(|f| WINDOW_FIELDS.contains(&f)));
        store
            .save_config(&edits)
            .map_err(|e| format!("the settings database failed: {e}"))?;
        for key in edits.keys() {
            if let Some(field) = Field::from_key(key) {
                sources.insert(field, Source::Sqlite);
            }
        }
        base = Some(next);
        Ok(())
    })
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
    // `None` only in the degraded mode below, where another instance may run.
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
    let mut ctx = match ResolveContext::from_process(args) {
        Ok(ctx) => ctx,
        Err(error) => return fail(&format!("rust-microsip: {error}"), 1),
    };
    // The settings database is a configuration layer: open it first.
    // Interrupted-call recovery needs the single-instance lock: without it,
    // calls left "open" may belong to another running window.
    let (storage, saved) = open_storage(&ctx.env, listener.is_some());
    let durable = saved.is_some();
    ctx.sqlite = config::sqlite_layer(&saved.unwrap_or_default());
    let resolved = config::resolve(&ctx);
    let mut notices = Vec::new();
    let (stored, password, sources) = match resolved {
        Ok(effective) => {
            let sources = effective.provenance().clone();
            (Some(effective.config), effective.password, Some(sources))
        }
        // No configuration yet: first run, open settings from the window.
        // Without provenance (unreadable files) the window refuses to save.
        Err(ConfigError::Missing(_)) => (None, None, config::layer_sources(&ctx).ok()),
        Err(ConfigError::FileMissing(_)) if args.config.is_none() => {
            (None, None, config::layer_sources(&ctx).ok())
        }
        Err(error) => {
            let _ = writeln!(
                std::io::stderr().lock(),
                "rust-microsip: configuration not loaded: {error}"
            );
            notices.push(Event::Notice(Notice::InvalidState));
            (None, None, config::layer_sources(&ctx).ok())
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
    let mut load_password = None;
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
        if !durable {
            // CFG-08: the saved settings layer is unknown, so the effective
            // account may not be what the user configured.
            let _ = writeln!(
                std::io::stderr().lock(),
                "rust-microsip: not registering automatically: the settings database is unavailable"
            );
        }
        match (password, trust_ok && durable) {
            (Some(password), true) => {
                let _ = engine.send(Command::Register(
                    Box::new(stored.account.clone()),
                    password,
                ));
            }
            // No password from env/.env/file/TOML: use the keyring item the
            // user chose to remember for this exact account, read off the UI
            // thread; the window registers when it arrives.
            // A reference for other account settings (another server, or
            // the older `user@domain` form) is not used: the user is asked.
            (None, true) => {
                let remembered = storage
                    .store
                    .as_ref()
                    .and_then(|s| s.secret_ref().ok().flatten());
                let current = keyring_reference(&stored.account);
                load_password = remembered.map(|r| (current.as_deref() == Some(r.as_str()), r));
            }
            (_, false) => {}
        }
    }
    let terminating = Arc::new(AtomicBool::new(false));
    let signal_code = Arc::new(AtomicU8::new(0));
    watch_signals(
        repaint.clone(),
        Arc::clone(&terminating),
        Arc::clone(&signal_code),
    );
    let settings = UiSettings {
        account: stored.as_ref().map(|s| s.account.clone()),
        theme: stored
            .as_ref()
            .map_or(Theme::System, |s| ui_theme(s.ui.theme)),
        audio_input: audio_prefs.input_device.clone(),
        audio_output: audio_prefs.output_device.clone(),
        overridden: sources.as_ref().map(overridden).unwrap_or_default(),
        desktop_entry: data_home(&ctx.env).map(|d| d.join("applications").join(DESKTOP_FILE)),
    };
    let save = settings_saver(durable, sources, stored);
    let (results_tx, results) = mpsc::channel();
    let window = repaint.clone();
    let wake: Waker = Arc::new(move || {
        if let Some(ctx) = window.get() {
            ctx.request_repaint();
        }
    });
    let backend = EngineBackend {
        engine: Some(engine),
        repaint,
        pending: RefCell::new(notices),
        listener,
        external: RefCell::new(initial),
        notifier: Notifier::session(),
        ringing: Cell::new(None),
        terminating,
        keyring: spawn_keyring_worker(results_tx.clone(), wake.clone()),
        results_tx,
        results,
        audio_test_running: Arc::new(AtomicBool::new(false)),
        devices_running: Arc::new(AtomicBool::new(false)),
        wake,
    };
    match load_password {
        Some((true, reference)) => {
            if let Err(reason) = backend.run_job(Job::LoadPassword { reference }) {
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "rust-microsip: remembered password not read: {reason}"
                );
            }
        }
        // Shown in the window as "enter it in Settings".
        Some((false, reference)) => {
            let _ = backend.results_tx.send(JobResult::PasswordLoaded {
                reference,
                result: Err("it was saved for different account settings".into()),
            });
        }
        None => {}
    }
    match ui::run(Box::new(backend), settings, save, storage) {
        // After SIGTERM/SIGINT: 143 or 130, as if killed by the signal.
        Ok(()) => match signal_code.load(Ordering::SeqCst) {
            0 => ExitCode::SUCCESS,
            code => ExitCode::from(code),
        },
        Err(_) => fail(
            "rust-microsip: cannot open the window (no usable display or graphics driver)",
            1,
        ),
    }
}

/// SIGTERM/SIGINT close the window through the normal path without the
/// "Hang up and quit?" prompt, so the engine ends calls and unregisters
/// within its bounded shutdown instead of dying mid-call; the process then
/// exits with 143 (TERM) or 130 (INT), stored in `code`. Without a window
/// yet, or on a second signal while shutting down, it exits immediately
/// with that code.
fn watch_signals(
    window: Arc<OnceLock<eframe::egui::Context>>,
    terminating: Arc<AtomicBool>,
    code: Arc<AtomicU8>,
) {
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
                let first = tokio::select! {
                    _ = term.recv() => 143,
                    _ = int.recv() => 130,
                };
                code.store(first, Ordering::SeqCst);
                // Set before the close request, so the frame that sees it quits.
                terminating.store(true, Ordering::SeqCst);
                match window.get() {
                    Some(ctx) => ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close),
                    None => std::process::exit(first.into()),
                }
                let second: u8 = tokio::select! {
                    _ = term.recv() => 143,
                    _ = int.recv() => 130,
                };
                std::process::exit(second.into());
            });
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
            let result = cli_context(&args).and_then(|ctx| config::resolve(&ctx));
            let report = DoctorReport::new(&result);
            print(&if json {
                report.to_json()
            } else {
                report.to_string()
            })
        }
        CliCommand::ConfigValidate => {
            match cli_context(&args).and_then(|ctx| config::resolve(&ctx)) {
                Ok(_) => print("configuration: OK\n"),
                Err(error) => fail(&format!("configuration: INVALID\nerror: {error}"), 1),
            }
        }
        CliCommand::ConfigExport => {
            match cli_context(&args)
                .and_then(|ctx| config::resolve(&ctx))
                .and_then(|effective| effective.config.to_toml())
            {
                Ok(text) => print(&text),
                Err(error) => fail(&format!("configuration: INVALID\nerror: {error}"), 1),
            }
        }
        CliCommand::ConfigImport { apply } => config_import(&args, apply),
        CliCommand::Run => desktop(&args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_microsip_core::{AccountSettings, MediaSecurity, Transport};

    fn account() -> AccountSettings {
        AccountSettings {
            display_name: String::new(),
            username: "1001".into(),
            auth_username: None,
            domain: "pbx.invalid".into(),
            server: "192.0.2.1".into(),
            port: 5060,
            transport: Transport::Udp,
            media: MediaSecurity::PlainRtp,
            expires: 300,
        }
    }

    #[test]
    fn window_saves_only_edited_fields_to_sqlite_and_refuses_overrides()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut store = Store::open_in_memory()?;
        let base = StoredConfig {
            account: account(),
            password_env: config::DEFAULT_PASSWORD_ENV.into(),
            password_file: None,
            ca_file: None,
            ui: config::UiSettings::default(),
            audio: config::AudioSettings::default(),
        };
        // The server comes from TOML, the theme from the database.
        let sources = BTreeMap::from([
            (Field::Server, Source::Toml),
            (Field::Theme, Source::Sqlite),
        ]);
        let mut save = settings_saver(true, Some(sources), Some(base));
        let mut settings = UiSettings {
            account: Some(account()),
            theme: Theme::Dark,
            ..UiSettings::default()
        };
        save(&settings, Some(&mut store))?;
        let saved = store.load_config()?;
        assert_eq!(
            saved,
            BTreeMap::from([("ui.theme".to_owned(), "dark".to_owned())]),
            "unchanged and overridden fields are not written"
        );
        // Editing a TOML-held field names the source and writes nothing.
        settings.account = Some(AccountSettings {
            server: "192.0.2.9".into(),
            display_name: "Lab".into(),
            ..account()
        });
        let error = save(&settings, Some(&mut store)).err().unwrap_or_default();
        assert!(
            error.contains("account.server") && error.contains("toml"),
            "{error}"
        );
        assert_eq!(store.load_config()?.len(), 1);
        // Without a durable store nothing is claimed as saved.
        let mut volatile = settings_saver(false, Some(BTreeMap::new()), None);
        assert!(volatile(&settings, Some(&mut store)).is_err());
        Ok(())
    }

    #[test]
    fn first_run_save_skips_fields_held_by_higher_sources() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut store = Store::open_in_memory()?;
        let sources = BTreeMap::from([(Field::Domain, Source::Env)]);
        let mut save = settings_saver(true, Some(sources), None);
        let settings = UiSettings {
            account: Some(account()),
            ..UiSettings::default()
        };
        save(&settings, Some(&mut store))?;
        let saved = store.load_config()?;
        assert_eq!(
            saved.get("account.username").map(String::as_str),
            Some("1001")
        );
        assert!(!saved.contains_key("account.domain"));
        // Only fields the window edits are written.
        assert!(!saved.contains_key("account.password_env") && !saved.contains_key("ui.dnd"));
        assert_eq!(
            overridden(&BTreeMap::from([
                (Field::Domain, Source::Env),
                (Field::Theme, Source::Sqlite)
            ])),
            ["account.domain (env)"]
        );
        Ok(())
    }

    #[test]
    fn window_save_without_provenance_is_refused_and_never_deletes_stored_paths()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut store = Store::open_in_memory()?;
        let stored = BTreeMap::from([
            ("account.ca_file".to_owned(), Some("/srv/ca.pem".to_owned())),
            (
                "account.password_file".to_owned(),
                Some("/srv/sip.pw".to_owned()),
            ),
            ("account.expires".to_owned(), Some("600".to_owned())),
        ]);
        store.save_config(&stored)?;
        let settings = UiSettings {
            account: Some(account()),
            theme: Theme::Dark,
            ..UiSettings::default()
        };
        // Invalid configuration files: provenance unknown, nothing saved.
        let mut blind = settings_saver(true, None, None);
        let error = blind(&settings, Some(&mut store)).err().unwrap_or_default();
        assert!(error.contains("could not be read"), "{error}");
        assert_eq!(store.load_config()?.len(), 3);
        // Missing or invalid configuration with known provenance: the window
        // writes its own fields only; stored paths it does not show survive.
        let mut save = settings_saver(true, Some(BTreeMap::new()), None);
        save(&settings, Some(&mut store))?;
        save(&settings, Some(&mut store))?;
        let saved = store.load_config()?;
        assert_eq!(
            saved.get("account.ca_file").map(String::as_str),
            Some("/srv/ca.pem")
        );
        assert_eq!(
            saved.get("account.password_file").map(String::as_str),
            Some("/srv/sip.pw")
        );
        assert_eq!(
            saved.get("account.expires").map(String::as_str),
            Some("600")
        );
        assert_eq!(saved.get("ui.theme").map(String::as_str), Some("dark"));
        Ok(())
    }

    #[test]
    fn keyring_reference_names_the_server_and_transport() {
        let reference = keyring_reference(&account());
        assert_eq!(
            reference.as_deref(),
            Some("1001@pbx.invalid;server=192.0.2.1:5060;transport=udp")
        );
        let moved = AccountSettings {
            server: "192.0.2.9".into(),
            ..account()
        };
        assert_ne!(keyring_reference(&moved), reference);
        let long = AccountSettings {
            display_name: String::new(),
            username: "u".repeat(250),
            ..account()
        };
        assert_eq!(keyring_reference(&long), None);
    }
}
