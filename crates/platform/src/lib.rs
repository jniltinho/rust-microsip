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

/// [`DESKTOP_ENTRY`] with `Exec` set to the absolute path of `executable`.
///
/// GLib ignores an entry whose `Exec` program is not on `PATH`, which is the
/// usual case for a downloaded ELF or an AppImage: the dock icon and the link
/// handler only work when the entry names the real file. Paths that would
/// need escaping (quotes, `` ` ``, `$`, `\`, `%`, control characters) fall back
/// to the bare name.
pub fn desktop_entry(executable: Option<&std::path::Path>) -> String {
    let quoted = executable
        .filter(|p| p.is_absolute())
        .and_then(|p| p.to_str())
        .filter(|p| {
            !p.chars()
                .any(|c| matches!(c, '"' | '`' | '$' | '\\' | '%') || c.is_control())
        })
        .map(|p| format!("\"{p}\""));
    match quoted {
        Some(exec) => {
            DESKTOP_ENTRY.replace("\nExec=rust-microsip %u\n", &format!("\nExec={exec} %u\n"))
        }
        None => DESKTOP_ENTRY.to_owned(),
    }
}

/// Whether `text` is exactly an entry [`desktop_entry`] can write (for any
/// executable path), so removing it never deletes a user's or another
/// application's edits.
pub fn is_desktop_entry(text: &str) -> bool {
    let Some(start) = text.find("\nExec=") else {
        return false;
    };
    let line_end = text[start + 1..]
        .find('\n')
        .map_or(text.len(), |i| start + 1 + i);
    let exec = &text[start + 1..line_end];
    let generic = format!(
        "{}{}",
        &text[..start + 1],
        text.get(line_end + 1..).unwrap_or("")
    );
    let ours = exec == "Exec=rust-microsip %u"
        || exec
            .strip_prefix("Exec=\"/")
            .and_then(|rest| rest.strip_suffix("\" %u"))
            .is_some_and(|path| {
                !path
                    .chars()
                    .any(|c| matches!(c, '"' | '`' | '$' | '\\' | '%') || c.is_control())
            });
    ours && generic == DESKTOP_ENTRY.replace("Exec=rust-microsip %u\n", "")
}

#[cfg(test)]
mod tests {
    #[test]
    fn entry_names_the_real_executable_and_is_recognized() {
        use std::path::Path;
        let entry = super::desktop_entry(Some(Path::new("/opt/My Apps/rust-microsip.AppImage")));
        assert!(entry.contains("\nExec=\"/opt/My Apps/rust-microsip.AppImage\" %u\n"));
        assert!(super::is_desktop_entry(&entry));
        assert!(super::is_desktop_entry(super::DESKTOP_ENTRY));
        for odd in ["relative/app", "/tmp/a\"b", "/tmp/$HOME", "/tmp/50%"] {
            assert_eq!(
                super::desktop_entry(Some(Path::new(odd))),
                super::DESKTOP_ENTRY
            );
        }
        let edited = super::DESKTOP_ENTRY.replace("%u", "--bogus %u");
        assert!(!super::is_desktop_entry(&edited));
        assert!(!super::is_desktop_entry(
            &entry.replace("Telephony", "Office")
        ));
        assert!(!super::is_desktop_entry(
            &entry.replace("\" %u", "\" --x %u")
        ));
    }

    #[test]
    fn desktop_entry_declares_handlers() {
        let entry = super::DESKTOP_ENTRY;
        assert!(entry.contains("\nExec=rust-microsip %u\n"));
        assert!(entry.contains("\nMimeType=x-scheme-handler/sip;x-scheme-handler/tel;\n"));
    }
}
