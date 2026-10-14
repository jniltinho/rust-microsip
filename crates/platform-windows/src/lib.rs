//! Windows desktop integration (add-windows-port W-B): the same API as the
//! Unix modules of `rust-microsip-platform`, which re-exports this crate on
//! Windows. The only crate allowed `unsafe` (Win32/WinRT calls), each block
//! with a `// SAFETY:` comment (design W2).
//!
//! Named pipes restricted to the user's SID (single instance and control),
//! Credential Manager, owner-only DACLs, the notification-area icon and
//! WinRT toasts. Verified so far only by cross-compiled tests and Wine smoke
//! runs; see `docs/receipts/` for Windows evidence.

#![cfg(windows)]

mod credentials;
mod files;
mod notify;
mod pipes;
mod tray;

pub use credentials::{OPERATION_TIMEOUT, SecretBytes, SecretError, SecretStore};
pub use files::{
    PrivateFileError, make_private_dir, read_own_file, read_private_file, write_private_file,
};
pub use notify::{CallAction, Delivery, NotificationAction, Notifier, NotifierStatus};
pub use pipes::{
    Acquired, ControlClient, ControlServer, ControlSocketError, GLOBAL_RATE, Incoming, Listener,
    MAX_CONNECTIONS, PER_CONNECTION_RATE, REPLY_DEADLINE, acquire, acquire_owner,
    default_runtime_dir,
};
pub use tray::{Tray, TrayAction, TrayStatus};

/// Callback run after a request or action is queued (same as the Unix crate).
pub type Waker = std::sync::Arc<dyn Fn() + Send + Sync>;
