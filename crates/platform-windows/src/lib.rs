//! Windows desktop integration (add-windows-port W-B): the same API as the
//! Unix modules of `rust-microsip-platform`, which re-exports this crate on
//! Windows. The only crate allowed `unsafe` (Win32/WinRT calls), each block
//! with a `// SAFETY:` comment (design W2).
//!
//! Everything reports "unavailable": no single-instance pipe (the app then
//! runs in its documented degraded mode), no control channel for the CLI or
//! MCP, no toasts, no tray and no Credential Manager (passwords stay for the
//! session). Private files are read without owner/DACL checks.
// ponytail: stand-ins being replaced part by part during milestone W-B.

#![cfg(windows)]

mod credentials;
mod files;
mod notify;
mod pipes;
mod tray;

pub use credentials::{OPERATION_TIMEOUT, SecretBytes, SecretError, SecretStore};
pub use files::{PrivateFileError, read_own_file, read_private_file};
pub use notify::{CallAction, Delivery, NotificationAction, Notifier, NotifierStatus};
pub use pipes::{
    Acquired, ControlClient, ControlServer, ControlSocketError, GLOBAL_RATE, Incoming, Listener,
    MAX_CONNECTIONS, PER_CONNECTION_RATE, REPLY_DEADLINE, acquire, acquire_owner,
    default_runtime_dir,
};
pub use tray::{Tray, TrayAction, TrayStatus};

/// Callback run after a request or action is queued (same as the Unix crate).
pub type Waker = std::sync::Arc<dyn Fn() + Send + Sync>;
