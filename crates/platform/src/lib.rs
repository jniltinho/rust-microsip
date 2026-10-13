//! Linux desktop integration: single instance with protected local IPC, the
//! authenticated control socket for the CLI and MCP server,
//! strict `sip:`/`tel:` link parsing, incoming-call notifications, the tray icon and
//! Secret Service password storage.
//!
//! Nothing here executes shell commands or installs files: the desktop entry
//! is only rendered, and installing it is a user-level opt-in left to the caller.

mod control;
mod instance;
mod notify;
mod private_file;
mod secrets;
mod tray;
mod uri;

pub use control::{
    ControlClient, ControlServer, ControlSocketError, GLOBAL_RATE, Incoming, MAX_CONNECTIONS,
    PER_CONNECTION_RATE, REPLY_DEADLINE,
};
pub use instance::{
    Ack, Acquired, InstanceError, Listener, MAX_LINE, Request, acquire, acquire_owner,
    default_runtime_dir,
};
pub use notify::{
    CallAction, Delivery, NotificationAction, Notifier, NotifierStatus, sanitize_caller,
};
pub use private_file::{PrivateFileError, read_private_file};
pub use secrets::{OPERATION_TIMEOUT, SecretBytes, SecretError, SecretStore};
pub use tray::{Tray, TrayAction, TrayStatus};
pub use uri::{MAX_URI_LEN, UriError, parse_dial_uri};

/// Callback run after a request or action is queued, e.g. to wake the UI loop.
pub type Waker = std::sync::Arc<dyn Fn() + Send + Sync>;

/// Freedesktop `.desktop` entry registering the `sip:`/`tel:` handlers.
///
/// Render-only: callers install it (for example under
/// `$XDG_DATA_HOME/applications/`) only after explicit user consent.
pub const DESKTOP_ENTRY: &str = "[Desktop Entry]
Type=Application
Name=Rust MicroSIP
Comment=SIP softphone
Exec=rust-microsip %u
Icon=rust-microsip
Terminal=false
Categories=Network;Telephony;
MimeType=x-scheme-handler/sip;x-scheme-handler/tel;
StartupNotify=true
";

#[cfg(test)]
mod tests {
    #[test]
    fn desktop_entry_declares_handlers() {
        let entry = super::DESKTOP_ENTRY;
        assert!(entry.contains("\nExec=rust-microsip %u\n"));
        assert!(entry.contains("\nMimeType=x-scheme-handler/sip;x-scheme-handler/tel;\n"));
    }
}
