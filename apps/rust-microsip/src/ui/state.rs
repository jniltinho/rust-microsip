//! Pure presentation state and reducer for the softphone window.
//!
//! Nothing here touches egui or the GPU. User intents arrive as [`Action`]s,
//! engine output as [`Event`]s; commands leave through [`Backend`]. The UI
//! never claims a call or registration state before the engine reports it:
//! mute, hold and transfer toggles reflect the latest [`CallSnapshot`].

use super::Backend;
use rust_microsip_core::{
    AccountSettings, CallId, CallPhase, CallSnapshot, Command, DialTarget, Direction, DtmfKey,
    EndReason, Event, MediaSecurity, Notice, RegistrationFailure, RegistrationState, TransferState,
    Transport, ValidationError,
};
use secrecy::SecretString;
use secrecy::zeroize::Zeroize;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Maximum characters accepted in the number and transfer entries.
pub const MAX_NUMBER_CHARS: usize = 64;
/// How long a transient notice stays visible.
pub const NOTICE_TTL: Duration = Duration::from_secs(5);
// ponytail: fixed in-memory cap; persistent history arrives with storage.
const MAX_HISTORY: usize = 200;
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
    /// Session call history.
    Calls,
    /// Contacts placeholder.
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
}

/// One finished call of this run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    /// Who initiated the call.
    pub direction: Direction,
    /// Remote party as reported by the engine.
    pub remote: String,
    /// Why the call ended.
    pub outcome: EndReason,
    /// Time spent connected, if the call was ever established.
    pub duration: Option<Duration>,
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
    /// Copy a history entry's remote into the number entry.
    FillFromHistory(usize),
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
    /// Finished calls of this run, newest first.
    pub history: Vec<HistoryEntry>,
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
            dnd: false,
            theme: settings.theme,
            account: settings.account,
            quitting: false,
            stopped: false,
            password: None,
            notice: None,
            versions: HashMap::new(),
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
            Action::FillFromHistory(index) => {
                if let Some(entry) = self.history.get(index) {
                    self.number = entry.remote.chars().take(MAX_NUMBER_CHARS).collect();
                    self.tab = Tab::Phone;
                }
            }
            Action::OpenSettings => {
                self.form = SettingsForm::from_account(self.account.as_ref());
                self.view = View::Settings;
            }
            Action::OpenAbout => {
                self.close_view();
                self.view = View::About;
            }
            Action::CloseView => self.close_view(),
            Action::SetTheme(theme) => {
                self.theme = theme;
                let settings = UiSettings {
                    account: self.account.clone(),
                    theme,
                };
                if let Err(message) = save(&settings) {
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
        self.view = View::Main;
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
            theme: self.theme,
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
        if let CallPhase::Ended(outcome) = snapshot.phase {
            let mut duration = None;
            if is_current {
                duration = self.call.take().and_then(|c| c.duration(now));
                self.transfer_target = None;
            }
            self.history.insert(
                0,
                HistoryEntry {
                    direction: snapshot.direction,
                    remote: snapshot.remote,
                    outcome,
                    duration,
                },
            );
            self.history.truncate(MAX_HISTORY);
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
        let mut st = UiState::new(UiSettings::default());
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
        assert_eq!(st.history[0].duration, Some(Duration::from_secs(65)));
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
        run(&mut st, &b, &[Action::FillFromHistory(0)]);
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
