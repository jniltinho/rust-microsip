//! Owner-only files and directories (design W6).
// ponytail: stand-in (no DACL check) until W-B task 2.6.

use std::io::Read;
use std::path::Path;
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
