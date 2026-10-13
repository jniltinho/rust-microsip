//! Pure presentation state and reducer for the softphone window.
//!
//! Nothing here touches egui or the GPU. User intents arrive as [`Action`]s,
//! engine output as [`Event`]s; commands leave through [`Backend`]. The UI
//! never claims a call or registration state before the engine reports it:
//! mute, hold and transfer toggles reflect the latest [`CallSnapshot`].
//!
//! History and contacts live in the SQLite [`Store`]. Its work runs
//! synchronously on the UI thread, but only on discrete triggers (a final call
//! snapshot, a click, a search edit), never per frame; each operation is a
//! small indexed query bounded by a row limit. The lists shown are cached here.

use super::Backend;
use rust_microsip_audio::{AudioDevices, DeviceInfo};
use rust_microsip_core::{
    AccountSettings, CallId, CallPhase, CallSnapshot, Command, DialTarget, Direction, DtmfKey,
    EndReason, Event, MediaSecurity, Notice, RegistrationFailure, RegistrationState, TransferState,
    Transport, ValidationError,
};
use rust_microsip_storage::{
    CSV_MAX_BYTES, CallOutcome, CallRecord, Contact, ContactNumber, CsvIssueKind, CsvPreview,
    HistoryEntry, HistoryFilter, MAX_REMOTE_LEN, NewContact, Store, preview_csv,
};
use secrecy::SecretString;
use secrecy::zeroize::Zeroize;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Maximum characters accepted in the number and transfer entries.
pub const MAX_NUMBER_CHARS: usize = 64;
/// How long a transient notice stays visible.
pub const NOTICE_TTL: Duration = Duration::from_secs(5);
/// Most recent history entries listed.
pub const HISTORY_LIMIT: u32 = 200;
/// Most contacts listed per search.
pub const CONTACTS_LIMIT: u32 = 500;
const DEFAULT_EXPIRES: u32 = 300;

/// Settings save callback: persists secret-free settings. The error string
/// must be a user-safe message without paths or secrets.
pub type SaveFn = dyn FnMut(&UiSettings) -> Result<(), String>;

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
}
impl Confirm {
    /// Question shown to the user.
    pub const fn question(self) -> &'static str {
        match self {
            Self::DeleteCall(_) => "Delete this history entry?",
            Self::ClearHistory => "Delete the whole call history?",
            Self::DeleteContact(_) => "Delete this contact?",
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
    /// Shut the engine down and close the window.
    Quit,
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
    password: Option<SecretString>,
    notice: Option<(String, Instant)>,
    versions: HashMap<CallId, u64>,
    times: HashMap<CallId, CallTimes>,
    store: Option<Store>,
    session: String,
}

impl UiState {
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
            audio_input: settings.audio_input,
            audio_output: settings.audio_output,
            storage_warning: None,
            dnd: false,
            theme: settings.theme,
            account: settings.account,
            quitting: false,
            stopped: false,
            password: None,
            notice: None,
            versions: HashMap::new(),
            times: HashMap::new(),
            store: None,
            // Unique per process start, so session keys never collide.
            session: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
                .to_string(),
        }
    }

    /// Attach the history/contacts store and load the lists. `warning`
    /// stays visible in the Calls and Contacts tabs (for example when the
    /// store is in memory only). Without a store nothing is recorded.
    pub fn attach_store(&mut self, store: Option<Store>, warning: Option<String>, now: Instant) {
        self.storage_warning = match (&store, warning) {
            (_, Some(text)) => Some(text),
            (None, None) => Some("History and contacts are unavailable.".to_owned()),
            (Some(_), None) => None,
        };
        self.store = store;
        self.refresh_history(now);
        self.refresh_contacts(now);
    }

    fn settings(&self) -> UiSettings {
        UiSettings {
            account: self.account.clone(),
            theme: self.theme,
            audio_input: self.audio_input.clone(),
            audio_output: self.audio_output.clone(),
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
                self.set_notice(notice_text(notice), now);
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
        save: &mut dyn FnMut(&UiSettings) -> Result<(), String>,
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
            Action::Confirm => self.run_confirm(now),
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
                // ponytail: enumerated on the UI thread when settings open;
                // move to a worker if slow hosts make the click lag.
                self.audio_devices = backend.audio_devices();
                self.view = View::Settings;
            }
            Action::OpenAbout => {
                self.close_view();
                self.view = View::About;
            }
            Action::CloseView => self.close_view(),
            Action::SetTheme(theme) => {
                self.theme = theme;
                if let Err(message) = save(&self.settings()) {
                    self.set_notice(format!("Theme not saved: {message}"), now);
                }
            }
            Action::SaveSettings => self.save_settings(backend, save, now),
            Action::Unregister => {
                self.send(backend, Command::Unregister, now);
            }
            Action::Quit => {
                if !self.quitting {
                    self.quitting = true;
                    self.send(backend, Command::Shutdown, now);
                }
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

    fn save_audio(
        &mut self,
        save: &mut dyn FnMut(&UiSettings) -> Result<(), String>,
        now: Instant,
        revert: impl FnOnce(&mut Self),
    ) {
        match save(&self.settings()) {
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

    fn run_confirm(&mut self, now: Instant) {
        let (Some(confirm), Some(store)) = (self.confirm.take(), &mut self.store) else {
            return;
        };
        let result = match confirm {
            Confirm::DeleteCall(id) => store.delete_call(id),
            Confirm::ClearHistory => store.clear_history().map(|_| ()),
            Confirm::DeleteContact(id) => store.delete_contact(id),
        };
        if let Err(error) = result {
            self.set_notice(format!("Not deleted: {error}"), now);
        }
        self.refresh_history(now);
        self.refresh_contacts(now);
    }

    fn save_settings(
        &mut self,
        backend: &dyn Backend,
        save: &mut dyn FnMut(&UiSettings) -> Result<(), String>,
        now: Instant,
    ) {
        let account = match self.form.to_account() {
            Ok(account) => account,
            Err(error) => return self.set_notice(error.message(), now),
        };
        let password = if self.form.password.is_empty() {
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
        if let Err(message) = save(&settings) {
            return self.set_notice(format!("Settings not saved: {message}"), now);
        }
        self.account = Some(account.clone());
        self.password = Some(password.clone());
        self.close_view();
        if self.send(backend, Command::Register(Box::new(account), password), now) {
            self.set_notice("Settings saved; registering", now);
        }
    }

    /// Apply one engine event.
    pub fn apply_event(&mut self, event: Event, now: Instant) {
        match event {
            Event::Registration(state) => self.registration = state,
            Event::Notice(notice) => self.set_notice(notice_text(notice), now),
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

    /// Record one final snapshot; the session key makes it idempotent.
    fn record_call(
        &mut self,
        snap: &CallSnapshot,
        reason: EndReason,
        times: CallTimes,
        now: Instant,
    ) {
        let Some(store) = &mut self.store else { return };
        let answered = times.answered;
        let record = CallRecord {
            session_key: format!("{}:{}", self.session, snap.id.0),
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
        match store.record_call(&record) {
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
    }
    impl Fake {
        fn take(&self) -> Vec<String> {
            self.sent.take()
        }
    }

    fn no_save() -> impl FnMut(&UiSettings) -> Result<(), String> {
        |_| Ok(())
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
        st.attach_store(Store::open_in_memory().ok(), None, Instant::now());
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
        let mut save = |s: &UiSettings| -> Result<(), String> {
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
        st.attach_store(None, None, Instant::now());
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
        let mut save = |s: &UiSettings| -> Result<(), String> {
            saved.borrow_mut().push(s.clone());
            Ok(())
        };
        let now = Instant::now();
        st.dispatch(Action::OpenSettings, &b, &mut save, now);
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
}
