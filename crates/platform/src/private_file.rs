//! Reading small secret files (grant capabilities) that must be private to
//! this user.

use rustix::fs::{FileType, Mode, OFlags};
use std::io::Read;
use std::path::Path;

/// Why a private file was refused.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PrivateFileError {
    /// Missing, unreadable, a symlink or a FIFO that would block.
    #[error("the file cannot be read")]
    Unreadable,
    /// Not a regular file of this user with mode 0600 (or stricter).
    #[error("the file must be a private regular file (mode 0600) of this user")]
    NotPrivate,
    /// Larger than the limit.
    #[error("the file is too large")]
    TooLarge,
}

/// Read up to `max` bytes of UTF-8 from `path`, only if it is a regular file
/// owned by this user with no group/other permissions.
///
/// The file is opened once with `O_NOFOLLOW | O_NONBLOCK` (no symlink, no
/// blocking on a FIFO) and every check runs on that descriptor, so a path
/// swapped between check and read cannot be read.
///
/// # Errors
/// [`PrivateFileError`] describing the refusal, never the contents.
pub fn read_private_file(path: &Path, max: u64) -> Result<String, PrivateFileError> {
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let fd =
        rustix::fs::open(path, flags, Mode::empty()).map_err(|_| PrivateFileError::Unreadable)?;
    let stat = rustix::fs::fstat(&fd).map_err(|_| PrivateFileError::Unreadable)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || stat.st_uid != rustix::process::geteuid().as_raw()
        || stat.st_mode & 0o077 != 0
    {
        return Err(PrivateFileError::NotPrivate);
    }
    let mut text = String::new();
    std::fs::File::from(fd)
        .take(max + 1)
        .read_to_string(&mut text)
        .map_err(|_| PrivateFileError::Unreadable)?;
    if text.len() as u64 > max {
        return Err(PrivateFileError::TooLarge);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn only_private_regular_files_are_read() {
        let dir = std::env::temp_dir().join(format!("rms-private-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap_or_default();
        let file = dir.join("grant");
        std::fs::write(&file, "fictional-capability").unwrap_or_default();
        let set = |mode| std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode));
        set(0o600).unwrap_or_default();
        assert_eq!(
            read_private_file(&file, 64).as_deref(),
            Ok("fictional-capability")
        );
        assert_eq!(read_private_file(&file, 4), Err(PrivateFileError::TooLarge));
        set(0o640).unwrap_or_default();
        assert_eq!(
            read_private_file(&file, 64),
            Err(PrivateFileError::NotPrivate)
        );
        set(0o600).unwrap_or_default();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&file, &link).unwrap_or_default();
        assert_eq!(
            read_private_file(&link, 64),
            Err(PrivateFileError::Unreadable)
        );
        assert_eq!(
            read_private_file(&dir, 64),
            Err(PrivateFileError::NotPrivate)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
