//! Desktop integration (Linux here; Windows in `rust-microsip-platform-windows`, re-exported): single instance with protected local IPC, the
//! authenticated control socket for the CLI and MCP server,
//! strict `sip:`/`tel:` link parsing, incoming-call notifications, the tray icon and
//! Secret Service password storage.
//!
//! Nothing here executes shell commands or installs files: the desktop entry
//! is only rendered, and installing it is a user-level opt-in left to the caller.

#[cfg(unix)]
mod control;
#[cfg(unix)]
mod instance;
#[cfg(unix)]
mod notify;
#[cfg(unix)]
mod private_file;
#[cfg(unix)]
mod secrets;
#[cfg(unix)]
mod tray;
mod uri;

pub use rust_microsip_control::caller::sanitize_caller;
pub use rust_microsip_control::instance::{Ack, InstanceError, MAX_LINE, Request};
pub use uri::{MAX_URI_LEN, UriError, parse_dial_uri};

#[cfg(unix)]
pub use control::{
    ControlClient, ControlServer, ControlSocketError, GLOBAL_RATE, Incoming, MAX_CONNECTIONS,
    PER_CONNECTION_RATE, REPLY_DEADLINE,
};
#[cfg(unix)]
pub use instance::{Acquired, Listener, acquire, acquire_owner, default_runtime_dir};
#[cfg(unix)]
pub use notify::{CallAction, Delivery, NotificationAction, Notifier, NotifierStatus};
#[cfg(unix)]
pub use private_file::{PrivateFileError, read_own_file, read_private_file};
#[cfg(windows)]
pub use rust_microsip_platform_windows::{
    Acquired, CallAction, ControlClient, ControlServer, ControlSocketError, Delivery, GLOBAL_RATE,
    Incoming, Listener, MAX_CONNECTIONS, MAX_RECENT, NotificationAction, Notifier, NotifierStatus,
    OPERATION_TIMEOUT, PER_CONNECTION_RATE, PrivateFileError, REPLY_DEADLINE, RecentCall,
    SecretBytes, SecretError, SecretStore, Tray, TrayAction, TrayStatus, acquire, acquire_owner,
    default_runtime_dir, make_private_dir, read_own_file, read_private_file, write_private_file,
};
#[cfg(unix)]
pub use secrets::{OPERATION_TIMEOUT, SecretBytes, SecretError, SecretStore};
#[cfg(unix)]
pub use tray::{MAX_RECENT, RecentCall, Tray, TrayAction, TrayStatus};

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

/// [`DESKTOP_ENTRY`] with `Exec` set to the absolute path of `executable`.
///
/// GLib ignores an entry whose `Exec` program is not on `PATH`, which is the
/// usual case for a downloaded ELF or an AppImage: the dock icon and the link
/// handler only work when the entry names the real file. Paths that would
/// need escaping or are not allowed in `Exec` (quotes, `` ` ``, `$`, `\`,
/// `%`, `=`, control characters) fall back to the bare name.
pub fn desktop_entry(executable: Option<&std::path::Path>) -> String {
    let quoted = executable
        .filter(|p| p.is_absolute())
        .and_then(|p| p.to_str())
        .filter(|p| {
            !p.chars()
                .any(|c| matches!(c, '"' | '`' | '$' | '\\' | '%' | '=') || c.is_control())
        })
        .map(|p| format!("\"{p}\""));
    match quoted {
        Some(exec) => {
            DESKTOP_ENTRY.replace("\nExec=rust-microsip %u\n", &format!("\nExec={exec} %u\n"))
        }
        None => DESKTOP_ENTRY.to_owned(),
    }
}

/// The file a desktop entry should start: the AppImage when this process
/// runs inside one, otherwise this executable. `$APPIMAGE` is trusted only
/// when this executable lies under `$APPDIR` (a child of another AppImage
/// inherits both), and only existing regular files count (not a replaced
/// binary's "(deleted)" path).
pub fn desktop_executable() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let appimage = std::env::var_os("APPDIR")
        .map(std::path::PathBuf::from)
        .filter(|dir| dir.is_absolute() && exe.starts_with(dir))
        .and_then(|_| std::env::var_os("APPIMAGE"))
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute() && p.is_file());
    appimage.or_else(|| Some(exe).filter(|p| p.is_file()))
}

#[cfg(test)]
mod tests {
    // Desktop entries are a Linux mechanism (Windows paths are not `/`-rooted).
    #[cfg(unix)]
    #[test]
    fn entry_names_the_real_executable() {
        use std::path::Path;
        let entry = super::desktop_entry(Some(Path::new("/opt/My Apps/rust-microsip.AppImage")));
        assert!(entry.contains("\nExec=\"/opt/My Apps/rust-microsip.AppImage\" %u\n"));
        assert_eq!(entry.matches("\nExec=").count(), 1);
        for odd in [
            "relative/app",
            "/tmp/a\"b",
            "/tmp/$HOME",
            "/tmp/50%",
            "/tmp/a=b",
        ] {
            assert_eq!(
                super::desktop_entry(Some(Path::new(odd))),
                super::DESKTOP_ENTRY
            );
        }
        // The test binary is a regular file outside any AppImage.
        assert_eq!(super::desktop_executable(), std::env::current_exe().ok());
    }

    #[test]
    fn desktop_entry_declares_handlers() {
        let entry = super::DESKTOP_ENTRY;
        assert!(entry.contains("\nExec=rust-microsip %u\n"));
        assert!(entry.contains("\nMimeType=x-scheme-handler/sip;x-scheme-handler/tel;\n"));
    }
}
