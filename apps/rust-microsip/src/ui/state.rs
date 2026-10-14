//! Pure presentation state and reducer for the softphone window.
//!
//! Nothing here touches egui or the GPU. User intents arrive as [`Action`]s,
//! engine output as [`Event`]s; commands leave through [`Backend`]. The UI
//! never claims a call or registration state before the engine reports it:
//! mute, hold and transfer toggles reflect the latest [`CallSnapshot`].
//!
//! History and contacts live in the SQLite [`Store`]. Its work runs
//! synchronously on the UI thread, but only on discrete triggers (a call
//! start or final snapshot, a click, a search edit), never per frame; each
//! operation is a small indexed query bounded by a row limit. The lists shown
//! are cached here. Keyring and audio-device work never runs here: it goes to
//! the backend as a [`Job`] and comes back as a [`JobResult`].

use super::{Backend, Job, JobResult};
use rust_microsip_audio::{AudioDevices, DeviceInfo};
use rust_microsip_core::{
    AccountSettings, CallId, CallPhase, CallSnapshot, Command, DialTarget, Direction, DtmfKey,
    EndReason, Event, MediaSecurity, Notice, RegistrationFailure, RegistrationState, TransferState,
    Transport, ValidationError,
};
use rust_microsip_storage::{
    CSV_MAX_BYTES, CallOutcome, CallRecord, CallStart, Contact, ContactNumber, CsvIssueKind,
    CsvPreview, HistoryEntry, HistoryFilter, MAX_REMOTE_LEN, NewContact, Store, preview_csv,
};
use secrecy::SecretString;
use secrecy::zeroize::Zeroize;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod control;
mod grants;
pub use grants::{GrantForm, GrantSpec, IssuedGrant, write_private};
/// Maximum characters accepted in the number and transfer entries.
pub const MAX_NUMBER_CHARS: usize = 64;
/// How long a transient notice stays visible.
pub const NOTICE_TTL: Duration = Duration::from_secs(5);
/// Most recent history entries listed.
pub const HISTORY_LIMIT: u32 = 200;
/// Most contacts listed per search.
pub const CONTACTS_LIMIT: u32 = 500;
const DEFAULT_EXPIRES: u32 = 300;

/// Settings save callback: persists secret-free settings into the store
/// (`None` when no store is open). The error string must be a user-safe
/// message without paths or secrets.
pub type SaveFn = dyn FnMut(&UiSettings, Option<&mut Store>) -> Result<(), String>;
/// [`SaveFn`] borrowed for one dispatch (any lifetime).
type SaveRef<'a> = dyn FnMut(&UiSettings, Option<&mut Store>) -> Result<(), String> + 'a;

/// How long the local audio test runs.
pub const AUDIO_TEST_SECONDS: u64 = 3;
/// File name of the opt-in `sip:`/`tel:` handler entry.
pub const DESKTOP_FILE: &str = "rust-microsip.desktop";
/// Longest keyring reference, matching the Secret Service attribute bound.
const MAX_KEYRING_REF: usize = 256;

/// Keyring reference for `account`:
/// `user@domain;server=host:port;transport=udp|tcp|tls`.
///
/// It names the registrar too, so a password remembered for one server is
/// never sent to another one. References from older versions
/// (`user@domain`) simply no longer match and the password is asked again.
/// `None` when the reference would exceed the keyring bound or hold control
/// characters.
pub fn keyring_reference(account: &AccountSettings) -> Option<String> {
    let reference = format!(
        "{}@{};server={}:{};transport={}",
        account.username,
        account.domain,
        account.server,
        account.port,
        account.transport.as_str()
    );
    (reference.len() <= MAX_KEYRING_REF && !reference.chars().any(char::is_control))
        .then_some(reference)
}

/// Top-level tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tab {
    /// Number entry, keypad and call controls.
    #[default]
    Phone,
    /// Persistent call history.
    Calls,
    /// Local contacts.
    Contacts,
}
impl Tab {
    /// Tabs in display order.
    pub const ALL: [Self; 3] = [Self::Phone, Self::Calls, Self::Contacts];
    /// Visible label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Phone => "Phone",
            Self::Calls => "Calls",
            Self::Contacts => "Contacts",
        }
    }
}

/// Color theme preference, applied live.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Theme {
    /// Follow the desktop preference; falls back to light.
    #[default]
    System,
    /// Light visuals.
    Light,
    /// Dark visuals.
    Dark,
}
impl Theme {
    /// Themes in display order.
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];
    /// Visible label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

/// Secret-free settings owned by the UI and handed to the save callback.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UiSettings {
    /// The single SIP account, if configured.
    pub account: Option<AccountSettings>,
    /// Theme preference.
    pub theme: Theme,
    /// Capture device id (or name); `None` is the system default. Applied on
    /// the next start.
    pub audio_input: Option<String>,
    /// Playback device id (or name); `None` is the system default. Applied on
    /// the next start.
    pub audio_output: Option<String>,
    /// Fields fixed by a higher source than the settings database, as
    /// `key (source)`; shown read-only in settings.
    pub overridden: Vec<String>,
    /// Where the opt-in link handler entry is installed
    /// (`$XDG_DATA_HOME/applications/rust-microsip.desktop`); `None` when no
    /// data directory is known.
    pub desktop_entry: Option<PathBuf>,
    /// Private directory for session grant files (`$XDG_RUNTIME_DIR/rust-microsip/grants`).
    pub grant_dir: Option<PathBuf>,
    /// Private directory for headless bootstrap files (`$XDG_CONFIG_HOME/rust-microsip/bootstrap`).
    pub bootstrap_dir: Option<PathBuf>,
}

/// Which content replaces the tab body.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum View {
    /// The selected tab.
    #[default]
    Main,
    /// Account and theme settings.
    Settings,
    /// About and licenses.
    About,
    /// Add or edit a contact ([`UiState::contact_form`]).
    ContactEdit,
    /// CSV contact import ([`UiState::import`]).
    Import,
}

/// A destructive action waiting for confirmation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confirm {
    /// Delete one history entry by row id.
    DeleteCall(i64),
    /// Delete every history entry.
    ClearHistory,
    /// Delete one contact by row id.
    DeleteContact(i64),
    /// Hang up the active call and quit.
    Quit,
    /// Delete the agent control audit log.
    ClearAudit,
}
impl Confirm {
    /// Question shown to the user.
    pub const fn question(self) -> &'static str {
        match self {
            Self::DeleteCall(_) => "Delete this history entry?",
            Self::ClearHistory => "Delete the whole call history?",
            Self::DeleteContact(_) => "Delete this contact?",
            Self::Quit => "Hang up and quit?",
            Self::ClearAudit => "Delete the agent access audit log?",
        }
    }
    /// Label of the confirming button.
    pub const fn button(self) -> &'static str {
        match self {
            Self::Quit => "Hang up and quit",
            _ => "Delete",
        }
    }
}

/// Contact editor buffers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContactForm {
    /// Row id when editing; `None` adds a new contact.
    pub id: Option<i64>,
    /// Display name.
    pub name: String,
    /// Shown first in the list.
    pub favorite: bool,
    /// `(number, label)` rows; rows with an empty number are ignored on save.
    pub numbers: Vec<(String, String)>,
}
impl ContactForm {
    fn from_contact(contact: &Contact) -> Self {
        Self {
            id: Some(contact.id),
            name: contact.name.clone(),
            favorite: contact.favorite,
            numbers: contact
                .numbers
                .iter()
                .map(|n| (n.number.clone(), n.label.clone().unwrap_or_default()))
                .collect(),
        }
    }
    /// Contact data for the store, which validates it.
    pub fn to_contact(&self) -> NewContact {
        NewContact {
            name: self.name.clone(),
            favorite: self.favorite,
            numbers: self
                .numbers
                .iter()
                .filter(|(number, _)| !number.trim().is_empty())
                .map(|(number, label)| ContactNumber {
                    number: number.clone(),
                    label: Some(label.clone()),
                })
                .collect(),
        }
    }
}

/// CSV import screen buffers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportForm {
    /// Absolute path of the CSV file as typed.
    pub path: String,
    /// Validated preview awaiting confirmation; cleared when the path changes.
    pub preview: Option<CsvPreview>,
    /// Why the file could not be previewed or imported.
    pub error: Option<String>,
}

/// Wall-clock and monotonic marks of one call, for history.
#[derive(Clone, Copy, Debug)]
struct CallTimes {
    started_utc: i64,
    answered: Option<(i64, Instant)>,
}

/// The current call plus UI-side timing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveCall {
    /// Latest engine snapshot.
    pub snapshot: CallSnapshot,
    /// Monotonic time of the first `Established` snapshot.
    pub established_at: Option<Instant>,
}
impl ActiveCall {
    /// Connected duration at `now`.
    pub fn duration(&self, now: Instant) -> Option<Duration> {
        self.established_at
            .map(|t| now.saturating_duration_since(t))
    }
    /// An incoming call waiting for a local answer.
    pub fn is_incoming_ringing(&self) -> bool {
        self.snapshot.direction == Direction::Incoming && self.snapshot.phase == CallPhase::Ringing
    }
    /// Established or held.
    pub fn is_connected(&self) -> bool {
        matches!(
            self.snapshot.phase,
            CallPhase::Established | CallPhase::Held
        )
    }
}

/// Why the settings form cannot be saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormError {
    /// The port is not a number between 1 and 65535.
    Port,
    /// No password was typed and none is held for this session.
    PasswordRequired,
    /// Core validation failed.
    Invalid(ValidationError),
}
impl FormError {
    /// User-visible message that never echoes input.
    pub const fn message(self) -> &'static str {
        match self {
            Self::Port => "Port must be a number between 1 and 65535",
            Self::PasswordRequired => "Password is required",
            Self::Invalid(error) => validation_text(error),
        }
    }
}

/// Editable account fields. The password buffer is wiped after save or close.
#[derive(Debug, Default)]
pub struct SettingsForm {
    /// Display name.
    pub display_name: String,
    /// SIP user part.
    pub username: String,
    /// Optional authentication user.
    pub auth_username: String,
    /// SIP domain.
    pub domain: String,
    /// Registrar/proxy host.
    pub server: String,
    /// Port as typed.
    pub port: String,
    /// Signaling transport.
    pub transport: Option<Transport>,
    /// Media protection policy.
    pub media: Option<MediaSecurity>,
    /// Masked password edit buffer; empty keeps the session password.
    pub password: String,
    /// Remember the password in the system keyring.
    pub remember: bool,
    expires: u32,
}
impl SettingsForm {
    /// Fill the form from saved settings, or defaults.
    pub fn from_account(account: Option<&AccountSettings>) -> Self {
        match account {
            Some(a) => Self {
                display_name: a.display_name.clone(),
                username: a.username.clone(),
                auth_username: a.auth_username.clone().unwrap_or_default(),
                domain: a.domain.clone(),
                server: a.server.clone(),
                port: a.port.to_string(),
                transport: Some(a.transport),
                media: Some(a.media),
                password: String::new(),
                remember: false,
                expires: a.expires,
            },
            None => Self {
                port: Transport::Udp.default_port().to_string(),
                transport: Some(Transport::Udp),
                media: Some(MediaSecurity::PlainRtp),
                expires: DEFAULT_EXPIRES,
                ..Self::default()
            },
        }
    }
    /// Selected transport (UDP when unset).
    pub fn transport(&self) -> Transport {
        self.transport.unwrap_or(Transport::Udp)
    }
    /// Selected media policy (plain RTP when unset).
    pub fn media(&self) -> MediaSecurity {
        self.media.unwrap_or(MediaSecurity::PlainRtp)
    }
    /// Change transport, moving the port along if it was the old default.
    pub fn set_transport(&mut self, transport: Transport) {
        if self.port.trim() == self.transport().default_port().to_string() {
            self.port = transport.default_port().to_string();
        }
        self.transport = Some(transport);
    }
    /// Build validated account settings (the password is handled separately).
    ///
    /// # Errors
    /// [`FormError::Port`] or [`FormError::Invalid`].
    pub fn to_account(&self) -> Result<AccountSettings, FormError> {
        let port = self
            .port
            .trim()
            .parse::<u16>()
            .map_err(|_| FormError::Port)?;
        let auth = self.auth_username.trim();
        let account = AccountSettings {
            display_name: self.display_name.trim().to_owned(),
            username: self.username.trim().to_owned(),
            auth_username: (!auth.is_empty()).then(|| auth.to_owned()),
            domain: self.domain.trim().to_owned(),
            server: self.server.trim().to_owned(),
            port,
            transport: self.transport(),
            media: self.media(),
            expires: self.expires,
        };
        account.validate().map_err(FormError::Invalid)?;
        Ok(account)
    }
    /// Wipe the password buffer.
    pub fn wipe_password(&mut self) {
        self.password.zeroize();
    }
}

/// A user intent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Approve a pending control request (by operation id).
    ApproveControl(String),
    /// Deny a pending control request (by operation id).
    DenyControl(String),
    /// Deny every pending control request and pause new ones.
    DenyAllControl,
    /// Issue a grant from the form: for this session, or as a bootstrap for the headless service.
    CreateGrant {
        /// Export for the headless service.
        bootstrap: bool,
    },
    /// Revoke a grant by id.
    RevokeGrant(String),
    /// Ask to delete the agent access audit log.
    ClearAudit,
    /// Switch tab (also returns to the main view).
    SelectTab(Tab),
    /// Keypad or keyboard key: DTMF while established, else appended.
    Key(char),
    /// Remove the last number character.
    Backspace,
    /// Clear the number.
    ClearNumber,
    /// Append `+`.
    Plus,
    /// Answer an incoming call, or dial the number.
    Call,
    /// Reject an incoming call, or hang up.
    End,
    /// Toggle microphone mute.
    ToggleMute,
    /// Hold or resume.
    ToggleHold,
    /// Toggle do-not-disturb.
    ToggleDnd,
    /// Open the inline transfer prompt.
    OpenTransfer,
    /// Send the transfer.
    ConfirmTransfer,
    /// Close the transfer prompt.
    CancelTransfer,
    /// Put a number into the entry and show the phone tab (no dialing).
    FillNumber(String),
    /// Put a number into the entry and dial it.
    DialNumber(String),
    /// Show only matching history entries.
    SetHistoryFilter(HistoryFilter),
    /// Re-run the history query with the current search text.
    SearchHistory,
    /// Ask to delete one history entry.
    DeleteCall(i64),
    /// Ask to delete the whole history.
    ClearHistory,
    /// Re-run the contact query with the current search text.
    SearchContacts,
    /// Open the editor for a new contact.
    NewContact,
    /// Open the editor for a stored contact.
    EditContact(i64),
    /// Add an empty number row to the editor.
    AddNumberRow,
    /// Remove one number row from the editor.
    RemoveNumberRow(usize),
    /// Validate and store the editor contents.
    SaveContact,
    /// Ask to delete a contact.
    DeleteContact(i64),
    /// Open the CSV import screen.
    OpenImport,
    /// Read the typed file and show its preview.
    PreviewImport,
    /// Apply the shown preview atomically.
    ConfirmImport,
    /// Run the pending destructive action.
    Confirm,
    /// Drop the pending destructive action.
    CancelConfirm,
    /// Persist the capture device (`None`: system default).
    SetAudioInput(Option<String>),
    /// Persist the playback device (`None`: system default).
    SetAudioOutput(Option<String>),
    /// Show settings.
    OpenSettings,
    /// Show about/licenses.
    OpenAbout,
    /// Return to the main view, discarding unsaved form edits.
    CloseView,
    /// Change and persist the theme immediately.
    SetTheme(Theme),
    /// Validate, persist and register with the form contents.
    SaveSettings,
    /// Remove the registration binding.
    Unregister,
    /// Quit; asks for confirmation while a call is active.
    Quit,
    /// Shut the engine down and close the window without asking (confirmed,
    /// or SIGTERM/SIGINT).
    QuitNow,
    /// Play a short tone and show the microphone level, without a call.
    TestAudio,
    /// Install the opt-in `sip:`/`tel:` handler desktop entry.
    InstallLinkHandler,
    /// Remove the handler desktop entry.
    RemoveLinkHandler,
}

/// Whole-window presentation state.
#[derive(Debug)]
pub struct UiState {
    /// Selected tab.
    pub tab: Tab,
    /// Current view.
    pub view: View,
    /// Number entry buffer.
    pub number: String,
    /// Transfer target buffer while the prompt is open.
    pub transfer_target: Option<String>,
    /// Settings form buffers.
    pub form: SettingsForm,
    /// Latest registration state.
    pub registration: RegistrationState,
    /// Current call, if any.
    pub call: Option<ActiveCall>,
    /// Listed history entries, newest first.
    pub history: Vec<HistoryEntry>,
    /// History list filter.
    pub history_filter: HistoryFilter,
    /// History search text (remote party substring).
    pub history_search: String,
    /// Listed contacts, favorites first.
    pub contacts: Vec<Contact>,
    /// Contact search text (name or number substring).
    pub contact_search: String,
    /// Contact editor, while [`View::ContactEdit`] is shown.
    pub contact_form: Option<ContactForm>,
    /// CSV import screen, while [`View::Import`] is shown.
    pub import: Option<ImportForm>,
    /// Destructive action awaiting confirmation.
    pub confirm: Option<Confirm>,
    /// Devices listed when settings opened; `None` when unavailable.
    pub audio_devices: Option<AudioDevices>,
    /// The device list is being read off the UI thread.
    pub audio_devices_loading: bool,
    /// Saved capture device selection.
    pub audio_input: Option<String>,
    /// Saved playback device selection.
    pub audio_output: Option<String>,
    /// Persistent warning when history/contacts are not saved to disk.
    pub storage_warning: Option<String>,
    /// Do-not-disturb as last sent successfully.
    pub dnd: bool,
    /// Theme preference.
    pub theme: Theme,
    /// Saved account.
    pub account: Option<AccountSettings>,
    /// Shutdown was requested.
    pub quitting: bool,
    /// The engine reported it stopped.
    pub stopped: bool,
    /// The engine died; the status bar says so until restart.
    pub engine_failed: bool,
    /// Latest microphone peak (0-32767) while the audio test runs.
    pub audio_test: Option<u16>,
    /// Run id of the latest audio test; older results are ignored.
    audio_test_generation: u64,
    /// Fields fixed by a higher source, shown read-only in settings.
    pub overridden: Vec<String>,
    /// Target path of the link handler entry.
    pub desktop_entry: Option<PathBuf>,
    password: Option<SecretString>,
    /// Keyring reference whose store job is in flight.
    remember_pending: Option<String>,
    notice: Option<(String, Instant)>,
    versions: HashMap<CallId, u64>,
    times: HashMap<CallId, CallTimes>,
    store: Option<Store>,
    session: String,
    control: Option<crate::owner::ControlOwner>,
    /// Approval prompt on screen and when it appeared (arming delay).
    pub approval_shown: Option<(String, Instant)>,
    /// Last frame that drew the approval prompt.
    pub approval_frame: Option<Instant>,
    /// "Agent access" form in Settings.
    pub grant_form: GrantForm,
    grant_dir: Option<PathBuf>,
    bootstrap_dir: Option<PathBuf>,
}

impl UiState {
    /// Serve control requests as this owner session (see [`crate::owner`]).
    pub fn attach_control(&mut self, owner: crate::owner::ControlOwner) {
        self.control = Some(owner);
    }

    /// Initial state from saved settings.
    pub fn new(settings: UiSettings) -> Self {
        Self {
            tab: Tab::Phone,
            view: View::Main,
            number: String::new(),
            transfer_target: None,
            form: SettingsForm::from_account(settings.account.as_ref()),
            registration: RegistrationState::Disabled,
            call: None,
            history: Vec::new(),
            history_filter: HistoryFilter::All,
            history_search: String::new(),
            contacts: Vec::new(),
            contact_search: String::new(),
            contact_form: None,
            import: None,
            confirm: None,
            audio_devices: None,
            audio_devices_loading: false,
            audio_input: settings.audio_input,
            audio_output: settings.audio_output,
            audio_test: None,
            audio_test_generation: 0,
            overridden: settings.overridden,
            desktop_entry: settings.desktop_entry,
            remember_pending: None,
            storage_warning: None,
            dnd: false,
            theme: settings.theme,
            account: settings.account,
            quitting: false,
            stopped: false,
            engine_failed: false,
            password: None,
            notice: None,
            versions: HashMap::new(),
            times: HashMap::new(),
            store: None,
            control: None,
            approval_shown: None,
            approval_frame: None,
            grant_form: GrantForm::default(),
            grant_dir: settings.grant_dir,
            bootstrap_dir: settings.bootstrap_dir,
            // Unique per process start, so session keys never collide.
            session: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
                .to_string(),
        }
    }

    /// Attach the history/contacts store, close calls an earlier process left
    /// open as interrupted (only with `recover`, see
    /// [`super::Storage::recover_interrupted`]), and load the lists. `warning`
    /// stays visible in the Calls and Contacts tabs (for example when the store
    /// is in memory only). Without a store nothing is recorded. Call once,
    /// before any call starts.
    pub fn attach_store(
        &mut self,
        store: Option<Store>,
        warning: Option<String>,
        recover: bool,
        now: Instant,
    ) {
        self.storage_warning = match (&store, warning) {
            (_, Some(text)) => Some(text),
            (None, None) => Some("History and contacts are unavailable.".to_owned()),
            (Some(_), None) => None,
        };
        self.store = store;
        match self
            .store
            .as_mut()
            .filter(|_| recover)
            .map(Store::recover_interrupted)
        {
            Some(Ok(0)) | None => {}
            Some(Ok(n)) => self.set_notice(
                format!("{n} call(s) interrupted by an earlier exit were added to history"),
                now,
            ),
            Some(Err(error)) => self.set_notice(format!("History not recovered: {error}"), now),
        }
        self.refresh_history(now);
        self.refresh_contacts(now);
    }

    fn settings(&self) -> UiSettings {
        UiSettings {
            account: self.account.clone(),
            theme: self.theme,
            audio_input: self.audio_input.clone(),
            audio_output: self.audio_output.clone(),
            overridden: self.overridden.clone(),
            desktop_entry: self.desktop_entry.clone(),
            grant_dir: self.grant_dir.clone(),
            bootstrap_dir: self.bootstrap_dir.clone(),
        }
    }

    /// Keyring reference stored for the account, if any.
    pub fn secret_ref(&self) -> Option<String> {
        self.store
            .as_ref()
            .and_then(|s| s.secret_ref().ok().flatten())
    }

    /// Whether the local audio test can start; the error is the reason.
    pub fn can_test_audio(&self) -> Result<(), &'static str> {
        if self.call.is_some() {
            Err("The call is using the audio devices")
        } else if self.audio_test.is_some() {
            Err("The audio test is running")
        } else {
            Ok(())
        }
    }

    fn refresh_history(&mut self, now: Instant) {
        let Some(store) = &self.store else { return };
        let search = Some(self.history_search.as_str()).filter(|s| !s.trim().is_empty());
        match store.recent_calls(HISTORY_LIMIT, self.history_filter, search) {
            Ok(list) => self.history = list,
            Err(error) => self.set_notice(format!("History not loaded: {error}"), now),
        }
    }

    fn refresh_contacts(&mut self, now: Instant) {
        let Some(store) = &self.store else { return };
        match store.search_contacts(&self.contact_search, CONTACTS_LIMIT) {
            Ok(list) => self.contacts = list,
            Err(error) => self.set_notice(format!("Contacts not loaded: {error}"), now),
        }
    }

    /// The notice to show at `now`, if still fresh.
    pub fn notice(&self, now: Instant) -> Option<&str> {
        self.notice
            .as_ref()
            .filter(|(_, at)| now.saturating_duration_since(*at) < NOTICE_TTL)
            .map(|(text, _)| text.as_str())
    }

    /// Time until the visible notice expires.
    pub fn notice_remaining(&self, now: Instant) -> Option<Duration> {
        self.notice
            .as_ref()
            .and_then(|(_, at)| NOTICE_TTL.checked_sub(now.saturating_duration_since(*at)))
            .filter(|d| !d.is_zero())
    }

    fn set_notice(&mut self, text: impl Into<String>, now: Instant) {
        self.notice = Some((text.into(), now));
    }

    /// Visible label of the call button.
    pub fn call_label(&self) -> &'static str {
        if self
            .call
            .as_ref()
            .is_some_and(ActiveCall::is_incoming_ringing)
        {
            "Answer"
        } else {
            "Call"
        }
    }
    /// Whether Call is possible; the error is the hover reason.
    pub fn can_call(&self) -> Result<(), &'static str> {
        match &self.call {
            Some(c) if c.is_incoming_ringing() => Ok(()),
            Some(_) => Err("A call is already active"),
            None if self.number.trim().is_empty() => Err("Enter a number first"),
            None => Ok(()),
        }
    }
    /// Visible label of the end button.
    pub fn end_label(&self) -> &'static str {
        if self
            .call
            .as_ref()
            .is_some_and(ActiveCall::is_incoming_ringing)
        {
            "Reject"
        } else {
            "End"
        }
    }
    /// Whether End is possible.
    pub fn can_end(&self) -> Result<CallId, &'static str> {
        match &self.call {
            Some(c) if c.snapshot.phase == CallPhase::Terminating => Err("The call is ending"),
            Some(c) => Ok(c.snapshot.id),
            None => Err("No active call"),
        }
    }
    /// Call usable for mute/hold/transfer.
    pub fn connected_call(&self) -> Result<&CallSnapshot, &'static str> {
        match &self.call {
            Some(c) if c.is_connected() => Ok(&c.snapshot),
            Some(_) => Err("Available once the call is connected"),
            None => Err("No active call"),
        }
    }
    fn established_call(&self) -> Option<CallId> {
        self.call
            .as_ref()
            .filter(|c| c.snapshot.phase == CallPhase::Established)
            .map(|c| c.snapshot.id)
    }
    /// Whether keypad keys currently send DTMF.
    pub fn keypad_sends_dtmf(&self) -> bool {
        self.established_call().is_some()
    }

    fn send(&mut self, backend: &dyn Backend, command: Command, now: Instant) -> bool {
        match backend.send(command) {
            Ok(()) => true,
            Err(notice) => {
                self.apply_event(Event::Notice(notice), now);
                false
            }
        }
    }

    fn push_number(&mut self, c: char) {
        if self.number.chars().count() < MAX_NUMBER_CHARS {
            self.number.push(c);
        }
    }

    /// Apply one user intent, sending at most one command.
    pub fn dispatch(
        &mut self,
        action: Action,
        backend: &dyn Backend,
        save: &mut SaveRef<'_>,
        now: Instant,
    ) {
        match action {
            Action::SelectTab(tab) => {
                self.close_view();
                self.tab = tab;
            }
            Action::Key(c) => {
                if let Some(id) = self.established_call() {
                    if let Ok(key) = DtmfKey::new(c) {
                        self.send(backend, Command::Dtmf(id, key), now);
                    }
                } else if c.is_ascii_digit() || c == '*' || c == '#' {
                    self.push_number(c);
                }
            }
            Action::Backspace => {
                self.number.pop();
            }
            Action::ClearNumber => self.number.clear(),
            Action::Plus => self.push_number('+'),
            Action::Call => match &self.call {
                Some(c) if c.is_incoming_ringing() => {
                    let id = c.snapshot.id;
                    self.send(backend, Command::Answer(id), now);
                }
                Some(_) => self.set_notice("A call is already active", now),
                None => match DialTarget::parse(&self.number) {
                    Ok(target) => {
                        self.send(backend, Command::Dial(target), now);
                    }
                    Err(error) => self.set_notice(validation_text(error), now),
                },
            },
            Action::End => match &self.call {
                Some(c) if c.is_incoming_ringing() => {
                    let id = c.snapshot.id;
                    self.send(backend, Command::Reject(id), now);
                }
                _ => {
                    if let Ok(id) = self.can_end() {
                        self.send(backend, Command::Hangup(id), now);
                    }
                }
            },
            Action::ToggleMute => {
                if let Ok(call) = self.connected_call() {
                    let command = Command::SetMute(call.id, !call.muted);
                    self.send(backend, command, now);
                }
            }
            Action::ToggleHold => {
                if let Ok(call) = self.connected_call() {
                    let command = if call.phase == CallPhase::Held {
                        Command::Resume(call.id)
                    } else {
                        Command::Hold(call.id)
                    };
                    self.send(backend, command, now);
                }
            }
            Action::ToggleDnd => {
                // No confirmation event exists for DND; the flag follows a
                // successfully queued command.
                if self.send(backend, Command::SetDnd(!self.dnd), now) {
                    self.dnd = !self.dnd;
                }
            }
            Action::OpenTransfer => {
                if self.connected_call().is_ok() {
                    self.transfer_target = Some(String::new());
                }
            }
            Action::ConfirmTransfer => {
                let Some(target) = self.transfer_target.as_deref() else {
                    return;
                };
                let id = match self.connected_call() {
                    Ok(call) => call.id,
                    Err(reason) => return self.set_notice(reason, now),
                };
                match DialTarget::parse(target) {
                    Ok(target) => {
                        if self.send(backend, Command::Transfer(id, target), now) {
                            self.transfer_target = None;
                        }
                    }
                    Err(error) => self.set_notice(validation_text(error), now),
                }
            }
            Action::CancelTransfer => self.transfer_target = None,
            Action::FillNumber(number) => {
                self.close_view();
                self.number = number.chars().take(MAX_NUMBER_CHARS).collect();
                self.tab = Tab::Phone;
            }
            Action::DialNumber(number) => {
                if self.call.is_some() {
                    return self.set_notice("A call is already active", now);
                }
                self.dispatch(Action::FillNumber(number), backend, save, now);
                self.dispatch(Action::Call, backend, save, now);
            }
            Action::SetHistoryFilter(filter) => {
                self.history_filter = filter;
                self.refresh_history(now);
            }
            Action::SearchHistory => self.refresh_history(now),
            Action::DeleteCall(id) => self.confirm = Some(Confirm::DeleteCall(id)),
            Action::ClearHistory => self.confirm = Some(Confirm::ClearHistory),
            Action::ClearAudit => self.confirm = Some(Confirm::ClearAudit),
            Action::SearchContacts => self.refresh_contacts(now),
            Action::NewContact => {
                self.close_view();
                self.contact_form = Some(ContactForm {
                    numbers: vec![(String::new(), String::new())],
                    ..ContactForm::default()
                });
                self.view = View::ContactEdit;
            }
            Action::EditContact(id) => {
                if let Some(contact) = self.contacts.iter().find(|c| c.id == id) {
                    let form = ContactForm::from_contact(contact);
                    self.close_view();
                    self.contact_form = Some(form);
                    self.view = View::ContactEdit;
                }
            }
            Action::AddNumberRow => {
                if let Some(form) = &mut self.contact_form {
                    form.numbers.push((String::new(), String::new()));
                }
            }
            Action::RemoveNumberRow(index) => {
                if let Some(form) = self
                    .contact_form
                    .as_mut()
                    .filter(|f| index < f.numbers.len())
                {
                    form.numbers.remove(index);
                }
            }
            Action::SaveContact => self.save_contact(now),
            Action::DeleteContact(id) => self.confirm = Some(Confirm::DeleteContact(id)),
            Action::OpenImport => {
                self.close_view();
                self.import = Some(ImportForm::default());
                self.view = View::Import;
            }
            Action::PreviewImport => {
                if let Some(form) = &mut self.import {
                    let result = read_csv_file(&form.path).and_then(|text| {
                        preview_csv(&text).map_err(|e| format!("Not importable: {e}"))
                    });
                    (form.preview, form.error) = match result {
                        Ok(preview) => (Some(preview), None),
                        Err(error) => (None, Some(error)),
                    };
                }
            }
            Action::ConfirmImport => self.confirm_import(now),
            Action::Confirm => self.run_confirm(backend, now),
            Action::CancelConfirm => self.confirm = None,
            Action::SetAudioInput(device) => {
                let old = std::mem::replace(&mut self.audio_input, device);
                self.save_audio(save, now, |st| st.audio_input = old);
            }
            Action::SetAudioOutput(device) => {
                let old = std::mem::replace(&mut self.audio_output, device);
                self.save_audio(save, now, |st| st.audio_output = old);
            }
            Action::OpenSettings => {
                self.close_view();
                self.form = SettingsForm::from_account(self.account.as_ref());
                self.form.remember = self.secret_ref().is_some() || self.remember_pending.is_some();
                // ALSA probing may block: listed off the UI thread.
                self.audio_devices = None;
                self.audio_devices_loading = backend.run_job(Job::ListAudioDevices).is_ok();
                self.view = View::Settings;
            }
            Action::OpenAbout => {
                self.close_view();
                self.view = View::About;
            }
            Action::CloseView => self.close_view(),
            Action::SetTheme(theme) => {
                self.theme = theme;
                if let Err(message) = save(&self.settings(), self.store.as_mut()) {
                    self.set_notice(format!("Theme not saved: {message}"), now);
                }
            }
            Action::SaveSettings => self.save_settings(backend, save, now),
            Action::Unregister => {
                self.send(backend, Command::Unregister, now);
            }
            Action::Quit if self.call.is_some() && !self.quitting => {
                self.confirm = Some(Confirm::Quit);
            }
            Action::Quit | Action::QuitNow => self.quit_now(backend, now),
            Action::ApproveControl(id) => self.control_decide(&id, true, backend, now),
            Action::DenyControl(id) => self.control_decide(&id, false, backend, now),
            Action::DenyAllControl => self.control_deny_all(backend, now),
            Action::CreateGrant { bootstrap } => self.create_grant_file(bootstrap, now),
            Action::RevokeGrant(id) => self.revoke_grant(&id),
            Action::TestAudio => {
                if let Err(reason) = self.can_test_audio() {
                    return self.set_notice(reason, now);
                }
                self.audio_test_generation += 1;
                let job = Job::AudioTest {
                    generation: self.audio_test_generation,
                    input: self.audio_input.clone(),
                    output: self.audio_output.clone(),
                };
                match backend.run_job(job) {
                    Ok(()) => self.audio_test = Some(0),
                    Err(reason) => {
                        self.set_notice(format!("Audio test unavailable: {reason}"), now)
                    }
                }
            }
            Action::InstallLinkHandler => {
                let text = match self.desktop_entry.as_deref().map(install_desktop_entry) {
                    None => "No data directory (XDG_DATA_HOME/HOME) is available".to_owned(),
                    Some(Ok(())) => {
                        "Link handler entry installed; see the command below to make it the default"
                            .to_owned()
                    }
                    Some(Err(error)) => format!("Link handler not installed: {}", io_text(&error)),
                };
                self.set_notice(text, now);
            }
            Action::RemoveLinkHandler => {
                let text = match self.desktop_entry.as_deref().map(remove_desktop_entry) {
                    None => "No data directory (XDG_DATA_HOME/HOME) is available".to_owned(),
                    Some(Ok(true)) => "Link handler entry removed".to_owned(),
                    Some(Ok(false)) => "The link handler entry was not installed by rust-microsip \
                        or was changed; it was left in place"
                        .to_owned(),
                    Some(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                        "The link handler entry is not installed".to_owned()
                    }
                    Some(Err(error)) => format!("Link handler not removed: {}", io_text(&error)),
                };
                self.set_notice(text, now);
            }
        }
    }

    fn quit_now(&mut self, backend: &dyn Backend, now: Instant) {
        if !self.quitting {
            self.quitting = true;
            self.confirm = None;
            self.send(backend, Command::Shutdown, now);
        }
    }

    /// Apply the result of a background [`Job`].
    pub fn apply_job(&mut self, result: JobResult, backend: &dyn Backend, now: Instant) {
        match result {
            JobResult::PasswordStored { reference, result } => {
                if self.remember_pending.as_ref() != Some(&reference) {
                    return; // Superseded by a later save.
                }
                self.remember_pending = None;
                let saved = result.and_then(|()| {
                    self.store
                        .as_mut()
                        .ok_or_else(|| "history storage is unavailable".to_owned())?
                        .set_secret_ref(Some(&reference))
                        .map_err(|e| e.to_string())
                });
                match saved {
                    Ok(()) => self.set_notice("Password remembered in the system keyring", now),
                    Err(error) => self.set_notice(
                        format!(
                            "Password not remembered ({error}); it is kept for this session only"
                        ),
                        now,
                    ),
                }
            }
            JobResult::PasswordDeleted { result, .. } => {
                if let Err(error) = result {
                    self.set_notice(
                        format!("The keyring entry could not be removed ({error}); remove it in the desktop keyring"),
                        now,
                    );
                }
            }
            JobResult::PasswordLoaded { reference, result } => match result {
                Ok(Some(password)) => {
                    let account = self
                        .account
                        .clone()
                        .filter(|a| keyring_reference(a).as_deref() == Some(reference.as_str()));
                    if let (Some(account), None, false) = (account, &self.password, self.quitting) {
                        self.password = Some(password.clone());
                        self.send(backend, Command::Register(Box::new(account), password), now);
                    }
                }
                Ok(None) => self.set_notice(
                    "No remembered password in the system keyring; enter it in Settings",
                    now,
                ),
                Err(error) => self.set_notice(
                    format!("Remembered password unavailable ({error}); enter it in Settings"),
                    now,
                ),
            },
            JobResult::AudioDevices(devices) => {
                self.audio_devices_loading = false;
                self.audio_devices = devices;
            }
            // Results of an earlier run never touch a newer one.
            JobResult::AudioLevel { generation, peak } => {
                if self.audio_test.is_some() && generation == self.audio_test_generation {
                    self.audio_test = Some(peak);
                }
            }
            JobResult::AudioTestDone { generation, result } => {
                if generation != self.audio_test_generation {
                    return;
                }
                self.audio_test = None;
                match result {
                    Ok(()) => self.set_notice("Audio test finished", now),
                    Err(error) => self.set_notice(format!("Audio test failed: {error}"), now),
                }
            }
        }
    }

    /// Queue keyring work after a successful settings save.
    fn update_keyring(
        &mut self,
        backend: &dyn Backend,
        reference: Option<String>,
        typed: bool,
        password: &SecretString,
        now: Instant,
    ) {
        let current = self.secret_ref();
        let pending = self.remember_pending.take();
        if self.form.remember && reference.is_none() {
            self.set_notice(
                "Password not remembered (the account settings are too long for the keyring); \
                 it is kept for this session only",
                now,
            );
        }
        // Every reference that may hold an item and is no longer wanted.
        let wanted = reference.as_deref().filter(|_| self.form.remember);
        let mut stale: Vec<String> = current
            .iter()
            .chain(pending.iter())
            .filter(|r| Some(r.as_str()) != wanted)
            .cloned()
            .collect();
        stale.dedup();
        if current.as_deref().is_some_and(|r| Some(r) != wanted)
            && let Some(store) = self.store.as_mut()
            && let Err(error) = store.set_secret_ref(None)
        {
            self.set_notice(format!("Keyring reference not cleared: {error}"), now);
        }
        for old in stale {
            if let Err(reason) = backend.run_job(Job::DeletePassword { reference: old }) {
                self.set_notice(
                    format!("The keyring entry could not be removed ({reason})"),
                    now,
                );
            }
        }
        let unchanged = current.as_deref() == wanted && !typed;
        if let (Some(wanted), false) = (wanted, unchanged) {
            let job = Job::StorePassword {
                reference: wanted.to_owned(),
                password: password.clone(),
            };
            match backend.run_job(job) {
                Ok(()) => self.remember_pending = Some(wanted.to_owned()),
                Err(reason) => self.set_notice(
                    format!("Password not remembered ({reason}); it is kept for this session only"),
                    now,
                ),
            }
        }
    }

    fn close_view(&mut self) {
        self.form.wipe_password();
        self.contact_form = None;
        self.import = None;
        self.confirm = None;
        self.view = View::Main;
    }

    fn save_audio(&mut self, save: &mut SaveRef<'_>, now: Instant, revert: impl FnOnce(&mut Self)) {
        match save(&self.settings(), self.store.as_mut()) {
            Ok(()) => self.set_notice("Audio device saved; it applies on the next start", now),
            Err(message) => {
                revert(self);
                self.set_notice(format!("Audio device not saved: {message}"), now);
            }
        }
    }

    fn save_contact(&mut self, now: Instant) {
        let (Some(form), Some(store)) = (&self.contact_form, &mut self.store) else {
            return;
        };
        let contact = form.to_contact();
        let result = match form.id {
            Some(id) => store.update_contact(id, &contact),
            None => store.add_contact(&contact).map(|_| ()),
        };
        match result {
            Ok(()) => {
                self.close_view();
                self.refresh_contacts(now);
                self.set_notice("Contact saved", now);
            }
            // Messages name the field only, never the input.
            Err(error) => self.set_notice(format!("Contact not saved: {error}"), now),
        }
    }

    fn confirm_import(&mut self, now: Instant) {
        let Some(store) = &mut self.store else { return };
        let Some(preview) = self.import.as_ref().and_then(|f| f.preview.as_ref()) else {
            return;
        };
        match store.import_csv(preview) {
            Ok(summary) => {
                self.close_view();
                self.refresh_contacts(now);
                self.set_notice(
                    format!(
                        "Imported: {} new, {} merged contacts, {} numbers",
                        summary.contacts_created, summary.contacts_merged, summary.numbers_added
                    ),
                    now,
                );
            }
            Err(error) => {
                if let Some(form) = &mut self.import {
                    form.error = Some(format!("Nothing imported: {error}"));
                }
            }
        }
    }

    fn run_confirm(&mut self, backend: &dyn Backend, now: Instant) {
        if self.confirm == Some(Confirm::Quit) {
            return self.quit_now(backend, now);
        }
        let (Some(confirm), Some(store)) = (self.confirm.take(), &mut self.store) else {
            return;
        };
        let result = match confirm {
            Confirm::DeleteCall(id) => store.delete_call(id),
            Confirm::ClearHistory => store.clear_history().map(|_| ()),
            Confirm::DeleteContact(id) => store.delete_contact(id),
            Confirm::ClearAudit => store.control_audit_clear().map(|_| ()),
            Confirm::Quit => Ok(()),
        };
        if let Err(error) = result {
            self.set_notice(format!("Not deleted: {error}"), now);
        }
        self.refresh_history(now);
        self.refresh_contacts(now);
    }

    fn save_settings(&mut self, backend: &dyn Backend, save: &mut SaveRef<'_>, now: Instant) {
        let account = match self.form.to_account() {
            Ok(account) => account,
            Err(error) => return self.set_notice(error.message(), now),
        };
        let typed = !self.form.password.is_empty();
        let password = if !typed {
            match &self.password {
                Some(secret) => secret.clone(),
                None => return self.set_notice(FormError::PasswordRequired.message(), now),
            }
        } else {
            SecretString::from(self.form.password.as_str())
        };
        let settings = UiSettings {
            account: Some(account.clone()),
            ..self.settings()
        };
        if let Err(message) = save(&settings, self.store.as_mut()) {
            return self.set_notice(format!("Settings not saved: {message}"), now);
        }
        self.account = Some(account.clone());
        self.password = Some(password.clone());
        if self.send(
            backend,
            Command::Register(Box::new(account.clone()), password.clone()),
            now,
        ) {
            self.set_notice("Settings saved; registering", now);
        }
        let reference = keyring_reference(&account);
        self.update_keyring(backend, reference, typed, &password, now);
        self.close_view();
    }

    /// Apply one engine event.
    pub fn apply_event(&mut self, event: Event, now: Instant) {
        match event {
            Event::Registration(state) => self.registration = state,
            // While quitting or after a normal stop the queue closes on purpose.
            Event::Notice(Notice::EngineFailed) if self.stopped || self.quitting => {}
            Event::Notice(notice) => {
                self.engine_failed |= notice == Notice::EngineFailed;
                self.set_notice(notice_text(notice), now);
            }
            Event::Stopped => self.stopped = true,
            Event::Call(snapshot) => self.apply_call(snapshot, now),
        }
    }

    fn apply_call(&mut self, snapshot: CallSnapshot, now: Instant) {
        if self
            .versions
            .get(&snapshot.id)
            .is_some_and(|v| *v >= snapshot.version)
        {
            return;
        }
        self.versions.insert(snapshot.id, snapshot.version);
        let is_current = self
            .call
            .as_ref()
            .is_some_and(|c| c.snapshot.id == snapshot.id);
        if !self.times.contains_key(&snapshot.id) && !matches!(snapshot.phase, CallPhase::Ended(_))
        {
            self.journal_start(&snapshot, now);
        }
        let times = self.times.entry(snapshot.id).or_insert(CallTimes {
            started_utc: utc_now(),
            answered: None,
        });
        if matches!(snapshot.phase, CallPhase::Established | CallPhase::Held)
            && times.answered.is_none()
        {
            times.answered = Some((utc_now(), now));
        }
        if let CallPhase::Ended(reason) = snapshot.phase {
            let times = self.times.remove(&snapshot.id);
            if is_current {
                self.call = None;
                self.transfer_target = None;
            }
            if let Some(times) = times {
                self.record_call(&snapshot, reason, times, now);
            }
            return;
        }
        if snapshot.transfer == Some(TransferState::Succeeded) && is_current {
            self.transfer_target = None;
        }
        match &mut self.call {
            Some(call) if is_current => {
                if snapshot.phase == CallPhase::Established && call.established_at.is_none() {
                    call.established_at = Some(now);
                }
                call.snapshot = snapshot;
            }
            // One call at a time: a second call is shown only once it ends.
            Some(_) => {}
            None => {
                let established_at = (snapshot.phase == CallPhase::Established).then_some(now);
                self.call = Some(ActiveCall {
                    snapshot,
                    established_at,
                });
            }
        }
    }

    fn session_key(&self, id: CallId) -> String {
        format!("{}:{}", self.session, id.0)
    }

    /// Journal a call as it starts, so a crash leaves an interrupted entry.
    fn journal_start(&mut self, snap: &CallSnapshot, now: Instant) {
        let start = CallStart {
            session_key: self.session_key(snap.id),
            started_utc: utc_now(),
            direction: snap.direction,
            remote: sanitize_remote(&snap.remote),
            secure: snap.secure,
        };
        if let Some(Err(error)) = self.store.as_mut().map(|s| s.begin_call(&start)) {
            self.set_notice(format!("Call not saved to history: {error}"), now);
        }
    }

    /// Record one final snapshot; the session key makes it idempotent.
    fn record_call(
        &mut self,
        snap: &CallSnapshot,
        reason: EndReason,
        times: CallTimes,
        now: Instant,
    ) {
        let session_key = self.session_key(snap.id);
        let Some(store) = &mut self.store else { return };
        let answered = times.answered;
        let record = CallRecord {
            session_key,
            started_utc: times.started_utc,
            answered_utc: answered.map(|(utc, _)| utc),
            ended_utc: Some(utc_now()),
            direction: snap.direction,
            remote: sanitize_remote(&snap.remote),
            outcome: CallOutcome::from_end(snap.direction, answered.is_some(), reason),
            duration_ms: answered.and_then(|(_, at)| {
                u64::try_from(now.saturating_duration_since(at).as_millis()).ok()
            }),
            secure: snap.secure,
        };
        match store.finish_call(&record) {
            Ok(_) => self.refresh_history(now),
            Err(error) => self.set_notice(format!("Call not saved to history: {error}"), now),
        }
    }

    /// Status bar account text.
    pub fn account_text(&self) -> String {
        self.account.as_ref().map_or_else(
            || "No account configured".to_owned(),
            |a| format!("{}@{}", a.username, a.domain),
        )
    }
}

/// Status bar registration text.
pub fn registration_text(state: RegistrationState) -> String {
    match state {
        RegistrationState::Disabled => "Disabled".to_owned(),
        RegistrationState::Registering => "Registering\u{2026}".to_owned(),
        RegistrationState::Registered => "Registered".to_owned(),
        RegistrationState::Failed(reason) => format!(
            "Failed: {}",
            match reason {
                RegistrationFailure::Unauthorized => "wrong credentials".to_owned(),
                RegistrationFailure::Timeout => "no response".to_owned(),
                RegistrationFailure::Certificate => "certificate rejected".to_owned(),
                RegistrationFailure::Network => "network error".to_owned(),
                RegistrationFailure::Rejected(code) => format!("rejected ({code})"),
            }
        ),
    }
}

/// Human phase text for a call.
pub const fn phase_text(call: &CallSnapshot) -> &'static str {
    match (call.phase, call.direction) {
        (CallPhase::Dialing, _) => "Calling\u{2026}",
        (CallPhase::Ringing, Direction::Outgoing) => "Ringing\u{2026}",
        (CallPhase::Ringing, Direction::Incoming) => "Incoming call",
        (CallPhase::Established, _) => "Connected",
        (CallPhase::Held, _) => "On hold",
        (CallPhase::Terminating, _) => "Ending\u{2026}",
        (CallPhase::Ended(reason), _) => end_reason_text(reason),
    }
}

/// Human text for a call outcome.
pub const fn end_reason_text(reason: EndReason) -> &'static str {
    match reason {
        EndReason::Normal => "Ended",
        EndReason::Canceled => "Canceled",
        EndReason::Rejected => "Rejected",
        EndReason::Busy => "Busy",
        EndReason::NoAnswer => "No answer",
        EndReason::Transferred => "Transferred",
        EndReason::NotAcceptable => "Not acceptable",
        EndReason::Failed => "Failed",
    }
}

/// Human text for a transfer state.
pub fn transfer_text(state: TransferState) -> String {
    match state {
        TransferState::Pending => "Transfer pending\u{2026}".to_owned(),
        TransferState::Succeeded => "Transfer succeeded".to_owned(),
        TransferState::Failed(code) => format!("Transfer failed ({code})"),
    }
}

/// Human text for an engine notice.
pub const fn notice_text(notice: Notice) -> &'static str {
    match notice {
        Notice::Busy => "Another call is already active",
        Notice::NotRegistered => "Not registered",
        Notice::UnknownCall => "The call no longer exists",
        Notice::InvalidState => "Not possible in the current call state",
        Notice::AudioUnavailable => "No audio device; the call continues without local audio",
        Notice::Overloaded => "Busy; the action was dropped, try again",
        Notice::EngineFailed => "The phone engine stopped; restart the application",
    }
}

/// Human text for a validation error; never echoes input.
pub const fn validation_text(error: ValidationError) -> &'static str {
    match error {
        ValidationError::Empty => "A required field is empty",
        ValidationError::TooLong => "A field is too long",
        ValidationError::InvalidCharacters => "A field contains unsupported characters",
        ValidationError::InvalidPort => "Port must be between 1 and 65535",
        ValidationError::SrtpRequiresTls => "SRTP requires TLS signaling",
    }
}

/// Unix seconds now.
pub fn utc_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Remote text the store accepts: no control characters, bounded length.
fn sanitize_remote(remote: &str) -> String {
    let mut out = String::new();
    for c in remote.chars().filter(|c| !c.is_control()) {
        if out.len() + c.len_utf8() > MAX_REMOTE_LEN {
            break;
        }
        out.push(c);
    }
    out
}

/// User-safe text for a file error; never includes the path.
fn io_text(error: &std::io::Error) -> &'static str {
    match error.kind() {
        std::io::ErrorKind::PermissionDenied => "permission denied",
        std::io::ErrorKind::NotFound => "the directory does not exist",
        _ => "the file could not be written",
    }
}

/// Write the [`rust_microsip_platform::desktop_entry`] for this executable
/// ([`rust_microsip_platform::desktop_executable`]) to `path` atomically with
/// mode `0644`, creating the `applications` directory when missing, and the
/// app icon beside it in the data directory's hicolor theme (Wayland docks
/// take the icon from the entry, not from the window). Nothing is executed
/// and no default handler is changed.
pub fn install_desktop_entry(path: &Path) -> std::io::Result<()> {
    let executable = rust_microsip_platform::desktop_executable();
    let entry = rust_microsip_platform::desktop_entry(executable.as_deref());
    write_public(path, entry.as_bytes())?;
    match icon_path(path) {
        Some(icon) => write_public(&icon, super::ICON),
        None => Ok(()),
    }
}

/// `<data>/icons/hicolor/256x256/apps/rust-microsip.png` for the entry at
/// `<data>/applications/rust-microsip.desktop`.
fn icon_path(entry: &Path) -> Option<std::path::PathBuf> {
    let data = entry.parent()?.parent()?;
    Some(data.join("icons/hicolor/256x256/apps/rust-microsip.png"))
}

/// Atomic write with mode `0644`, creating the parent directory.
fn write_public(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let dir = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().ok_or(std::io::ErrorKind::InvalidInput)?;
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    let _ = std::fs::remove_file(&tmp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o644);
    let written = options
        .open(&tmp)
        .and_then(|mut file| {
            // Not subject to the umask: desktop entries are world-readable.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(std::fs::Permissions::from_mode(0o644))?;
            }
            file.write_all(bytes)?;
            file.sync_all()
        })
        .and_then(|()| std::fs::rename(&tmp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// Delete the link handler entry at `path` only if it is exactly what
/// [`install_desktop_entry`] writes. Returns `Ok(false)`, leaving the file,
/// when it is not a regular file or its content differs (another
/// application's entry, or one the user edited).
///
/// # Errors
/// I/O failures, including [`std::io::ErrorKind::NotFound`].
pub fn remove_desktop_entry(path: &Path) -> std::io::Result<bool> {
    use rust_microsip_platform::{
        PrivateFileError, desktop_entry, desktop_executable, read_own_file,
    };
    // Our icon goes with our entry, or alone when the entry is already gone;
    // an entry kept as someone's edit keeps its icon.
    let remove_icon = || {
        if let Some(icon) = icon_path(path)
            && read_own_file(&icon, super::ICON.len() as u64).is_ok_and(|b| b == super::ICON)
        {
            let _ = std::fs::remove_file(icon);
        }
    };
    let ours = [
        desktop_entry(None),
        desktop_entry(desktop_executable().as_deref()),
    ];
    // One O_NOFOLLOW open and a bounded read: a swapped symlink or FIFO is refused.
    match read_own_file(path, 8192) {
        Ok(bytes) if ours.iter().any(|e| e.as_bytes() == bytes) => {
            std::fs::remove_file(path)?;
            remove_icon();
            Ok(true)
        }
        Err(PrivateFileError::Unreadable) if std::fs::symlink_metadata(path).is_err() => {
            remove_icon();
            Err(std::io::ErrorKind::NotFound.into())
        }
        _ => Ok(false),
    }
}

/// Read a CSV file for preview: absolute path, regular file, at most
/// [`CSV_MAX_BYTES`], UTF-8. Errors are user-safe and never echo the path.
fn read_csv_file(path: &str) -> Result<String, String> {
    let path = Path::new(path.trim());
    if !path.is_absolute() {
        return Err("Enter the absolute path of a CSV file".into());
    }
    let unreadable = || "The file cannot be read".to_owned();
    let meta = std::fs::metadata(path).map_err(|_| unreadable())?;
    // Refuses FIFOs and devices, which could block or never end.
    if !meta.is_file() {
        return Err("The path is not a regular file".into());
    }
    let limit = u64::try_from(CSV_MAX_BYTES).unwrap_or(u64::MAX);
    if meta.len() > limit {
        return Err("The file exceeds the 10 MiB limit".into());
    }
    let mut text = String::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(limit.saturating_add(1)).read_to_string(&mut text))
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::InvalidData => "The file is not UTF-8 text".to_owned(),
            _ => unreadable(),
        })?;
    if text.len() > CSV_MAX_BYTES {
        return Err("The file exceeds the 10 MiB limit".into());
    }
    Ok(text)
}

/// CSV issues listed individually on the import screen.
pub const MAX_ISSUES_SHOWN: usize = 50;

/// Human text for a CSV row problem.
pub const fn csv_issue_text(kind: CsvIssueKind) -> &'static str {
    match kind {
        CsvIssueKind::Malformed => "broken quoting",
        CsvIssueKind::ColumnCount => "wrong number of columns",
        CsvIssueKind::InvalidName => "invalid name",
        CsvIssueKind::InvalidNumber => "invalid number",
        CsvIssueKind::InvalidLabel => "invalid label",
        CsvIssueKind::TooManyNumbers => "too many numbers for one contact",
    }
}

/// Value persisted for a device: its stable id, or its name without one.
pub fn device_key(device: &DeviceInfo) -> String {
    if device.id.is_empty() {
        device.name.clone()
    } else {
        device.id.clone()
    }
}

/// Coarse age of a history entry from elapsed seconds.
pub fn age_text(seconds: i64) -> String {
    match seconds.max(0) {
        s if s < 60 => "just now".to_owned(),
        s if s < 3600 => format!("{} min ago", s / 60),
        s if s < 86_400 => format!("{} h ago", s / 3600),
        s => format!("{} d ago", s / 86_400),
    }
}

/// Visible label of a history outcome.
pub const fn outcome_text(outcome: CallOutcome) -> &'static str {
    match outcome {
        CallOutcome::Answered => "Answered",
        CallOutcome::Missed => "Missed",
        CallOutcome::Canceled => "Canceled",
        CallOutcome::Rejected => "Rejected",
        CallOutcome::Busy => "Busy",
        CallOutcome::NoAnswer => "No answer",
        CallOutcome::Failed => "Failed",
        CallOutcome::Interrupted => "Interrupted",
    }
}

/// `m:ss` or `h:mm:ss`.
pub fn format_duration(duration: Duration) -> String {
    let s = duration.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        sent: RefCell<Vec<String>>,
        fail: Option<Notice>,
        jobs: RefCell<Vec<String>>,
    }
    impl Backend for Fake {
        fn send(&self, command: Command) -> Result<(), Notice> {
            if let Some(notice) = self.fail {
                return Err(notice);
            }
            let text = match &command {
                Command::Dial(t) => format!("Dial({t})"),
                Command::Transfer(id, t) => format!("Transfer({}, {t})", id.0),
                Command::Dtmf(id, k) => format!("Dtmf({}, {})", id.0, k.as_char()),
                other => format!("{other:?}"),
            };
            self.sent.borrow_mut().push(text);
            Ok(())
        }
        fn poll(&self) -> Vec<Event> {
            Vec::new()
        }
        fn run_job(&self, job: Job) -> Result<(), &'static str> {
            // Opening settings always lists devices; not recorded.
            if !matches!(job, Job::ListAudioDevices) {
                self.jobs.borrow_mut().push(format!("{job:?}"));
            }
            Ok(())
        }
    }
    impl Fake {
        fn take(&self) -> Vec<String> {
            self.sent.take()
        }
    }

    fn no_save() -> impl FnMut(&UiSettings, Option<&mut Store>) -> Result<(), String> {
        |_, _| Ok(())
    }

    fn snap(id: u64, direction: Direction, phase: CallPhase, version: u64) -> CallSnapshot {
        CallSnapshot {
            id: CallId(id),
            direction,
            remote: "1002".into(),
            phase,
            muted: false,
            secure: true,
            transfer: None,
            version,
        }
    }

    fn run(state: &mut UiState, backend: &Fake, actions: &[Action]) {
        let mut save = no_save();
        for action in actions {
            state.dispatch(action.clone(), backend, &mut save, Instant::now());
        }
    }

    fn stored() -> UiState {
        let mut st = UiState::new(UiSettings::default());
        st.attach_store(Store::open_in_memory().ok(), None, true, Instant::now());
        assert!(st.storage_warning.is_none());
        st
    }

    fn end(st: &mut UiState, id: u64, direction: Direction, answered: bool, reason: EndReason) {
        let now = Instant::now();
        let mut s = snap(id, direction, CallPhase::Ringing, 1);
        s.remote = format!("10{id:02}");
        st.apply_event(Event::Call(s.clone()), now);
        if answered {
            s.phase = CallPhase::Established;
            s.version = 2;
            st.apply_event(Event::Call(s.clone()), now);
        }
        s.phase = CallPhase::Ended(reason);
        s.version = 3;
        st.apply_event(Event::Call(s), now);
    }

    #[test]
    fn history_records_once_and_filters_and_searches() {
        let mut st = stored();
        let b = Fake::default();
        end(&mut st, 1, Direction::Incoming, false, EndReason::Canceled);
        end(&mut st, 2, Direction::Outgoing, true, EndReason::Normal);
        end(&mut st, 3, Direction::Incoming, true, EndReason::Normal);
        end(&mut st, 4, Direction::Incoming, false, EndReason::Rejected);
        // A repeated final snapshot is not recorded again.
        end(&mut st, 4, Direction::Incoming, false, EndReason::Rejected);
        assert_eq!(st.history.len(), 4);
        let remotes = |st: &UiState| -> Vec<String> {
            st.history.iter().map(|h| h.call.remote.clone()).collect()
        };
        run(
            &mut st,
            &b,
            &[Action::SetHistoryFilter(HistoryFilter::Missed)],
        );
        assert_eq!(remotes(&st), ["1001"]);
        assert_eq!(st.history[0].call.duration_ms, None);
        run(
            &mut st,
            &b,
            &[Action::SetHistoryFilter(HistoryFilter::Outgoing)],
        );
        assert_eq!(remotes(&st), ["1002"]);
        assert!(st.history[0].call.answered_utc.is_some());
        run(
            &mut st,
            &b,
            &[Action::SetHistoryFilter(HistoryFilter::Incoming)],
        );
        assert_eq!(st.history.len(), 3);
        st.history_search = "003".into();
        run(&mut st, &b, &[Action::SearchHistory]);
        assert_eq!(remotes(&st), ["1003"]);
        // Deletion only happens after confirmation.
        let id = st.history[0].id;
        run(
            &mut st,
            &b,
            &[Action::DeleteCall(id), Action::CancelConfirm],
        );
        assert_eq!(st.history.len(), 1);
        run(&mut st, &b, &[Action::DeleteCall(id), Action::Confirm]);
        assert!(st.history.is_empty());
        st.history_search.clear();
        run(
            &mut st,
            &b,
            &[
                Action::SetHistoryFilter(HistoryFilter::All),
                Action::ClearHistory,
            ],
        );
        assert_eq!(st.history.len(), 3);
        run(&mut st, &b, &[Action::Confirm]);
        assert!(st.history.is_empty() && st.confirm.is_none());
    }

    #[test]
    fn redial_from_history_needs_an_explicit_action() {
        let mut st = stored();
        let b = Fake::default();
        end(&mut st, 5, Direction::Outgoing, false, EndReason::Busy);
        let remote = st.history[0].call.remote.clone();
        st.tab = Tab::Calls;
        run(&mut st, &b, &[Action::FillNumber(remote.clone())]);
        assert!(b.take().is_empty());
        assert_eq!((st.tab, st.number.as_str()), (Tab::Phone, "1005"));
        run(&mut st, &b, &[Action::DialNumber(remote)]);
        assert_eq!(b.take(), ["Dial(1005)"]);
    }

    #[test]
    fn contact_editor_validates_and_persists() {
        let mut st = stored();
        let b = Fake::default();
        let now = Instant::now();
        run(&mut st, &b, &[Action::NewContact]);
        assert_eq!(st.view, View::ContactEdit);
        run(&mut st, &b, &[Action::SaveContact]);
        assert_eq!(
            st.notice(now),
            Some("Contact not saved: invalid contact name")
        );
        let form = st.contact_form.as_mut().map(|f| {
            f.name = "Ana Example".into();
            f.numbers[0] = ("1002".into(), "work".into());
        });
        assert!(form.is_some());
        run(&mut st, &b, &[Action::AddNumberRow]);
        if let Some(f) = st.contact_form.as_mut() {
            f.numbers[1].0 = "bad number".into();
        }
        run(&mut st, &b, &[Action::SaveContact]);
        assert_eq!(
            st.notice(now),
            Some("Contact not saved: invalid phone number")
        );
        assert_eq!(st.view, View::ContactEdit);
        if let Some(f) = st.contact_form.as_mut() {
            f.numbers[1].0 = "1002".into();
        }
        run(&mut st, &b, &[Action::SaveContact]);
        assert_eq!(
            st.notice(now),
            Some("Contact not saved: invalid duplicate number")
        );
        if let Some(f) = st.contact_form.as_mut() {
            f.numbers[1] = ("+5511900000000".into(), String::new());
            f.favorite = true;
        }
        run(&mut st, &b, &[Action::AddNumberRow, Action::SaveContact]);
        assert_eq!(st.view, View::Main);
        assert_eq!(st.contacts.len(), 1);
        let ana = &st.contacts[0];
        assert!(ana.favorite && ana.numbers.len() == 2);
        assert_eq!(ana.numbers[0].label.as_deref(), Some("work"));
        assert_eq!(ana.numbers[1].label, None);
        // Edit keeps the id; a second contact sorts after the favorite.
        let id = ana.id;
        run(&mut st, &b, &[Action::EditContact(id)]);
        if let Some(f) = st.contact_form.as_mut() {
            f.name = "Ana Renamed".into();
        }
        run(
            &mut st,
            &b,
            &[Action::RemoveNumberRow(1), Action::SaveContact],
        );
        run(&mut st, &b, &[Action::NewContact]);
        if let Some(f) = st.contact_form.as_mut() {
            f.name = "Aaron Example".into();
            f.numbers[0].0 = "1003".into();
        }
        run(&mut st, &b, &[Action::SaveContact]);
        let names: Vec<_> = st.contacts.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Ana Renamed", "Aaron Example"]);
        assert_eq!(st.contacts[0].numbers.len(), 1);
        st.contact_search = "1003".into();
        run(&mut st, &b, &[Action::SearchContacts]);
        assert_eq!(st.contacts.len(), 1);
        let id = st.contacts[0].id;
        run(&mut st, &b, &[Action::DeleteContact(id), Action::Confirm]);
        assert!(st.contacts.is_empty());
        assert!(b.take().is_empty());
    }

    #[test]
    fn csv_import_previews_then_imports_atomically() -> std::io::Result<()> {
        let dir = std::env::temp_dir().join(format!("rust-microsip-ui-csv-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let bad = dir.join("bad.csv");
        let good = dir.join("good.csv");
        std::fs::write(&bad, "name,number\nAna,1002\nAna,1002\nBob,not a number\n")?;
        std::fs::write(
            &good,
            "name,number,label\nAna,1002,work\nAna,1002,work\nAna,1003,\nBob,1004,\n",
        )?;
        let mut st = stored();
        let b = Fake::default();
        run(&mut st, &b, &[Action::OpenImport]);
        let set_path = |st: &mut UiState, path: &str| {
            if let Some(f) = st.import.as_mut() {
                f.path = path.into();
            }
        };
        set_path(&mut st, "relative.csv");
        run(&mut st, &b, &[Action::PreviewImport]);
        let error = st.import.as_ref().and_then(|f| f.error.clone());
        assert_eq!(
            error.as_deref(),
            Some("Enter the absolute path of a CSV file")
        );
        set_path(&mut st, &dir.to_string_lossy());
        run(&mut st, &b, &[Action::PreviewImport]);
        let error = st.import.as_ref().and_then(|f| f.error.clone());
        assert_eq!(error.as_deref(), Some("The path is not a regular file"));
        set_path(&mut st, &bad.to_string_lossy());
        run(&mut st, &b, &[Action::PreviewImport]);
        let preview = st.import.as_ref().and_then(|f| f.preview.clone());
        let preview = preview.ok_or(std::io::ErrorKind::InvalidData)?;
        assert_eq!((preview.record_count(), preview.duplicate_count()), (3, 1));
        assert_eq!(preview.issues().len(), 1);
        assert_eq!(preview.issues()[0].line, 4);
        assert_eq!(csv_issue_text(preview.issues()[0].kind), "invalid number");
        run(&mut st, &b, &[Action::ConfirmImport]);
        assert!(st.contacts.is_empty() && st.view == View::Import);
        set_path(&mut st, &good.to_string_lossy());
        run(&mut st, &b, &[Action::PreviewImport, Action::ConfirmImport]);
        assert_eq!(st.view, View::Main);
        assert_eq!(st.contacts.len(), 2);
        assert_eq!(
            st.notice(Instant::now()),
            Some("Imported: 2 new, 0 merged contacts, 3 numbers")
        );
        std::fs::remove_dir_all(dir)
    }

    #[test]
    fn audio_selection_is_saved_with_other_settings() {
        let (mut st, b) = (UiState::new(UiSettings::default()), Fake::default());
        let saved = RefCell::new(Vec::new());
        let fail = std::cell::Cell::new(false);
        let mut save = |s: &UiSettings, _: Option<&mut Store>| -> Result<(), String> {
            if fail.get() {
                return Err("Configure an account before saving.".into());
            }
            saved.borrow_mut().push(s.clone());
            Ok(())
        };
        let now = Instant::now();
        st.dispatch(Action::SetTheme(Theme::Dark), &b, &mut save, now);
        st.dispatch(
            Action::SetAudioInput(Some("alsa:hw:1".into())),
            &b,
            &mut save,
            now,
        );
        st.dispatch(
            Action::SetAudioOutput(Some("alsa:hw:2".into())),
            &b,
            &mut save,
            now,
        );
        assert_eq!(
            st.notice(now),
            Some("Audio device saved; it applies on the next start")
        );
        let last = saved.borrow().last().cloned().unwrap_or_default();
        assert_eq!(last.theme, Theme::Dark);
        assert_eq!(last.audio_input.as_deref(), Some("alsa:hw:1"));
        assert_eq!(last.audio_output.as_deref(), Some("alsa:hw:2"));
        // A failed save leaves the previous selection.
        fail.set(true);
        st.dispatch(Action::SetAudioInput(None), &b, &mut save, now);
        assert_eq!(st.audio_input.as_deref(), Some("alsa:hw:1"));
        // The theme save keeps the audio selection.
        fail.set(false);
        st.dispatch(Action::SetTheme(Theme::Light), &b, &mut save, now);
        let last = saved.borrow().last().cloned().unwrap_or_default();
        assert_eq!(last.audio_output.as_deref(), Some("alsa:hw:2"));
        let device = DeviceInfo {
            id: String::new(),
            name: "USB Headset".into(),
            is_default: false,
        };
        assert_eq!(device_key(&device), "USB Headset");
    }

    #[test]
    fn missing_store_warns_and_never_records() {
        let mut st = UiState::new(UiSettings::default());
        st.attach_store(None, None, true, Instant::now());
        assert!(st.storage_warning.is_some());
        end(&mut st, 1, Direction::Incoming, false, EndReason::Canceled);
        assert!(st.history.is_empty() && st.call.is_none());
        assert_eq!(sanitize_remote("10\u{7}02"), "1002");
        assert_eq!(sanitize_remote(&"9".repeat(400)).len(), MAX_REMOTE_LEN);
        assert_eq!(age_text(7_200), "2 h ago");
    }

    #[test]
    fn dial_validates_number_before_sending() {
        let (mut st, b) = (UiState::new(UiSettings::default()), Fake::default());
        assert!(st.can_call().is_err());
        st.number = "10 02".into();
        run(&mut st, &b, &[Action::Call]);
        assert!(b.take().is_empty());
        assert_eq!(
            st.notice(Instant::now()),
            Some(validation_text(ValidationError::InvalidCharacters))
        );
        st.number.clear();
        run(
            &mut st,
            &b,
            &[
                Action::Key('1'),
                Action::Key('x'),
                Action::Key('0'),
                Action::Plus,
                Action::Backspace,
            ],
        );
        run(
            &mut st,
            &b,
            &[Action::Key('0'), Action::Key('2'), Action::Call],
        );
        assert_eq!(st.number, "1002");
        assert_eq!(b.take(), ["Dial(1002)"]);
    }

    #[test]
    fn keypad_sends_dtmf_only_when_established() {
        let (mut st, b) = (UiState::new(UiSettings::default()), Fake::default());
        let now = Instant::now();
        st.apply_event(
            Event::Call(snap(7, Direction::Outgoing, CallPhase::Ringing, 1)),
            now,
        );
        run(&mut st, &b, &[Action::Key('5')]);
        assert!(b.take().is_empty());
        assert_eq!(st.number, "5");
        st.apply_event(
            Event::Call(snap(7, Direction::Outgoing, CallPhase::Established, 2)),
            now,
        );
        run(&mut st, &b, &[Action::Key('9'), Action::Key('#')]);
        assert_eq!(b.take(), ["Dtmf(7, 9)", "Dtmf(7, #)"]);
        assert_eq!(st.number, "5");
        st.apply_event(
            Event::Call(snap(7, Direction::Outgoing, CallPhase::Held, 3)),
            now,
        );
        run(&mut st, &b, &[Action::Key('1')]);
        assert!(b.take().is_empty());
    }

    #[test]
    fn incoming_banner_answer_and_reject() {
        let (mut st, b) = (UiState::new(UiSettings::default()), Fake::default());
        st.apply_event(
            Event::Call(snap(3, Direction::Incoming, CallPhase::Ringing, 1)),
            Instant::now(),
        );
        assert_eq!((st.call_label(), st.end_label()), ("Answer", "Reject"));
        assert!(st.can_call().is_ok());
        run(&mut st, &b, &[Action::Call, Action::End]);
        assert_eq!(b.take(), ["Answer(CallId(3))", "Reject(CallId(3))"]);
    }

    #[test]
    fn hold_mute_follow_snapshots_not_clicks() {
        let (mut st, b) = (UiState::new(UiSettings::default()), Fake::default());
        let now = Instant::now();
        run(&mut st, &b, &[Action::ToggleMute, Action::ToggleHold]);
        assert!(b.take().is_empty());
        st.apply_event(
            Event::Call(snap(1, Direction::Outgoing, CallPhase::Established, 1)),
            now,
        );
        run(&mut st, &b, &[Action::ToggleMute, Action::ToggleHold]);
        assert_eq!(b.take(), ["SetMute(CallId(1), true)", "Hold(CallId(1))"]);
        let mut held = snap(1, Direction::Outgoing, CallPhase::Held, 2);
        held.muted = true;
        st.apply_event(Event::Call(held), now);
        run(
            &mut st,
            &b,
            &[Action::ToggleMute, Action::ToggleHold, Action::End],
        );
        assert_eq!(
            b.take(),
            [
                "SetMute(CallId(1), false)",
                "Resume(CallId(1))",
                "Hangup(CallId(1))"
            ]
        );
    }

    #[test]
    fn dnd_and_transfer_prompt() {
        let (mut st, b) = (UiState::new(UiSettings::default()), Fake::default());
        run(&mut st, &b, &[Action::ToggleDnd, Action::OpenTransfer]);
        assert!(st.dnd && st.transfer_target.is_none());
        assert_eq!(b.take(), ["SetDnd(true)"]);
        st.apply_event(
            Event::Call(snap(4, Direction::Outgoing, CallPhase::Established, 1)),
            Instant::now(),
        );
        run(&mut st, &b, &[Action::OpenTransfer]);
        st.transfer_target = Some("bad target".into());
        run(&mut st, &b, &[Action::ConfirmTransfer]);
        assert!(b.take().is_empty() && st.transfer_target.is_some());
        st.transfer_target = Some("1003".into());
        run(&mut st, &b, &[Action::ConfirmTransfer]);
        assert_eq!(b.take(), ["Transfer(4, 1003)"]);
        assert!(st.transfer_target.is_none());
    }

    #[test]
    fn engine_failure_is_sticky_in_the_status() {
        let mut st = UiState::new(UiSettings::default());
        st.apply_event(
            Event::Registration(RegistrationState::Registered),
            Instant::now(),
        );
        assert!(!st.engine_failed);
        let b = Fake {
            fail: Some(Notice::EngineFailed),
            ..Fake::default()
        };
        run(&mut st, &b, &[Action::ToggleDnd]);
        assert!(st.engine_failed && !st.dnd);
        st.apply_event(Event::Notice(Notice::Busy), Instant::now());
        assert!(st.engine_failed, "later notices do not clear the failure");
        let mut quit = UiState::new(UiSettings::default());
        quit.apply_event(Event::Stopped, Instant::now());
        quit.apply_event(Event::Notice(Notice::EngineFailed), Instant::now());
        assert!(
            !quit.engine_failed,
            "a closed queue after a normal stop is expected"
        );
    }

    #[test]
    fn overloaded_backend_shows_notice_and_keeps_dnd() {
        let mut st = UiState::new(UiSettings::default());
        let b = Fake {
            fail: Some(Notice::Overloaded),
            ..Fake::default()
        };
        run(&mut st, &b, &[Action::ToggleDnd]);
        assert!(!st.dnd);
        assert_eq!(
            st.notice(Instant::now()),
            Some(notice_text(Notice::Overloaded))
        );
    }

    #[test]
    fn settings_validation_save_and_register() {
        let (mut st, b) = (UiState::new(UiSettings::default()), Fake::default());
        let saved = RefCell::new(Vec::new());
        let mut save = |s: &UiSettings, _: Option<&mut Store>| -> Result<(), String> {
            saved.borrow_mut().push(s.clone());
            Ok(())
        };
        let now = Instant::now();
        st.dispatch(Action::OpenSettings, &b, &mut save, now);
        // Devices are listed by a job, never on the UI thread.
        assert!(st.audio_devices_loading && st.audio_devices.is_none());
        st.apply_job(
            JobResult::AudioDevices(Some(AudioDevices::default())),
            &b,
            now,
        );
        assert!(!st.audio_devices_loading && st.audio_devices.is_some());
        st.form.username = "1001".into();
        st.form.domain = "pbx.invalid".into();
        st.form.server = "192.0.2.1".into();
        st.form.media = Some(MediaSecurity::Srtp);
        st.dispatch(Action::SaveSettings, &b, &mut save, now);
        assert_eq!(st.notice(now), Some("SRTP requires TLS signaling"));
        st.form.set_transport(Transport::Tls);
        assert_eq!(st.form.port, "5061");
        st.dispatch(Action::SaveSettings, &b, &mut save, now);
        assert_eq!(st.notice(now), Some("Password is required"));
        assert!(saved.borrow().is_empty() && b.take().is_empty());
        st.form.password = "s3cret-marker".into();
        st.dispatch(Action::SaveSettings, &b, &mut save, now);
        assert!(st.form.password.is_empty() && st.view == View::Main);
        let sent = b.take();
        assert_eq!(sent.len(), 1);
        assert!(sent[0].starts_with("Register(") && !sent[0].contains("s3cret-marker"));
        assert_eq!(saved.borrow().len(), 1);
        assert_eq!(st.account_text(), "1001@pbx.invalid");
        // Re-saving without retyping reuses the session password.
        st.dispatch(Action::OpenSettings, &b, &mut save, now);
        st.dispatch(Action::SaveSettings, &b, &mut save, now);
        assert_eq!(b.take().len(), 1);
        st.dispatch(Action::SetTheme(Theme::Dark), &b, &mut save, now);
        assert_eq!(saved.borrow().last().map(|s| s.theme), Some(Theme::Dark));
    }

    #[test]
    fn events_dedup_by_version_and_final_moves_to_history() {
        let mut st = stored();
        let t0 = Instant::now();
        st.apply_event(Event::Registration(RegistrationState::Registered), t0);
        assert_eq!(registration_text(st.registration), "Registered");
        st.apply_event(
            Event::Call(snap(9, Direction::Outgoing, CallPhase::Established, 2)),
            t0,
        );
        // Stale and duplicate snapshots are ignored.
        st.apply_event(
            Event::Call(snap(9, Direction::Outgoing, CallPhase::Dialing, 1)),
            t0,
        );
        st.apply_event(
            Event::Call(snap(9, Direction::Outgoing, CallPhase::Held, 2)),
            t0,
        );
        assert_eq!(
            st.call.as_ref().map(|c| c.snapshot.phase),
            Some(CallPhase::Established)
        );
        // A second call does not replace the current one.
        st.apply_event(
            Event::Call(snap(10, Direction::Incoming, CallPhase::Ringing, 1)),
            t0,
        );
        assert_eq!(st.call.as_ref().map(|c| c.snapshot.id), Some(CallId(9)));
        let t1 = t0 + Duration::from_secs(65);
        assert_eq!(
            st.call
                .as_ref()
                .and_then(|c| c.duration(t1))
                .map(format_duration),
            Some("1:05".into())
        );
        st.apply_event(
            Event::Call(snap(
                9,
                Direction::Outgoing,
                CallPhase::Ended(EndReason::Normal),
                3,
            )),
            t1,
        );
        assert!(st.call.is_none());
        assert_eq!(st.history[0].call.duration_ms, Some(65_000));
        assert_eq!(st.history[0].call.outcome, CallOutcome::Answered);
        st.apply_event(
            Event::Call(snap(
                9,
                Direction::Outgoing,
                CallPhase::Ended(EndReason::Normal),
                3,
            )),
            t1,
        );
        assert_eq!(st.history.len(), 1);
        let b = Fake::default();
        st.tab = Tab::Calls;
        let remote = st.history[0].call.remote.clone();
        run(&mut st, &b, &[Action::FillNumber(remote)]);
        assert_eq!((st.tab, st.number.as_str()), (Tab::Phone, "1002"));
        st.apply_event(
            Event::Registration(RegistrationState::Failed(RegistrationFailure::Rejected(
                403,
            ))),
            t1,
        );
        assert_eq!(registration_text(st.registration), "Failed: rejected (403)");
        assert!(st.notice(t1 + NOTICE_TTL).is_none());
    }

    fn established(st: &mut UiState, id: u64) {
        st.apply_event(
            Event::Call(snap(id, Direction::Outgoing, CallPhase::Established, 1)),
            Instant::now(),
        );
    }

    #[test]
    fn quitting_during_a_call_asks_first_and_signals_do_not() {
        let (mut st, b) = (UiState::new(UiSettings::default()), Fake::default());
        established(&mut st, 1);
        run(&mut st, &b, &[Action::Quit]);
        assert_eq!(st.confirm, Some(Confirm::Quit));
        assert_eq!(Confirm::Quit.question(), "Hang up and quit?");
        assert!(!st.quitting && b.take().is_empty());
        run(&mut st, &b, &[Action::CancelConfirm]);
        assert!(!st.quitting && st.call.is_some());
        run(&mut st, &b, &[Action::Quit, Action::Confirm]);
        assert!(st.quitting && st.confirm.is_none());
        assert_eq!(b.take(), ["Shutdown"]);
        // SIGTERM path: no prompt.
        let (mut st, b) = (UiState::new(UiSettings::default()), Fake::default());
        established(&mut st, 2);
        run(&mut st, &b, &[Action::QuitNow]);
        assert!(st.quitting && st.confirm.is_none());
        assert_eq!(b.take(), ["Shutdown"]);
        // Without a call, quitting is immediate.
        let (mut st, b) = (UiState::new(UiSettings::default()), Fake::default());
        run(&mut st, &b, &[Action::Quit, Action::Quit]);
        assert!(st.quitting);
        assert_eq!(b.take(), ["Shutdown"]);
    }

    const REF: &str = "1001@pbx.invalid;server=192.0.2.1:5060;transport=udp";

    fn fill_account(st: &mut UiState) {
        st.form.username = "1001".into();
        st.form.domain = "pbx.invalid".into();
        st.form.server = "192.0.2.1".into();
    }

    #[test]
    fn remembered_password_goes_to_the_keyring_off_the_ui_thread() {
        let mut st = stored();
        let b = Fake::default();
        let now = Instant::now();
        run(&mut st, &b, &[Action::OpenSettings]);
        assert!(!st.form.remember);
        fill_account(&mut st);
        st.form.password = "s3cret-marker".into();
        st.form.remember = true;
        run(&mut st, &b, &[Action::SaveSettings]);
        // Registration does not wait for the keyring.
        assert_eq!(b.take().len(), 1);
        let jobs = b.jobs.take();
        assert_eq!(jobs.len(), 1);
        assert!(jobs[0].starts_with("StorePassword") && jobs[0].contains("1001@pbx.invalid"));
        assert!(!jobs[0].contains("s3cret-marker"));
        assert_eq!(st.secret_ref(), None, "only after the keyring confirms");
        // A failure keeps the session password and says why.
        st.apply_job(
            JobResult::PasswordStored {
                reference: REF.into(),
                result: Err("the keyring is locked".into()),
            },
            &b,
            now,
        );
        assert_eq!(st.secret_ref(), None);
        let notice = st.notice(now).unwrap_or_default().to_owned();
        assert!(notice.contains("locked") && notice.contains("session only"));
        // Saving again (same password) retries, and success records the reference.
        run(&mut st, &b, &[Action::OpenSettings]);
        st.form.remember = true;
        run(&mut st, &b, &[Action::SaveSettings]);
        assert_eq!(b.jobs.take().len(), 1);
        st.apply_job(
            JobResult::PasswordStored {
                reference: REF.into(),
                result: Ok(()),
            },
            &b,
            now,
        );
        assert_eq!(st.secret_ref().as_deref(), Some(REF));
        run(&mut st, &b, &[Action::OpenSettings]);
        assert!(st.form.remember);
        // Unchanged password and reference: no new keyring write.
        run(&mut st, &b, &[Action::SaveSettings]);
        assert!(b.jobs.take().is_empty());
        // Unchecking clears the reference now and deletes the item.
        run(&mut st, &b, &[Action::OpenSettings]);
        st.form.remember = false;
        run(&mut st, &b, &[Action::SaveSettings]);
        assert_eq!(st.secret_ref(), None);
        let jobs = b.jobs.take();
        assert_eq!(jobs.len(), 1);
        assert!(jobs[0].starts_with("DeletePassword") && jobs[0].contains("1001@pbx.invalid"));
        assert!(b.take().iter().all(|c| !c.contains("s3cret-marker")));
    }

    #[test]
    fn a_late_store_result_does_not_override_unchecking() {
        let mut st = stored();
        let b = Fake::default();
        run(&mut st, &b, &[Action::OpenSettings]);
        fill_account(&mut st);
        st.form.password = "pw".into();
        st.form.remember = true;
        run(&mut st, &b, &[Action::SaveSettings, Action::OpenSettings]);
        st.form.remember = false;
        run(&mut st, &b, &[Action::SaveSettings]);
        let jobs = b.jobs.take();
        assert!(jobs[0].starts_with("StorePassword") && jobs[1].starts_with("DeletePassword"));
        st.apply_job(
            JobResult::PasswordStored {
                reference: REF.into(),
                result: Ok(()),
            },
            &b,
            Instant::now(),
        );
        assert_eq!(st.secret_ref(), None);
    }

    #[test]
    fn startup_password_from_the_keyring_registers_the_matching_account() {
        let account = AccountSettings {
            display_name: String::new(),
            username: "1001".into(),
            auth_username: None,
            domain: "pbx.invalid".into(),
            server: "192.0.2.1".into(),
            port: 5060,
            transport: Transport::Udp,
            media: MediaSecurity::PlainRtp,
            expires: 300,
        };
        let mut st = UiState::new(UiSettings {
            account: Some(account),
            ..UiSettings::default()
        });
        let b = Fake::default();
        let now = Instant::now();
        let loaded = |reference: &str| JobResult::PasswordLoaded {
            reference: reference.into(),
            result: Ok(Some(SecretString::from("pw"))),
        };
        st.apply_job(loaded(&REF.replace("1001@", "1002@")), &b, now);
        assert!(b.take().is_empty(), "another account's item is not used");
        // An old-style reference or another server never gets the password.
        st.apply_job(loaded("1001@pbx.invalid"), &b, now);
        st.apply_job(loaded(&REF.replace("192.0.2.1", "192.0.2.9")), &b, now);
        assert!(b.take().is_empty());
        st.apply_job(loaded(REF), &b, now);
        let sent = b.take();
        assert!(sent.len() == 1 && sent[0].starts_with("Register("));
        // Only once: the session password now exists.
        st.apply_job(loaded(REF), &b, now);
        assert!(b.take().is_empty());
        let mut st = UiState::new(UiSettings::default());
        st.apply_job(
            JobResult::PasswordLoaded {
                reference: REF.into(),
                result: Err("Secret Service is unavailable".into()),
            },
            &b,
            now,
        );
        assert!(
            st.notice(now)
                .is_some_and(|n| n.contains("enter it in Settings"))
        );
    }

    #[test]
    fn audio_test_is_bounded_and_refused_during_a_call() {
        let (mut st, b) = (UiState::new(UiSettings::default()), Fake::default());
        let now = Instant::now();
        st.audio_output = Some("alsa:hw:2".into());
        run(&mut st, &b, &[Action::TestAudio]);
        assert_eq!(st.audio_test, Some(0));
        let jobs = b.jobs.take();
        assert!(jobs.len() == 1 && jobs[0].contains("alsa:hw:2"));
        run(&mut st, &b, &[Action::TestAudio]);
        assert!(b.jobs.take().is_empty(), "one test at a time");
        st.apply_job(
            JobResult::AudioLevel {
                generation: 1,
                peak: 1234,
            },
            &b,
            now,
        );
        assert_eq!(st.audio_test, Some(1234));
        st.apply_job(
            JobResult::AudioTestDone {
                generation: 1,
                result: Err("no matching audio output device".into()),
            },
            &b,
            now,
        );
        assert_eq!(st.audio_test, None);
        assert!(
            st.notice(now)
                .is_some_and(|n| n.contains("no matching audio output"))
        );
        // A second run: late results of the first never reset it.
        run(&mut st, &b, &[Action::TestAudio]);
        assert!(b.jobs.take()[0].contains("generation: 2"));
        for late in [
            JobResult::AudioLevel {
                generation: 1,
                peak: 99,
            },
            JobResult::AudioTestDone {
                generation: 1,
                result: Ok(()),
            },
        ] {
            st.apply_job(late, &b, now);
        }
        assert_eq!(st.audio_test, Some(0));
        st.apply_job(
            JobResult::AudioTestDone {
                generation: 2,
                result: Ok(()),
            },
            &b,
            now,
        );
        assert_eq!(st.audio_test, None);
        established(&mut st, 3);
        assert_eq!(
            st.can_test_audio(),
            Err("The call is using the audio devices")
        );
        run(&mut st, &b, &[Action::TestAudio]);
        assert!(b.jobs.take().is_empty() && st.audio_test.is_none());
    }

    #[test]
    fn calls_left_open_by_a_killed_process_are_recovered() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir =
            std::env::temp_dir().join(format!("rust-microsip-ui-journal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("db.sqlite3");
        {
            let mut st = UiState::new(UiSettings::default());
            st.attach_store(Some(Store::open(&path)?), None, true, Instant::now());
            established(&mut st, 1);
            // Not listed while it runs.
            assert!(st.history.is_empty());
            // Dropped mid-call: the process was killed.
        }
        // Without single-instance protection another window may own the
        // open call: nothing is recovered.
        let mut st = UiState::new(UiSettings::default());
        st.attach_store(Some(Store::open(&path)?), None, false, Instant::now());
        assert!(st.history.is_empty());
        let mut st = UiState::new(UiSettings::default());
        let now = Instant::now();
        st.attach_store(Some(Store::open(&path)?), None, true, now);
        assert_eq!(st.history.len(), 1);
        assert_eq!(st.history[0].call.outcome, CallOutcome::Interrupted);
        assert!(
            st.notice(now)
                .is_some_and(|n| n.starts_with("1 call(s) interrupted"))
        );
        // A normally ended call is recorded once, not as interrupted.
        end(&mut st, 2, Direction::Outgoing, true, EndReason::Normal);
        assert_eq!(st.history.len(), 2);
        assert_eq!(st.history[0].call.outcome, CallOutcome::Answered);
        // Close the store first: Windows refuses to delete an open file.
        drop(st);
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn link_handler_entry_is_opt_in_private_to_the_user_and_removable() -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let dir =
            std::env::temp_dir().join(format!("rust-microsip-ui-desktop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let entry = dir.join("applications").join(DESKTOP_FILE);
        let (mut st, b) = (
            UiState::new(UiSettings {
                desktop_entry: Some(entry.clone()),
                ..UiSettings::default()
            }),
            Fake::default(),
        );
        assert!(!entry.exists(), "nothing is installed without the action");
        run(&mut st, &b, &[Action::InstallLinkHandler]);
        let written = std::fs::read_to_string(&entry)?;
        let exe = std::env::current_exe()?;
        if !exe
            .to_string_lossy()
            .contains(['"', '`', '$', '\\', '%', '='])
        {
            assert!(written.contains(&format!("Exec=\"{}\" %u", exe.display())));
        }
        assert_eq!(
            std::fs::metadata(&entry)?.permissions().mode() & 0o777,
            0o644
        );
        let icon = dir.join("icons/hicolor/256x256/apps/rust-microsip.png");
        assert_eq!(
            std::fs::read(&icon)?,
            super::super::ICON,
            "icon beside the entry"
        );
        // Reinstalling replaces it atomically and leaves no temporary file.
        run(&mut st, &b, &[Action::InstallLinkHandler]);
        assert_eq!(std::fs::read_dir(dir.join("applications"))?.count(), 1);
        assert_eq!(std::fs::read_dir(icon.parent().unwrap_or(&dir))?.count(), 1);
        // An entry that differs from ours (edited or foreign) is kept.
        let edited = rust_microsip_platform::DESKTOP_ENTRY.replace("%u", "--bogus %u");
        std::fs::write(&entry, &edited)?;
        run(&mut st, &b, &[Action::RemoveLinkHandler]);
        assert_eq!(std::fs::read_to_string(&entry)?, edited);
        assert!(icon.exists(), "a kept entry keeps its icon");
        assert!(
            st.notice(Instant::now())
                .is_some_and(|n| n.contains("left in place"))
        );
        // An entry pointing at another program is someone's edit: kept.
        let other = rust_microsip_platform::desktop_entry(Some(Path::new("/opt/other/app")));
        std::fs::write(&entry, &other)?;
        run(&mut st, &b, &[Action::RemoveLinkHandler]);
        assert_eq!(std::fs::read_to_string(&entry)?, other);
        assert!(icon.exists());
        // An icon left without its entry is still cleaned up.
        std::fs::remove_file(&entry)?;
        run(&mut st, &b, &[Action::RemoveLinkHandler]);
        assert!(!icon.exists());
        run(&mut st, &b, &[Action::InstallLinkHandler]);
        run(&mut st, &b, &[Action::RemoveLinkHandler]);
        assert!(!entry.exists());
        assert!(!icon.exists(), "our icon is removed with the entry");
        run(&mut st, &b, &[Action::RemoveLinkHandler]);
        assert_eq!(
            st.notice(Instant::now()),
            Some("The link handler entry is not installed")
        );
        assert!(b.take().is_empty());
        std::fs::remove_dir_all(dir)
    }

    #[test]
    fn switching_theme_during_a_call_leaves_the_call_alone() {
        let mut st = UiState::new(UiSettings::default());
        let b = Fake::default();
        st.apply_event(
            Event::Call(snap(7, Direction::Outgoing, CallPhase::Established, 1)),
            Instant::now(),
        );
        for theme in [Theme::Dark, Theme::Light, Theme::System] {
            run(&mut st, &b, &[Action::SetTheme(theme)]);
            assert_eq!(st.theme, theme);
        }
        assert!(
            b.take().is_empty(),
            "a theme change sends no engine command"
        );
        assert_eq!(st.established_call(), Some(CallId(7)));
    }

    #[test]
    fn saves_receive_the_open_store() {
        let mut st = stored();
        let b = Fake::default();
        let mut seen = false;
        let mut save = |_: &UiSettings, store: Option<&mut Store>| -> Result<(), String> {
            seen = store.is_some();
            Err("`ui.theme` is set by toml; change it there (nothing was saved)".into())
        };
        let now = Instant::now();
        st.dispatch(Action::SetTheme(Theme::Dark), &b, &mut save, now);
        assert!(seen);
        assert!(st.notice(now).is_some_and(|n| n.contains("set by toml")));
    }
}
