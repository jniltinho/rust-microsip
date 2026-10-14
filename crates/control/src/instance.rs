//! Single-instance protocol: one line each way between a later launch and
//! the profile owner, shared by the Unix socket and the Windows named-pipe
//! adapters.

use rust_microsip_core::DialTarget;
use std::io;
use std::path::PathBuf;

/// Maximum length of one protocol line, including the newline.
pub const MAX_LINE: usize = 512;

/// A validated request forwarded by another process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Raise and focus the existing window.
    Activate,
    /// Show a dial confirmation for this target; never dial automatically.
    Dial(DialTarget),
}
impl Request {
    /// The wire line for this request, newline included.
    pub fn encode(&self) -> String {
        match self {
            Self::Activate => "v1 activate\n".to_owned(),
            Self::Dial(target) => format!("v1 dial {}\n", target.as_str()),
        }
    }
    /// Parse one wire line (without the newline); `None` when invalid.
    pub fn decode(line: &str) -> Option<Self> {
        match line.strip_prefix("v1 ")? {
            "activate" => Some(Self::Activate),
            rest => {
                let target = rest.strip_prefix("dial ")?;
                // DialTarget::parse trims; require the exact canonical form.
                let parsed = DialTarget::parse(target).ok()?;
                (parsed.as_str() == target).then_some(Self::Dial(parsed))
            }
        }
    }
}

/// Owner reply to a forwarded request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ack {
    /// The request was queued for the owner.
    Ok,
    /// The owner queue was full; the request was dropped.
    Busy,
    /// The owner rejected the request as malformed.
    Invalid,
    /// The headless service owns the profile: no window opens and links are
    /// not dialed (`engine_owned_by_service`, `uri_confirmation_unavailable`).
    Service,
}

/// Single-instance or IPC failure. Paths are included; request data never is.
#[derive(Debug, thiserror::Error)]
pub enum InstanceError {
    /// `XDG_RUNTIME_DIR` is unset or not absolute; there is no private fallback.
    #[cfg_attr(unix, error("XDG_RUNTIME_DIR is not set to an absolute path"))]
    #[cfg_attr(not(unix), error("it is not available on this platform yet"))]
    NoRuntimeDir,
    /// The runtime directory or socket is not a private object owned by this user.
    #[error("{0} is not private to the current user (expected owner-only access)")]
    Insecure(PathBuf),
    /// The owner process did not answer within the bounded timeout.
    #[error("the running instance did not respond")]
    NoResponse,
    /// The owner answered with an unexpected line.
    #[error("the running instance sent an invalid reply")]
    Protocol,
    /// An I/O operation failed.
    #[error("{path}: {source}")]
    Io {
        /// Object being accessed.
        path: PathBuf,
        /// Underlying error.
        source: io::Error,
    },
}
