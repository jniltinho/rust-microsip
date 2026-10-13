//! Domain contracts shared by the desktop UI, the telephony engine and audio.
//!
//! This crate holds no I/O: no sockets, devices, files or async runtime. The UI
//! sends [`Command`]s and renders [`Event`]s; the telephony engine owns dialogs
//! and media; audio adapters implement [`MediaIo`]. Secrets are carried only as
//! [`SecretString`] and never appear in `Debug` output.

use secrecy::SecretString;
use std::fmt;

/// SIP signaling transport for the single configured account.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Transport {
    /// Plain UDP signaling.
    Udp,
    /// Plain TCP signaling.
    Tcp,
    /// TLS signaling with certificate and host-name validation.
    Tls,
}
impl Transport {
    /// Lower-case label used in SIP URIs and configuration.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Udp => "udp",
            Self::Tcp => "tcp",
            Self::Tls => "tls",
        }
    }
    /// Default registrar port for this transport.
    pub const fn default_port(self) -> u16 {
        match self {
            Self::Tls => 5061,
            Self::Udp | Self::Tcp => 5060,
        }
    }
}

/// Media protection policy. SRTP is only valid with TLS signaling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MediaSecurity {
    /// Unencrypted RTP; the UI must show a persistent warning.
    PlainRtp,
    /// Mandatory SRTP with SDES keys; peers without SRTP are rejected.
    Srtp,
}

/// Validation failure for user-provided settings or dial strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    /// A required field is empty.
    #[error("a required field is empty")]
    Empty,
    /// A field exceeds its fixed length bound.
    #[error("a field is too long")]
    TooLong,
    /// A field contains characters outside its allowed set.
    #[error("a field contains unsupported characters")]
    InvalidCharacters,
    /// The port is zero.
    #[error("the port must be between 1 and 65535")]
    InvalidPort,
    /// SRTP was requested without TLS signaling.
    #[error("SRTP requires TLS signaling")]
    SrtpRequiresTls,
}

fn bounded(value: &str, max: usize, allowed: impl Fn(char) -> bool) -> Result<(), ValidationError> {
    if value.is_empty() {
        return Err(ValidationError::Empty);
    }
    if value.len() > max {
        return Err(ValidationError::TooLong);
    }
    if !value.chars().all(allowed) {
        return Err(ValidationError::InvalidCharacters);
    }
    Ok(())
}
fn user_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-_.!~*'()&=+$,;?/#".contains(c)
}
fn host_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-.:[]".contains(c)
}

/// Validated, secret-free account settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountSettings {
    /// Optional caller display name.
    pub display_name: String,
    /// SIP user part (extension or user name).
    pub username: String,
    /// Optional authentication user when it differs from `username`.
    pub auth_username: Option<String>,
    /// SIP domain used in the address of record.
    pub domain: String,
    /// Registrar/proxy host name or IP literal.
    pub server: String,
    /// Registrar/proxy port.
    pub port: u16,
    /// Signaling transport.
    pub transport: Transport,
    /// Media protection policy.
    pub media: MediaSecurity,
    /// Registration lifetime requested from the registrar, in seconds.
    pub expires: u32,
}
impl AccountSettings {
    /// Check bounds, character sets and the SRTP/TLS rule.
    ///
    /// # Errors
    /// Returns the first [`ValidationError`] found; values are never echoed.
    pub fn validate(&self) -> Result<(), ValidationError> {
        bounded(&self.username, 64, user_char)?;
        if let Some(auth) = &self.auth_username {
            bounded(auth, 64, user_char)?;
        }
        bounded(&self.domain, 253, host_char)?;
        bounded(&self.server, 253, host_char)?;
        if self.display_name.len() > 64
            || self
                .display_name
                .chars()
                .any(|c| c.is_control() || matches!(c, '"' | '\\'))
        {
            return Err(ValidationError::InvalidCharacters);
        }
        if self.port == 0 {
            return Err(ValidationError::InvalidPort);
        }
        if self.media == MediaSecurity::Srtp && self.transport != Transport::Tls {
            return Err(ValidationError::SrtpRequiresTls);
        }
        Ok(())
    }
}

/// A validated dial target: a SIP user part such as `1002`, `*97` or `+5511…`.
#[derive(Clone, PartialEq, Eq)]
pub struct DialTarget(String);
impl DialTarget {
    /// Validate a dial string typed by the user (at most 64 characters).
    ///
    /// # Errors
    /// Returns [`ValidationError`] for empty, oversized or unsupported input.
    pub fn parse(value: &str) -> Result<Self, ValidationError> {
        let value = value.trim();
        bounded(value, 64, |c| {
            c.is_ascii_alphanumeric() || "*#+-_.".contains(c)
        })?;
        if value[1..].contains('+') {
            return Err(ValidationError::InvalidCharacters);
        }
        Ok(Self(value.to_owned()))
    }
    /// The validated user part.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for DialTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DialTarget(..)")
    }
}
impl fmt::Display for DialTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One DTMF key accepted during a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DtmfKey(char);
impl DtmfKey {
    /// Accept `0-9`, `*`, `#` and `A-D`.
    ///
    /// # Errors
    /// Returns [`ValidationError::InvalidCharacters`] for any other key.
    pub fn new(key: char) -> Result<Self, ValidationError> {
        let key = key.to_ascii_uppercase();
        if key.is_ascii_digit() || matches!(key, '*' | '#' | 'A'..='D') {
            Ok(Self(key))
        } else {
            Err(ValidationError::InvalidCharacters)
        }
    }
    /// RFC 4733 event number (0-15).
    pub const fn event(self) -> u8 {
        match self.0 {
            '*' => 10,
            '#' => 11,
            'A' => 12,
            'B' => 13,
            'C' => 14,
            'D' => 15,
            digit => digit as u8 - b'0',
        }
    }
    /// The key as typed.
    pub const fn as_char(self) -> char {
        self.0
    }
}

/// Why registration is not active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistrationFailure {
    /// The registrar rejected the credentials.
    Unauthorized,
    /// The registrar or network did not respond in time.
    Timeout,
    /// TLS certificate or host-name validation failed.
    Certificate,
    /// Name resolution or connection failed.
    Network,
    /// The registrar answered with another failure status.
    Rejected(u16),
}

/// Account registration state as shown in the status bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistrationState {
    /// No account is configured or it was disabled.
    Disabled,
    /// A REGISTER transaction is in progress.
    Registering,
    /// The registrar accepted the binding.
    Registered,
    /// Registration failed; the engine retries with bounded backoff.
    Failed(RegistrationFailure),
}

/// Stable identifier of one call within an engine session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CallId(pub u64);

/// Who initiated the call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Dialed from this application.
    Outgoing,
    /// Offered by the registrar to this application.
    Incoming,
}

/// Why a call ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndReason {
    /// Either side hung up an established call.
    Normal,
    /// The local user canceled before answer.
    Canceled,
    /// The local user rejected an incoming call.
    Rejected,
    /// The remote side was busy.
    Busy,
    /// The remote side did not answer.
    NoAnswer,
    /// The call was transferred away.
    Transferred,
    /// Negotiation failed (for example, no common codec or no SRTP).
    NotAcceptable,
    /// Signaling or media failed.
    Failed,
}

/// Call lifecycle phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallPhase {
    /// Outgoing INVITE sent, no ringing yet.
    Dialing,
    /// Remote side is alerting (outgoing) or waiting for local answer (incoming).
    Ringing,
    /// Media is flowing both ways.
    Established,
    /// Locally held; no audio is sent or played.
    Held,
    /// BYE/CANCEL in progress.
    Terminating,
    /// Final state; no further transitions.
    Ended(EndReason),
}
impl CallPhase {
    /// Whether moving to `next` is a valid lifecycle transition.
    pub fn can_transition_to(self, next: Self) -> bool {
        use CallPhase::*;
        match (self, next) {
            (Ended(_), _) => false,
            (_, Ended(_)) => true,
            (Dialing, Ringing | Established | Terminating) => true,
            (Ringing, Established | Terminating) => true,
            (Established, Held | Terminating) => true,
            (Held, Established | Terminating) => true,
            _ => false,
        }
    }
    /// True for the final phase.
    pub const fn is_final(self) -> bool {
        matches!(self, Self::Ended(_))
    }
}

/// Progress of a blind transfer, confirmed only by NOTIFY.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferState {
    /// REFER sent, waiting for acceptance and final NOTIFY.
    Pending,
    /// The transfer target answered (final 2xx sipfrag).
    Succeeded,
    /// The transfer failed with this final status; the call is kept.
    Failed(u16),
}

/// Immutable view of one call for rendering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallSnapshot {
    /// Engine-assigned identifier.
    pub id: CallId,
    /// Origination side.
    pub direction: Direction,
    /// Remote party user part or display text (already sanitized).
    pub remote: String,
    /// Current phase.
    pub phase: CallPhase,
    /// Local microphone muted.
    pub muted: bool,
    /// Media is SRTP-protected.
    pub secure: bool,
    /// Last blind-transfer attempt.
    pub transfer: Option<TransferState>,
    /// Monotonic version for deduplication; increases with every change.
    pub version: u64,
}

/// Request from the UI (or a future control client) to the engine.
pub enum Command {
    /// Apply account settings and register with the given password.
    Register(Box<AccountSettings>, SecretString),
    /// Remove the registration binding.
    Unregister,
    /// Place an outgoing call.
    Dial(DialTarget),
    /// Answer an incoming call.
    Answer(CallId),
    /// Reject an incoming call (486 Busy Here).
    Reject(CallId),
    /// Cancel, reject or hang up any call.
    Hangup(CallId),
    /// Send one RFC 4733 DTMF key.
    Dtmf(CallId, DtmfKey),
    /// Put the call on hold.
    Hold(CallId),
    /// Resume a held call.
    Resume(CallId),
    /// Blind-transfer the remote party.
    Transfer(CallId, DialTarget),
    /// Mute or unmute the microphone for this call.
    SetMute(CallId, bool),
    /// Do-not-disturb: answer new incoming calls with 486.
    SetDnd(bool),
    /// End all calls, unregister and stop within a bounded time.
    Shutdown,
}
impl fmt::Debug for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Register(settings, _) => f
                .debug_tuple("Register")
                .field(settings)
                .field(&"[REDACTED]")
                .finish(),
            Self::Unregister => f.write_str("Unregister"),
            Self::Dial(_) => f.write_str("Dial(..)"),
            Self::Answer(id) => f.debug_tuple("Answer").field(id).finish(),
            Self::Reject(id) => f.debug_tuple("Reject").field(id).finish(),
            Self::Hangup(id) => f.debug_tuple("Hangup").field(id).finish(),
            Self::Dtmf(id, _) => f.debug_tuple("Dtmf").field(id).finish(),
            Self::Hold(id) => f.debug_tuple("Hold").field(id).finish(),
            Self::Resume(id) => f.debug_tuple("Resume").field(id).finish(),
            Self::Transfer(id, _) => f.debug_tuple("Transfer").field(id).finish(),
            Self::SetMute(id, muted) => f.debug_tuple("SetMute").field(id).field(muted).finish(),
            Self::SetDnd(on) => f.debug_tuple("SetDnd").field(on).finish(),
            Self::Shutdown => f.write_str("Shutdown"),
        }
    }
}

/// User-visible, input-free notice about a rejected command or a fault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    /// A call is already active; this release supports one call at a time.
    Busy,
    /// No account is registered.
    NotRegistered,
    /// The referenced call no longer exists.
    UnknownCall,
    /// The command is invalid in the call's current phase.
    InvalidState,
    /// No audio device could be opened; calls continue without local audio.
    AudioUnavailable,
    /// The command queue was full; the command was dropped.
    Overloaded,
}

/// Engine output, delivered in order. Final call snapshots are never dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Registration state changed.
    Registration(RegistrationState),
    /// A call was created or changed.
    Call(CallSnapshot),
    /// A command was refused or a fault occurred.
    Notice(Notice),
    /// The engine stopped after [`Command::Shutdown`].
    Stopped,
}

/// Samples per 20 ms G.711 frame at 8 kHz.
pub const FRAME_SAMPLES: usize = 160;

/// Real-time audio exchange between the engine and a device adapter.
///
/// Implementations must be non-blocking: both calls run on the engine's media
/// timer, never inside a device callback.
pub trait MediaIo: Send {
    /// Fill `frame` with the next captured 8 kHz mono frame. Returns `false`
    /// (and leaves silence) when no captured audio is available.
    fn capture(&mut self, frame: &mut [i16; FRAME_SAMPLES]) -> bool;
    /// Queue one decoded 8 kHz mono frame for playback; drops on overflow.
    fn play(&mut self, frame: &[i16; FRAME_SAMPLES]);
}

/// Silent [`MediaIo`] used when no audio device is available, and in tests.
#[derive(Default)]
pub struct NullMedia;
impl MediaIo for NullMedia {
    fn capture(&mut self, frame: &mut [i16; FRAME_SAMPLES]) -> bool {
        frame.fill(0);
        false
    }
    fn play(&mut self, _: &[i16; FRAME_SAMPLES]) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account() -> AccountSettings {
        AccountSettings {
            display_name: "Lab".into(),
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
    fn account_validation_rejects_bad_fields_and_srtp_without_tls() {
        assert_eq!(account().validate(), Ok(()));
        let mut bad = account();
        bad.username.clear();
        assert_eq!(bad.validate(), Err(ValidationError::Empty));
        let mut bad = account();
        bad.server = "evil host".into();
        assert_eq!(bad.validate(), Err(ValidationError::InvalidCharacters));
        let mut bad = account();
        bad.port = 0;
        assert_eq!(bad.validate(), Err(ValidationError::InvalidPort));
        let mut bad = account();
        bad.media = MediaSecurity::Srtp;
        assert_eq!(bad.validate(), Err(ValidationError::SrtpRequiresTls));
        bad.transport = Transport::Tls;
        assert_eq!(bad.validate(), Ok(()));
        let mut bad = account();
        bad.display_name = "a\"b".into();
        assert!(bad.validate().is_err());
        bad.display_name = "abc\\".into();
        assert!(bad.validate().is_err());
    }

    #[test]
    fn dial_targets_are_bounded_and_never_debug_printed() {
        for ok in ["1002", "*97", "+5511999990000", "alice.b"] {
            assert!(DialTarget::parse(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "10 02",
            "sip:1002@x",
            "1+2",
            "a;b",
            &"9".repeat(65),
            "1002\r\nX",
        ] {
            assert!(DialTarget::parse(bad).is_err(), "{bad:?}");
        }
        let target = DialTarget::parse(" 1002 ").unwrap_or_else(|_| unreachable!());
        assert_eq!(target.as_str(), "1002");
        assert_eq!(format!("{target:?}"), "DialTarget(..)");
    }

    #[test]
    fn dtmf_keys_map_to_rfc4733_events() {
        let events: Vec<u8> = "0123456789*#abcd"
            .chars()
            .filter_map(|c| DtmfKey::new(c).ok())
            .map(DtmfKey::event)
            .collect();
        assert_eq!(events, (0..16).collect::<Vec<_>>());
        assert!(DtmfKey::new('e').is_err() && DtmfKey::new(' ').is_err());
    }

    #[test]
    fn call_phase_transitions_are_monotonic_and_final_is_terminal() {
        use CallPhase::*;
        let all = [
            Dialing,
            Ringing,
            Established,
            Held,
            Terminating,
            Ended(EndReason::Normal),
        ];
        for from in all {
            for to in all {
                let allowed = from.can_transition_to(to);
                if from.is_final() {
                    assert!(!allowed);
                }
                if to.is_final() && !from.is_final() {
                    assert!(allowed);
                }
            }
        }
        assert!(!Established.can_transition_to(Dialing));
        assert!(!Held.can_transition_to(Ringing));
        assert!(Held.can_transition_to(Established));
    }

    #[test]
    fn register_command_debug_redacts_password() {
        let command = Command::Register(Box::new(account()), SecretString::from("s3cret-marker"));
        let text = format!("{command:?}");
        assert!(text.contains("[REDACTED]") && !text.contains("s3cret-marker"));
    }

    #[test]
    fn null_media_reports_silence() {
        let mut media = NullMedia;
        let mut frame = [7; FRAME_SAMPLES];
        assert!(!media.capture(&mut frame));
        assert!(frame.iter().all(|s| *s == 0));
    }
}
