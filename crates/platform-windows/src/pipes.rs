//! Single instance and control channel over named pipes (design W4).
// ponytail: stand-in until W-B task 2.3/2.4.

use crate::Waker;
use rust_microsip_control::instance::{Ack, InstanceError, Request};
use rust_microsip_control::{OwnerMode, Request as ControlRequest, Response, Welcome};
use std::io;
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
