//! Windows build (add-windows-port, milestone W-A): safe stand-ins with the
//! same API as the Unix modules, so the app compiles and runs on Windows.
//!
//! Everything reports "unavailable": no single-instance pipe (the app then
//! runs in its documented degraded mode), no control channel for the CLI or
//! MCP, no toasts, no tray and no Credential Manager (passwords stay for the
//! session). Private files are read without owner/DACL checks.
// ponytail: stand-ins until milestone W-B moves the real named pipes,
// toasts, tray, Credential Manager and DACL checks into crates/platform-windows.

use crate::Waker;
use crate::ipc::{Ack, InstanceError, Request};
use rust_microsip_control::{OwnerMode, Request as ControlRequest, Response, Welcome};
use rust_microsip_core::CallId;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

// --- single instance -------------------------------------------------------

/// Result of [`acquire`].
#[derive(Debug)]
pub enum Acquired {
    /// This process owns the profile and must serve forwarded requests.
    Owner(Listener),
    /// Another process owns the profile and acknowledged the request; exit now.
    Forwarded(Ack),
}

/// Owner side of the single-instance channel (not available yet on Windows).
#[derive(Debug)]
pub struct Listener {
    runtime_dir: PathBuf,
}

impl Listener {
    /// Mark the owner as the headless service.
    pub fn set_service_mode(&self) {}
    /// Directory holding the owner's private files.
    pub fn runtime_dir(&self) -> &Path {
        &self.runtime_dir
    }
    /// Run `waker` after each queued request.
    pub fn set_waker(&self, _waker: Waker) {}
    /// Next forwarded request, without blocking.
    pub fn try_recv(&self) -> Option<Request> {
        None
    }
    /// Next forwarded request, waiting up to `timeout`.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<Request> {
        std::thread::sleep(timeout);
        None
    }
}

/// No per-session runtime directory yet: the app runs without
/// single-instance protection, with a warning.
///
/// # Errors
/// Always [`InstanceError::NoRuntimeDir`].
pub fn default_runtime_dir() -> Result<PathBuf, InstanceError> {
    Err(InstanceError::NoRuntimeDir)
}

/// See the Unix implementation.
///
/// # Errors
/// Always [`InstanceError::NoRuntimeDir`] on Windows for now.
pub fn acquire(_runtime_dir: &Path, _request: &Request) -> Result<Acquired, InstanceError> {
    Err(InstanceError::NoRuntimeDir)
}

/// See the Unix implementation.
///
/// # Errors
/// Always [`InstanceError::NoRuntimeDir`] on Windows for now.
pub fn acquire_owner(_runtime_dir: &Path) -> Result<Option<Listener>, InstanceError> {
    Err(InstanceError::NoRuntimeDir)
}

// --- control channel -------------------------------------------------------

/// Simultaneous client connections.
pub const MAX_CONNECTIONS: usize = 16;
/// Requests per second per connection.
pub const PER_CONNECTION_RATE: u32 = 10;
/// Requests per second for all connections.
pub const GLOBAL_RATE: u64 = 50;
/// How long a client waits for the owner's reply.
pub const REPLY_DEADLINE: Duration = Duration::from_secs(5);

/// A client request waiting for the owner's answer.
#[derive(Debug)]
pub struct Incoming {
    /// The validated request.
    pub request: ControlRequest,
    /// Client process id, when known.
    pub peer_pid: Option<u32>,
}

impl Incoming {
    /// Whether the client has already given up.
    #[must_use]
    pub fn is_stale(&self) -> bool {
        true
    }
    /// Send the answer (dropped: there is no channel yet).
    pub fn respond(self, _response: Response) {}
}

/// Control channel failure.
#[derive(Debug, thiserror::Error)]
pub enum ControlSocketError {
    /// No owner is listening.
    #[error("no rust-microsip instance is running")]
    Unavailable,
    /// The channel is not private to the current user.
    #[error("{0} is not private to the current user")]
    Insecure(PathBuf),
    /// The owner speaks another protocol version.
    #[error("the running instance speaks an incompatible control protocol")]
    Protocol,
    /// The owner refused the connection for now.
    #[error("the running instance is busy; try again")]
    Busy,
    /// An I/O error.
    #[error("{0}")]
    Io(#[from] io::Error),
}

/// Owner side of the control channel (not available yet on Windows).
#[derive(Debug)]
pub struct ControlServer;

impl ControlServer {
    /// Start serving clients.
    ///
    /// # Errors
    /// Always [`ControlSocketError::Unavailable`] on Windows for now.
    pub fn start(
        _owner: &Listener,
        _mode: OwnerMode,
        _epoch: u64,
    ) -> Result<Self, ControlSocketError> {
        Err(ControlSocketError::Unavailable)
    }
    /// Run `waker` after each queued request.
    pub fn set_waker(&self, _waker: Waker) {}
    /// Next request, without blocking.
    pub fn try_recv(&self) -> Option<Incoming> {
        None
    }
    /// Next request, waiting up to `timeout`.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<Incoming> {
        std::thread::sleep(timeout);
        None
    }
}

impl Drop for ControlServer {
    // Mirrors the Unix server, whose drop stops the accept thread.
    fn drop(&mut self) {}
}

/// Client side of the control channel.
#[derive(Debug)]
pub struct ControlClient {
    welcome: Welcome,
}

impl ControlClient {
    /// Connect to the owner.
    ///
    /// # Errors
    /// Always [`ControlSocketError::Unavailable`] on Windows for now.
    pub fn connect(_runtime_dir: &Path) -> Result<Self, ControlSocketError> {
        Err(ControlSocketError::Unavailable)
    }
    /// The owner's greeting.
    pub const fn welcome(&self) -> &Welcome {
        &self.welcome
    }
    /// Send one request and wait for the answer.
    ///
    /// # Errors
    /// Always an I/O error on Windows for now.
    pub fn call(&mut self, _request: &ControlRequest) -> io::Result<Response> {
        Err(io::ErrorKind::NotConnected.into())
    }
}

// --- notifications ---------------------------------------------------------

/// Whether desktop notifications can currently be shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifierStatus {
    /// Still connecting.
    Connecting,
    /// Notifications work; `actions` tells whether buttons are supported.
    Available {
        /// The server supports answer/reject buttons.
        actions: bool,
    },
    /// No notification service; use the window only.
    Unavailable,
}

/// Outcome of a non-blocking notification request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Queued.
    Queued,
    /// The queue is full; dropped.
    QueueFull,
    /// Notifications are unavailable; nothing was sent.
    Unavailable,
}

/// Button pressed on an incoming-call notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallAction {
    /// "Answer".
    Answer,
    /// "Reject".
    Reject,
}

/// Action invoked on the notification for `call`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotificationAction {
    /// Call the notification was shown for.
    pub call: CallId,
    /// Chosen action.
    pub action: CallAction,
}

/// Incoming-call notifier (toasts arrive in milestone W-B).
#[derive(Debug, Default)]
pub struct Notifier;

impl Notifier {
    /// The user's notification service.
    pub fn session() -> Self {
        Self
    }
    /// An explicit service address (tests).
    pub fn with_address(_address: &str) -> Self {
        Self
    }
    /// Current availability.
    pub fn status(&self) -> NotifierStatus {
        NotifierStatus::Unavailable
    }
    /// Run `waker` after each action.
    pub fn set_waker(&self, _waker: Waker) {}
    /// Show "Incoming call".
    pub fn incoming_call(&self, _call: CallId, _caller: &str) -> Delivery {
        Delivery::Unavailable
    }
    /// Close the notification for `call`.
    pub fn close(&self, _call: CallId) -> Delivery {
        Delivery::Unavailable
    }
    /// Next action pressed by the user.
    pub fn try_recv_action(&self) -> Option<NotificationAction> {
        None
    }
}

// --- tray ------------------------------------------------------------------

/// Whether the tray icon is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayStatus {
    /// Still connecting.
    Connecting,
    /// Shown.
    Available,
    /// Not shown.
    Unavailable,
}

/// A click on the tray icon or its menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    /// Bring the window to the front.
    Show,
    /// Quit like the window's close button.
    Quit,
}

/// Tray icon (the notification area icon arrives in milestone W-B).
#[derive(Debug)]
pub struct Tray;

impl Tray {
    /// Show the icon.
    pub fn session(_width: u32, _height: u32, _rgba: &[u8]) -> Self {
        Self
    }
    /// Current availability.
    pub fn status(&self) -> TrayStatus {
        TrayStatus::Unavailable
    }
    /// Run `waker` after each click.
    pub fn set_waker(&self, _waker: Waker) {}
    /// Next click.
    pub fn try_recv(&self) -> Option<TrayAction> {
        None
    }
}

// --- secrets ---------------------------------------------------------------

/// How long a keyring operation may take.
pub const OPERATION_TIMEOUT: Duration = Duration::from_secs(10);

/// Keyring failure. Never contains secret material.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SecretError {
    /// No keyring available.
    #[error("the keyring is unavailable")]
    Unavailable,
    /// The keyring is locked.
    #[error("the keyring is locked")]
    Locked,
    /// No default keyring.
    #[error("there is no default keyring")]
    NoDefaultCollection,
    /// No answer in time.
    #[error("the keyring did not respond in time")]
    Timeout,
    /// Invalid account reference.
    #[error("invalid account reference")]
    InvalidAccount,
    /// Other keyring error.
    #[error("keyring error: {0}")]
    Service(String),
}

/// Secret bytes, redacted in `Debug` and wiped on drop.
pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    /// Borrow the secret.
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretBytes(<redacted>)")
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        self.0.fill(0);
        std::hint::black_box(&mut self.0);
    }
}

/// Keyring handle (Credential Manager arrives in milestone W-B).
#[derive(Clone, Debug)]
pub struct SecretStore;

impl SecretStore {
    /// Connect to the user's keyring.
    ///
    /// # Errors
    /// Always [`SecretError::Unavailable`] on Windows for now.
    pub fn connect() -> Result<Self, SecretError> {
        Err(SecretError::Unavailable)
    }
    /// Connect to an explicit address (tests).
    ///
    /// # Errors
    /// Always [`SecretError::Unavailable`] on Windows for now.
    pub fn with_address(_address: &str) -> Result<Self, SecretError> {
        Err(SecretError::Unavailable)
    }
    /// Store a secret.
    ///
    /// # Errors
    /// Always [`SecretError::Unavailable`].
    pub fn store(
        &self,
        _account_ref: &str,
        _label: &str,
        _secret: &[u8],
    ) -> Result<(), SecretError> {
        Err(SecretError::Unavailable)
    }
    /// Read a secret.
    ///
    /// # Errors
    /// Always [`SecretError::Unavailable`].
    pub fn retrieve(&self, _account_ref: &str) -> Result<Option<SecretBytes>, SecretError> {
        Err(SecretError::Unavailable)
    }
    /// Delete a secret.
    ///
    /// # Errors
    /// Always [`SecretError::Unavailable`].
    pub fn delete(&self, _account_ref: &str) -> Result<(), SecretError> {
        Err(SecretError::Unavailable)
    }
}

// --- private files ---------------------------------------------------------

/// Why a private file was refused.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PrivateFileError {
    /// Missing or unreadable.
    #[error("the file cannot be read")]
    Unreadable,
    /// Not a regular file of this user.
    #[error("the file must be a private regular file of this user")]
    NotPrivate,
    /// Larger than the limit.
    #[error("the file is too large")]
    TooLarge,
}

fn read_regular(path: &Path, max: u64) -> Result<Vec<u8>, PrivateFileError> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| PrivateFileError::Unreadable)?;
    if !meta.is_file() {
        return Err(PrivateFileError::NotPrivate);
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| PrivateFileError::Unreadable)?
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| PrivateFileError::Unreadable)?;
    if bytes.len() as u64 > max {
        return Err(PrivateFileError::TooLarge);
    }
    Ok(bytes)
}

/// Read up to `max` bytes of UTF-8 from a regular file (no DACL check yet).
///
/// # Errors
/// [`PrivateFileError`].
pub fn read_private_file(path: &Path, max: u64) -> Result<String, PrivateFileError> {
    String::from_utf8(read_regular(path, max)?).map_err(|_| PrivateFileError::Unreadable)
}

/// Read up to `max` bytes from a regular file.
///
/// # Errors
/// [`PrivateFileError`].
pub fn read_own_file(path: &Path, max: u64) -> Result<Vec<u8>, PrivateFileError> {
    read_regular(path, max)
}
