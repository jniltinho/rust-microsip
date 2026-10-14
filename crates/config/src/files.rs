//! Bounded, permission-checked file reads and atomic saves.

use crate::{ConfigError, StoredConfig};
use perm::{DirBuilderExt, OpenOptionsExt};
use secrecy::SecretString;
use secrecy::zeroize::Zeroizing;
use std::fs::{self, DirBuilder, File, Metadata, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Owner, mode and identity of files: Unix metadata, or neutral values on
/// Windows until the platform adapter checks DACLs (add-windows-port 2.6).
#[cfg(unix)]
mod perm {
    use std::fs::Metadata;
    use std::os::unix::fs::MetadataExt;
    pub(super) use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

    pub(super) fn same_file(a: &Metadata, b: &Metadata) -> bool {
        (a.dev(), a.ino()) == (b.dev(), b.ino())
    }
    pub(super) fn mode(meta: &Metadata) -> u32 {
        meta.mode()
    }
    pub(super) fn owner(meta: &Metadata) -> u32 {
        meta.uid()
    }
    /// Effective user id (owner of `/proc/self`, Linux only).
    pub(super) fn current_uid() -> std::io::Result<u32> {
        std::fs::metadata("/proc/self").map(|m| m.uid())
    }
}
#[cfg(windows)]
mod perm {
    // ponytail: no owner/mode checks on Windows yet; the adapter's DACLs
    // (add-windows-port task 2.6) replace them.
    use std::fs::Metadata;

    pub(super) trait DirBuilderExt {
        fn mode(&mut self, mode: u32) -> &mut Self;
    }
    impl DirBuilderExt for std::fs::DirBuilder {
        fn mode(&mut self, _mode: u32) -> &mut Self {
            self
        }
    }
    pub(super) trait OpenOptionsExt {
        fn mode(&mut self, mode: u32) -> &mut Self;
    }
    impl OpenOptionsExt for std::fs::OpenOptions {
        fn mode(&mut self, _mode: u32) -> &mut Self {
            self
        }
    }
    pub(super) fn same_file(a: &Metadata, b: &Metadata) -> bool {
        a.len() == b.len() && a.modified().ok() == b.modified().ok()
    }
    pub(super) fn mode(_meta: &Metadata) -> u32 {
        0o600
    }
    pub(super) fn owner(_meta: &Metadata) -> u32 {
        0
    }
    pub(super) fn current_uid() -> std::io::Result<u32> {
        Ok(0)
    }
}

/// Upper bound for TOML and `.env` files.
pub(crate) const MAX_CONFIG_BYTES: u64 = 64 * 1024;
/// Upper bound for password files.
const MAX_SECRET_BYTES: u64 = 4 * 1024;

/// Why a file was refused. The message tells the user how to fix it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FileProblem {
    /// The secret file is a symbolic link.
    #[error("it is a symbolic link; point the setting at the real file")]
    Symlink,
    /// Not a regular file.
    #[error("it is not a regular file")]
    NotRegular,
    /// Another user could modify it.
    #[error("it is writable by group or others; run `chmod go-w` on it")]
    Writable,
    /// It holds a secret but is not owned by the current user.
    #[error("it contains a secret but is owned by another user; run `chown` to take ownership")]
    NotOwned,
    /// Owned by a user other than the current user or root, who could rewrite it.
    #[error("it is owned by another user; run `chown` to take ownership")]
    ForeignOwner,
    /// It holds a secret but group or others have access.
    #[error("it contains a secret but is accessible by group or others; run `chmod 600` on it")]
    NotPrivate,
}

fn io_err(path: &Path) -> impl Fn(std::io::Error) -> ConfigError + '_ {
    move |e| ConfigError::Io {
        path: path.to_owned(),
        kind: e.kind(),
    }
}
fn insecure(path: &Path, problem: FileProblem) -> ConfigError {
    ConfigError::InsecureFile {
        path: path.to_owned(),
        problem,
    }
}

/// Effective user id of this process (owner of `/proc/self` on Linux).
pub(crate) fn current_uid() -> Result<u32, ConfigError> {
    perm::current_uid().map_err(io_err(Path::new("/proc/self")))
}

/// A file's text (zeroed on drop) and the metadata of the opened handle.
pub(crate) struct Loaded {
    pub(crate) text: Zeroizing<String>,
    pub(crate) meta: Metadata,
}

/// Read a regular, not group/other-writable file of at most `limit` bytes.
/// Returns `Ok(None)` when it does not exist.
pub(crate) fn read_bounded(
    path: &Path,
    limit: u64,
    reject_symlink: bool,
    owner: u32,
) -> Result<Option<Loaded>, ConfigError> {
    let io = io_err(path);
    // Check the type before opening so a FIFO or device never blocks the read.
    let before = match if reject_symlink {
        fs::symlink_metadata(path)
    } else {
        fs::metadata(path)
    } {
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        other => other.map_err(&io)?,
    };
    if before.file_type().is_symlink() {
        return Err(insecure(path, FileProblem::Symlink));
    }
    if !before.is_file() {
        return Err(insecure(path, FileProblem::NotRegular));
    }
    let file = File::open(path).map_err(&io)?;
    let meta = file.metadata().map_err(&io)?;
    if !perm::same_file(&meta, &before) {
        // Replaced between the check and the open.
        return Err(insecure(
            path,
            if reject_symlink {
                FileProblem::Symlink
            } else {
                FileProblem::NotRegular
            },
        ));
    }
    if perm::mode(&meta) & 0o022 != 0 {
        return Err(insecure(path, FileProblem::Writable));
    }
    if perm::owner(&meta) != owner && perm::owner(&meta) != 0 {
        return Err(insecure(path, FileProblem::ForeignOwner));
    }
    let mut buf = Zeroizing::new(Vec::new());
    file.take(limit + 1).read_to_end(&mut buf).map_err(&io)?;
    if buf.len() as u64 > limit {
        return Err(ConfigError::FileTooLarge(path.to_owned()));
    }
    let text = std::str::from_utf8(&buf).map_err(|_| ConfigError::NotUtf8(path.to_owned()))?;
    Ok(Some(Loaded {
        text: Zeroizing::new(text.to_owned()),
        meta,
    }))
}

/// Largest accepted PEM CA bundle.
pub const MAX_CA_BYTES: u64 = 4 * 1024 * 1024;

/// Read a PEM CA bundle that decides which TLS servers are trusted.
///
/// It must be a regular file of at most [`MAX_CA_BYTES`], owned by `uid` or
/// root and not writable by group or others: anyone able to change it could
/// intercept TLS and the SDES keys carried inside.
///
/// # Errors
/// Returns [`ConfigError::FileMissing`], [`ConfigError::InsecureFile`],
/// [`ConfigError::FileTooLarge`] or an I/O error; callers must not fall back
/// to other trust roots when a configured bundle fails.
pub fn read_ca_file(path: &Path, uid: u32) -> Result<Vec<u8>, ConfigError> {
    let loaded = read_bounded(path, MAX_CA_BYTES, false, uid)?
        .ok_or_else(|| ConfigError::FileMissing(path.to_owned()))?;
    Ok(loaded.text.as_bytes().to_vec())
}

/// A secret-bearing file must be owned by `uid` with no group/other permission bits.
pub(crate) fn require_private(path: &Path, meta: &Metadata, uid: u32) -> Result<(), ConfigError> {
    if perm::owner(meta) != uid {
        return Err(insecure(path, FileProblem::NotOwned));
    }
    if perm::mode(meta) & 0o077 != 0 {
        return Err(insecure(path, FileProblem::NotPrivate));
    }
    Ok(())
}

/// Read a private, non-symlink password file; one trailing newline is removed.
pub(crate) fn read_password_file(path: &Path, uid: u32) -> Result<SecretString, ConfigError> {
    let loaded = read_bounded(path, MAX_SECRET_BYTES, true, uid)?
        .ok_or_else(|| ConfigError::FileMissing(path.to_owned()))?;
    require_private(path, &loaded.meta, uid)?;
    let text = loaded.text.as_str();
    let secret = text
        .strip_suffix('\n')
        .map_or(text, |t| t.strip_suffix('\r').unwrap_or(t));
    if secret.is_empty() {
        return Err(ConfigError::EmptyPassword(path.to_owned()));
    }
    Ok(SecretString::from(secret))
}

/// Atomically write `config` as TOML to `path`.
///
/// Missing parent directories are created with mode `0700` (existing ones are left
/// as they are). The data goes to a `0600` temporary file in the same directory,
/// is flushed with `fsync`, renamed over `path`, and the directory is synced. The
/// previous file is replaced entirely: its comments, ordering and any literal
/// password are not preserved. No secret is ever written.
///
/// # Errors
/// [`ConfigError::Io`] on filesystem failures (the previous file stays intact), or
/// the errors of [`StoredConfig::to_toml`].
pub fn save(config: &StoredConfig, path: &Path) -> Result<(), ConfigError> {
    let text = config.to_toml()?;
    let io = io_err(path);
    let name = path.file_name().ok_or(ConfigError::Io {
        path: path.to_owned(),
        kind: ErrorKind::InvalidInput,
    })?;
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(&io)?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let tmp = dir.join(format!(
        ".{}.{}-{nanos}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    let write = || -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        File::open(dir)?.sync_all()
    };
    write().map_err(|e| {
        let _ = fs::remove_file(&tmp);
        io(e)
    })
}
