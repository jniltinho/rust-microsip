//! Local control contract, JSON v1 (AG-02, AG-03).
//!
//! The profile owner (window or headless service) serves this protocol on a
//! private Unix socket; the CLI and the MCP server are clients. One JSON
//! object per line, at most [`MAX_MESSAGE`] bytes. A connection starts with
//! [`Hello`] answered by [`Welcome`], then carries [`Request`]/[`Response`]
//! pairs in order. Unknown fields are rejected everywhere.
//!
//! This crate is pure: types, validation and framing over any
//! `Read`/`Write`. Sockets and peer credentials live in the platform crate;
//! policies, the ledger and execution live in the owner.

use rust_microsip_core::DialTarget;
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Read, Write};

/// Protocol name sent in [`Hello`] and [`Welcome`].
pub const PROTOCOL: &str = "rust-microsip-control";
/// Version of this contract.
pub const SCHEMA_VERSION: u32 = 1;
/// Largest message, including the newline.
pub const MAX_MESSAGE: usize = 1 << 20;
/// Page size limit for contacts and history.
pub const MAX_PAGE: u32 = 100;
/// Socket file name inside the private runtime directory.
pub const SOCKET_FILE: &str = "control.sock";

/// First message of a client.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    /// Must be [`PROTOCOL`].
    pub protocol: String,
    /// Must be [`SCHEMA_VERSION`].
    pub version: u32,
}
impl Hello {
    /// The hello of this version.
    #[must_use]
    pub fn current() -> Self {
        Self {
            protocol: PROTOCOL.to_owned(),
            version: SCHEMA_VERSION,
        }
    }
}

/// Which kind of process owns the profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerMode {
    /// The desktop window.
    Gui,
    /// The headless service.
    Service,
}

/// The owner's answer to [`Hello`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Welcome {
    /// [`PROTOCOL`].
    pub protocol: String,
    /// [`SCHEMA_VERSION`].
    pub version: u32,
    /// Owner kind.
    pub mode: OwnerMode,
    /// Owner session; changes on every owner start.
    pub epoch: u64,
}

/// Call-history filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryFilter {
    /// Every call.
    #[default]
    All,
    /// Incoming calls.
    Incoming,
    /// Outgoing calls.
    Outgoing,
    /// Missed calls.
    Missed,
}

/// Target of a call operation: the call and the snapshot version the client
/// saw, so an action never lands on a replacement call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallRef {
    /// Call identifier from `status_get`.
    pub call_id: u64,
    /// Snapshot version from `status_get`.
    pub expected_version: u64,
}

/// One operation. Reads have no effect; the others are mutations and need an
/// idempotency key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "op",
    content = "args",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Operation {
    /// Registration, call and DND summary (personal data masked by default).
    StatusGet,
    /// Audio devices.
    DevicesList,
    /// Contacts matching `query`.
    ContactsSearch {
        /// Literal text to match (may be empty).
        query: String,
        /// Opaque cursor from a previous page.
        cursor: Option<String>,
        /// Page size, at most [`MAX_PAGE`].
        limit: Option<u32>,
    },
    /// Call history.
    HistoryList {
        /// Filter.
        filter: Option<HistoryFilter>,
        /// Opaque cursor from a previous page.
        cursor: Option<String>,
        /// Page size, at most [`MAX_PAGE`].
        limit: Option<u32>,
    },
    /// Sanitized effective configuration (never secrets).
    ConfigShow,
    /// Outcome of an earlier mutation.
    OperationGet {
        /// Identifier returned when the mutation was accepted.
        operation_id: String,
    },
    /// Place a call.
    CallDial {
        /// Number or SIP user to call.
        target: String,
    },
    /// Answer a ringing incoming call.
    CallAnswer(CallRef),
    /// Reject a ringing incoming call.
    CallReject(CallRef),
    /// End a call.
    CallHangup(CallRef),
    /// Send DTMF digits.
    CallDtmf {
        /// Call identifier from `status_get`.
        call_id: u64,
        /// Snapshot version from `status_get`.
        expected_version: u64,
        /// Digits `0-9*#A-D`, at most 32.
        digits: String,
    },
    /// Mute or unmute the microphone.
    CallMute {
        /// Call identifier from `status_get`.
        call_id: u64,
        /// Snapshot version from `status_get`.
        expected_version: u64,
        /// Desired state.
        muted: bool,
    },
    /// Hold or resume.
    CallHold {
        /// Call identifier from `status_get`.
        call_id: u64,
        /// Snapshot version from `status_get`.
        expected_version: u64,
        /// Desired state.
        held: bool,
    },
    /// Blind transfer.
    CallTransfer {
        /// Call identifier from `status_get`.
        call_id: u64,
        /// Snapshot version from `status_get`.
        expected_version: u64,
        /// Transfer destination.
        target: String,
    },
    /// Do not disturb.
    DndSet {
        /// Desired state.
        enabled: bool,
    },
}
impl Operation {
    /// Stable name, also the MCP tool name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::StatusGet => "status_get",
            Self::DevicesList => "devices_list",
            Self::ContactsSearch { .. } => "contacts_search",
            Self::HistoryList { .. } => "history_list",
            Self::ConfigShow => "config_show",
            Self::OperationGet { .. } => "operation_get",
            Self::CallDial { .. } => "call_dial",
            Self::CallAnswer(_) => "call_answer",
            Self::CallReject(_) => "call_reject",
            Self::CallHangup(_) => "call_hangup",
            Self::CallDtmf { .. } => "call_dtmf",
            Self::CallMute { .. } => "call_mute",
            Self::CallHold { .. } => "call_hold",
            Self::CallTransfer { .. } => "call_transfer",
            Self::DndSet { .. } => "dnd_set",
        }
    }
    /// The call a session operation targets.
    #[must_use]
    pub const fn call_ref(&self) -> Option<CallRef> {
        match *self {
            Self::CallAnswer(call) | Self::CallReject(call) | Self::CallHangup(call) => Some(call),
            Self::CallDtmf {
                call_id,
                expected_version,
                ..
            }
            | Self::CallMute {
                call_id,
                expected_version,
                ..
            }
            | Self::CallHold {
                call_id,
                expected_version,
                ..
            }
            | Self::CallTransfer {
                call_id,
                expected_version,
                ..
            } => Some(CallRef {
                call_id,
                expected_version,
            }),
            _ => None,
        }
    }
    /// Whether the operation has external effects.
    #[must_use]
    pub const fn is_mutation(&self) -> bool {
        !matches!(
            self,
            Self::StatusGet
                | Self::DevicesList
                | Self::ContactsSearch { .. }
                | Self::HistoryList { .. }
                | Self::ConfigShow
                | Self::OperationGet { .. }
        )
    }
    /// Validate bounds and formats; errors never echo the input.
    ///
    /// # Errors
    /// [`ErrorCode::Validation`] with a field-level message.
    pub fn validate(&self) -> Result<(), ControlError> {
        let invalid = |message: &str| Err(ControlError::new(ErrorCode::Validation, message));
        let page = |limit: &Option<u32>, cursor: &Option<String>| {
            if limit.is_some_and(|l| l == 0 || l > MAX_PAGE) {
                return invalid("limit must be between 1 and 100");
            }
            if cursor.as_ref().is_some_and(|c| !token(c, 64)) {
                return invalid("cursor is not valid");
            }
            Ok(())
        };
        match self {
            Self::ContactsSearch {
                query,
                cursor,
                limit,
            } => {
                if query.chars().count() > 64 || query.chars().any(char::is_control) {
                    return invalid("query must be at most 64 printable characters");
                }
                page(limit, cursor)
            }
            Self::HistoryList { cursor, limit, .. } => page(limit, cursor),
            Self::OperationGet { operation_id } if !token(operation_id, 64) => {
                invalid("operation_id is not valid")
            }
            Self::CallDial { target } | Self::CallTransfer { target, .. }
                if DialTarget::parse(target).is_err() =>
            {
                invalid("target is not a valid number or SIP user")
            }
            Self::CallDtmf { digits, .. }
                if digits.is_empty()
                    || digits.len() > 32
                    || !digits.chars().all(|c| "0123456789*#ABCD".contains(c)) =>
            {
                invalid("digits must be 1 to 32 of 0-9 * # A-D")
            }
            _ => Ok(()),
        }
    }
}

/// `[A-Za-z0-9._:-]{1,max}`: identifiers, keys and cursors.
#[must_use]
pub fn token(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
}

/// A client request. `Debug` redacts the capability.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Client-chosen correlation identifier, echoed in the response.
    pub request_id: String,
    /// Required for mutations: retrying with the same key never repeats
    /// the effect; reusing it for another payload is a conflict.
    pub idempotency_key: Option<String>,
    /// What to do.
    pub operation: Operation,
    /// Secret of a grant issued in the window, read by the client from a
    /// private file (never from arguments). The owner stores only its hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability: Option<String>,
}
impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Request")
            .field("schema_version", &self.schema_version)
            .field("request_id", &self.request_id)
            .field("idempotency_key", &self.idempotency_key)
            .field("operation", &self.operation)
            .field(
                "capability",
                &self.capability.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}
impl Request {
    /// Validate the envelope and the operation.
    ///
    /// # Errors
    /// [`ErrorCode::Validation`].
    pub fn validate(&self) -> Result<(), ControlError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ControlError::new(
                ErrorCode::Validation,
                "unsupported schema_version",
            ));
        }
        if !token(&self.request_id, 64) {
            return Err(ControlError::new(
                ErrorCode::Validation,
                "request_id must be 1 to 64 of A-Z a-z 0-9 . _ : -",
            ));
        }
        if self.capability.as_ref().is_some_and(|c| !token(c, 128)) {
            return Err(ControlError::new(
                ErrorCode::Validation,
                "capability is not valid",
            ));
        }
        match (&self.idempotency_key, self.operation.is_mutation()) {
            (None, true) => {
                return Err(ControlError::new(
                    ErrorCode::Validation,
                    "idempotency_key is required for this operation",
                ));
            }
            (Some(key), _) if !token(key, 128) => {
                return Err(ControlError::new(
                    ErrorCode::Validation,
                    "idempotency_key must be 1 to 128 of A-Z a-z 0-9 . _ : -",
                ));
            }
            _ => {}
        }
        self.operation.validate()
    }
}

/// Outcome class of a response.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// A read succeeded, or a mutation's final outcome is known.
    Ok,
    /// A mutation was accepted; query `operation_get` for its outcome.
    Accepted,
    /// A person must approve the mutation in the window; query
    /// `operation_get` with the returned operation id. Not an error.
    ApprovalRequired,
    /// See [`ErrorBody`].
    Error,
}

/// Stable error codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Malformed or out-of-bounds input.
    Validation,
    /// No grant covers this operation.
    GrantRequired,
    /// A person denied the operation.
    ApprovalDenied,
    /// Nobody approved the operation in time.
    ApprovalExpired,
    /// The grant expired.
    GrantExpired,
    /// No owner is running; nothing was started.
    EngineUnavailable,
    /// The headless service owns the profile.
    EngineOwnedByService,
    /// The owner did not answer in time; the outcome may still change.
    Timeout,
    /// The idempotency key was used for a different payload.
    Conflict,
    /// The operation belongs to an earlier owner session.
    StaleEpoch,
    /// The stored outcome is older than the retention window.
    ResultExpired,
    /// Unknown call, operation or version.
    NotFound,
    /// Too many requests.
    RateLimited,
    /// The owner is busy; try again.
    Busy,
    /// The settings database or ledger is unavailable; nothing was done.
    StorageUnavailable,
    /// Not possible in the current state.
    InvalidState,
    /// The operation is not offered in this mode.
    Unsupported,
    /// Unexpected failure.
    Internal,
}
impl ErrorCode {
    /// CLI exit status for this error.
    #[must_use]
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Validation | Self::Conflict | Self::NotFound | Self::InvalidState => 2,
            Self::GrantRequired
            | Self::ApprovalDenied
            | Self::ApprovalExpired
            | Self::GrantExpired
            | Self::StaleEpoch => 3,
            Self::EngineUnavailable
            | Self::EngineOwnedByService
            | Self::StorageUnavailable
            | Self::Busy
            | Self::RateLimited
            | Self::Unsupported
            | Self::ResultExpired => 4,
            Self::Timeout => 5,
            Self::Internal => 1,
        }
    }
    /// Whether retrying later, with the same idempotency key, may succeed.
    #[must_use]
    pub const fn retryable(self) -> bool {
        matches!(
            self,
            Self::EngineUnavailable | Self::Timeout | Self::RateLimited | Self::Busy
        )
    }
}

/// Error details: a stable code and a message free of personal data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorBody {
    /// Stable code.
    pub code: ErrorCode,
    /// Human text; never contains numbers, names, digits or secrets.
    pub message: String,
    /// See [`ErrorCode::retryable`].
    pub retryable: bool,
}

/// An error before or during execution.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct ControlError {
    /// Stable code.
    pub code: ErrorCode,
    /// Message free of personal data.
    pub message: String,
}
impl ControlError {
    /// New error.
    #[must_use]
    pub fn new(code: ErrorCode, message: &str) -> Self {
        Self {
            code,
            message: message.to_owned(),
        }
    }
}

/// The owner's reply.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    /// [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Echo of [`Request::request_id`] (empty when the request was unreadable).
    pub request_id: String,
    /// Outcome class.
    pub status: Status,
    /// Set for accepted or pending mutations.
    pub operation_id: Option<String>,
    /// Result payload for reads and known outcomes.
    pub data: Option<serde_json::Value>,
    /// Set when `status` is [`Status::Error`].
    pub error: Option<ErrorBody>,
}
impl Response {
    /// A successful read or final outcome.
    #[must_use]
    pub fn ok(request_id: &str, data: serde_json::Value) -> Self {
        Self::with(request_id, Status::Ok, None, Some(data), None)
    }
    /// An accepted or pending mutation.
    #[must_use]
    pub fn pending(request_id: &str, status: Status, operation_id: &str) -> Self {
        Self::with(
            request_id,
            status,
            Some(operation_id.to_owned()),
            None,
            None,
        )
    }
    /// An error.
    #[must_use]
    pub fn error(request_id: &str, error: &ControlError) -> Self {
        let body = ErrorBody {
            code: error.code,
            message: error.message.clone(),
            retryable: error.code.retryable(),
        };
        Self::with(request_id, Status::Error, None, None, Some(body))
    }
    fn with(
        request_id: &str,
        status: Status,
        operation_id: Option<String>,
        data: Option<serde_json::Value>,
        error: Option<ErrorBody>,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            request_id: request_id.to_owned(),
            status,
            operation_id,
            data,
            error,
        }
    }
    /// CLI exit status: 0 done or accepted, 3 waiting for approval, else
    /// the error's code.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        match (self.status, &self.error) {
            (Status::Ok | Status::Accepted, _) => 0,
            (Status::ApprovalRequired, _) => 3,
            (Status::Error, Some(error)) => error.code.exit_code(),
            (Status::Error, None) => 1,
        }
    }
}

/// Write one message as a JSON line.
///
/// # Errors
/// I/O errors, or `InvalidData` when the message exceeds [`MAX_MESSAGE`].
pub fn write_message<T: Serialize>(writer: &mut impl Write, message: &T) -> io::Result<()> {
    let mut line = serde_json::to_vec(message).map_err(io::Error::other)?;
    line.push(b'\n');
    if line.len() > MAX_MESSAGE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message too large",
        ));
    }
    writer.write_all(&line)?;
    writer.flush()
}

/// Read one JSON line of at most [`MAX_MESSAGE`] bytes.
///
/// # Errors
/// `UnexpectedEof` at end of stream, `InvalidData` for an oversized,
/// non-UTF-8 or non-matching line, or other I/O errors.
pub fn read_message<T: for<'de> Deserialize<'de>>(reader: &mut impl BufRead) -> io::Result<T> {
    let mut line = Vec::new();
    let read = reader
        .by_ref()
        .take(MAX_MESSAGE as u64 + 1)
        .read_until(b'\n', &mut line)?;
    if read == 0 {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    if line.last() != Some(&b'\n') {
        let kind = if line.len() > MAX_MESSAGE {
            io::ErrorKind::InvalidData
        } else {
            io::ErrorKind::UnexpectedEof
        };
        return Err(io::Error::new(kind, "incomplete or oversized message"));
    }
    serde_json::from_slice(&line)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid message"))
}

#[cfg(test)]
mod tests;
