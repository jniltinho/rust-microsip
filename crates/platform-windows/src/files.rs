//! Owner-only files and directories (design W6): the Windows meaning of the
//! Unix uid and `0600`/`0700` mode checks.
//!
//! - *Owned by this user*: the owner SID is the token user, or the token's
//!   default owner (BUILTIN\Administrators for an elevated administrator,
//!   which is how Windows assigns ownership of objects that process creates).
//! - *Private*: every allow entry of the DACL names this user, SYSTEM or
//!   BUILTIN\Administrators. The last two keep access by privilege, as root
//!   does on Unix. A missing (NULL) DACL, an allow entry for anyone else
//!   (Everyone, Users, Authenticated Users, another user) or an entry type
//!   this module does not understand is refused. Deny entries only restrict
//!   and are accepted.
//!
//! Paths are opened once without following symbolic links or junctions
//! (`FILE_FLAG_OPEN_REPARSE_POINT`), and every check runs on that handle, so a
//! path swapped between check and read cannot be read.
// Remove once lib.rs re-exports make_private_dir and write_private_file.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::mem::offset_of;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Component, Path, PathBuf};
use std::ptr;
use windows_sys::Win32::Foundation::{ERROR_SUCCESS, HANDLE, HLOCAL, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
    SE_FILE_OBJECT, SetSecurityInfo,
};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, CreateWellKnownSid, DACL_SECURITY_INFORMATION, EqualSid,
    GetAce, GetSecurityDescriptorControl, GetSecurityDescriptorDacl, GetTokenInformation,
    OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    SE_DACL_PROTECTED, TOKEN_INFORMATION_CLASS, TOKEN_QUERY, TokenOwner, TokenUser,
    WELL_KNOWN_SID_TYPE, WinBuiltinAdministratorsSid, WinLocalSystemSid,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_GENERIC_READ, FILE_GENERIC_WRITE, READ_CONTROL, WRITE_DAC,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

// From Win32_System_SystemServices (feature not enabled for two constants).
const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
const ACCESS_DENIED_ACE_TYPE: u8 = 1;
/// SECURITY_MAX_SID_SIZE (68 bytes), rounded up to whole `u64` words.
const MAX_SID_WORDS: usize = 9;

/// Why a private file was refused.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PrivateFileError {
    /// Missing, unreadable, or a symbolic link / junction (reparse point).
    #[error("the file cannot be read")]
    Unreadable,
    /// Not a regular file owned by this user with an owner-only access list.
    #[error("the file must be a private regular file of this user")]
    NotPrivate,
    /// Larger than the limit.
    #[error("the file is too large")]
    TooLarge,
}

/// Read up to `max` bytes of UTF-8 from `path`, only if it is a regular file
/// owned by this user whose DACL grants access to nobody else (see the module
/// docs for SYSTEM and Administrators).
///
/// # Errors
/// [`PrivateFileError`] describing the refusal, never the contents.
pub fn read_private_file(path: &Path, max: u64) -> Result<String, PrivateFileError> {
    let bytes = read_checked(path, max, true)?;
    String::from_utf8(bytes).map_err(|_| PrivateFileError::Unreadable)
}

/// Read up to `max` bytes from `path` if it is a regular file owned by this
/// user (any DACL), with the same single no-follow open as
/// [`read_private_file`].
///
/// # Errors
/// [`PrivateFileError`]; [`PrivateFileError::Unreadable`] also covers a
/// missing file.
pub fn read_own_file(path: &Path, max: u64) -> Result<Vec<u8>, PrivateFileError> {
    read_checked(path, max, false)
}

/// Create `path` (and missing parents) and give it a protected DACL with one
/// entry: full control for the current user, inherited by new files and
/// subdirectories. Inherited entries from the parent are removed and the new
/// list propagates to existing children. SYSTEM and Administrators keep
/// access by privilege only.
///
/// The directory is opened without following a junction or symbolic link,
/// must be owned by this user, and the result is read back: owner, a
/// protected DACL and owner-only entries are all verified.
///
/// # Errors
/// Any I/O failure; [`io::ErrorKind::PermissionDenied`] when the path is a
/// reparse point, not a directory, owned by someone else, or the DACL cannot
/// be verified after setting it. The caller names the directory.
pub fn make_private_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)?;
    let dir = open_no_follow(path, READ_CONTROL | WRITE_DAC)?;
    let meta = dir.metadata()?;
    if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || !meta.is_dir() {
        return Err(not_private("the path is not a plain directory"));
    }
    let id = Identity::current()?;
    if !id.is_self(Security::of(&dir)?.owner) {
        return Err(not_private("the directory is owned by another user"));
    }
    set_private_dacl(&dir, &id, true)?;
    verify_private(&dir, &id)
}

/// Create the new file `name` inside `dir` (normally made by
/// [`make_private_dir`]), give it a protected owner-only DACL before any
/// content is written, then write `content` and flush it to disk.
///
/// # Errors
/// [`io::ErrorKind::InvalidInput`] when `name` is not a single file name,
/// [`io::ErrorKind::AlreadyExists`] when the file exists (it is never
/// replaced), [`io::ErrorKind::PermissionDenied`] when the DACL cannot be
/// verified (the empty file is then removed), or any other I/O failure.
pub fn write_private_file(dir: &Path, name: &str, content: &[u8]) -> io::Result<PathBuf> {
    let mut components = Path::new(name).components();
    if !matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(_)), None)
    ) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the file name must be a single path component",
        ));
    }
    let path = dir.join(name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .access_mode(FILE_GENERIC_WRITE | READ_CONTROL | WRITE_DAC)
        .share_mode(0)
        .open(&path)?;
    let protect = Identity::current().and_then(|id| {
        set_private_dacl(&file, &id, false)?;
        verify_private(&file, &id)
    });
    if let Err(error) = protect {
        drop(file);
        let _ = std::fs::remove_file(&path);
        return Err(error);
    }
    file.write_all(content)?;
    file.sync_all()?;
    Ok(path)
}

fn read_checked(path: &Path, max: u64, private: bool) -> Result<Vec<u8>, PrivateFileError> {
    let file = open_no_follow(path, FILE_GENERIC_READ).map_err(|_| PrivateFileError::Unreadable)?;
    let meta = file.metadata().map_err(|_| PrivateFileError::Unreadable)?;
    if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(PrivateFileError::Unreadable);
    }
    if !meta.is_file() {
        return Err(PrivateFileError::NotPrivate);
    }
    let id = Identity::current().map_err(|_| PrivateFileError::Unreadable)?;
    let security = Security::of(&file).map_err(|_| PrivateFileError::Unreadable)?;
    if !id.is_self(security.owner) || (private && !security.owner_only(&id)) {
        return Err(PrivateFileError::NotPrivate);
    }
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| PrivateFileError::Unreadable)?;
    if bytes.len() as u64 > max {
        return Err(PrivateFileError::TooLarge);
    }
    Ok(bytes)
}

/// Open an existing file or directory without following a reparse point.
fn open_no_follow(path: &Path, access: u32) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(access)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
}

fn not_private(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message.to_owned())
}

/// Replace the DACL of `object` with a protected one granting full control to
/// the current user only (inheritable for directories).
fn set_private_dacl(object: &File, id: &Identity, inherit: bool) -> io::Result<()> {
    let flags = if inherit { "OICI" } else { "" };
    let descriptor = Descriptor::from_sddl(&format!("D:P(A;{flags};FA;;;{})", id.user))?;
    let dacl = descriptor.dacl()?;
    // SAFETY: the handle is open with WRITE_DAC; `dacl` points into
    // `descriptor`, alive for the call; owner, group and SACL are not set.
    let error = unsafe {
        SetSecurityInfo(
            raw(object),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            dacl,
            ptr::null(),
        )
    };
    win32(error)
}

/// Read back owner, protection and entries of `object`.
fn verify_private(object: &File, id: &Identity) -> io::Result<()> {
    let security = Security::of(object)?;
    if !id.is_self(security.owner) {
        Err(not_private("readback: the owner is not this user"))
    } else if !security.protected()? {
        Err(not_private("readback: the access list is not protected"))
    } else if !security.owner_only(id) {
        Err(not_private(
            "readback: the access list grants access to others",
        ))
    } else {
        Ok(())
    }
}

fn raw(file: &File) -> HANDLE {
    file.as_raw_handle()
}

fn win32(error: u32) -> io::Result<()> {
    if error == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(error as i32))
    }
}

/// Memory allocated by the system with `LocalAlloc`, freed on drop.
struct LocalBox(HLOCAL);

impl Drop for LocalBox {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the pointer came from a Win32 call that allocates with
            // LocalAlloc and is freed exactly once here.
            unsafe { LocalFree(self.0) };
        }
    }
}

/// A SID in an owned, 8-byte aligned buffer (`offset` bytes from its start).
struct Sid {
    buf: Vec<u64>,
    offset: usize,
}

impl Sid {
    fn as_psid(&self) -> PSID {
        self.buf
            .as_ptr()
            .cast::<u8>()
            .wrapping_add(self.offset)
            .cast_mut()
            .cast()
    }

    fn is(&self, other: PSID) -> bool {
        // SAFETY: both pointers refer to valid SIDs alive for the call
        // (`other` from a live security descriptor); EqualSid only reads.
        !other.is_null() && unsafe { EqualSid(self.as_psid(), other) } != 0
    }

    fn well_known(kind: WELL_KNOWN_SID_TYPE) -> io::Result<Self> {
        let mut buf = vec![0u64; MAX_SID_WORDS];
        let mut size = (MAX_SID_WORDS * 8) as u32;
        // SAFETY: `buf` is writable for `size` bytes; no domain SID is needed
        // for the local well-known SIDs used here.
        let ok = unsafe {
            CreateWellKnownSid(kind, ptr::null_mut(), buf.as_mut_ptr().cast(), &mut size)
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { buf, offset: 0 })
    }

    /// The SID of `TokenUser` or `TokenOwner` of this process.
    fn from_token(class: TOKEN_INFORMATION_CLASS) -> io::Result<Self> {
        // SAFETY: GetCurrentProcess returns a pseudo handle; it has no
        // preconditions and needs no closing.
        let process = unsafe { GetCurrentProcess() };
        let mut token: HANDLE = ptr::null_mut();
        // SAFETY: `token` is a valid out pointer.
        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: OpenProcessToken succeeded, so `token` is an open handle
        // that we own and close exactly once.
        let token = unsafe { OwnedHandle::from_raw_handle(token) };
        // 1 KiB: TOKEN_USER / TOKEN_OWNER plus one SID (at most 68 bytes).
        let mut buf = vec![0u64; 128];
        let mut len = 0u32;
        // SAFETY: the token is open with TOKEN_QUERY and `buf` is writable
        // for the size passed.
        let ok = unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                class,
                buf.as_mut_ptr().cast(),
                (buf.len() * 8) as u32,
                &mut len,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        // TOKEN_USER and TOKEN_OWNER both start with the SID pointer, which
        // points into `buf`.
        // SAFETY: on success `buf` (8-byte aligned) starts with that pointer.
        let sid = unsafe { buf.as_ptr().cast::<PSID>().read() };
        let offset = (sid as usize)
            .checked_sub(buf.as_ptr() as usize)
            .filter(|offset| offset + 8 <= len as usize)
            .ok_or_else(|| io::Error::other("unexpected token information layout"))?;
        Ok(Self { buf, offset })
    }

    fn bytes(&self) -> Vec<u8> {
        let all: Vec<u8> = self.buf.iter().flat_map(|w| w.to_ne_bytes()).collect();
        all.get(self.offset..).unwrap_or_default().to_vec()
    }
}

/// `S-1-<authority>-<sub>...`, as ConvertSidToStringSidW prints it.
impl std::fmt::Display for Sid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let b = self.bytes();
        let count = usize::from(*b.get(1).ok_or(std::fmt::Error)?);
        let authority = b
            .get(2..8)
            .ok_or(std::fmt::Error)?
            .iter()
            .fold(0u64, |acc, byte| (acc << 8) | u64::from(*byte));
        write!(f, "S-{}-", b[0])?;
        if authority < 1 << 32 {
            write!(f, "{authority}")?;
        } else {
            write!(f, "0x{authority:012X}")?;
        }
        for i in 0..count {
            let start = 8 + 4 * i;
            let sub = b.get(start..start + 4).ok_or(std::fmt::Error)?;
            let value = u32::from_le_bytes([sub[0], sub[1], sub[2], sub[3]]);
            write!(f, "-{value}")?;
        }
        Ok(())
    }
}

/// The SIDs that decide ownership and privacy for this process.
struct Identity {
    user: Sid,
    default_owner: Sid,
    system: Sid,
    administrators: Sid,
}

impl Identity {
    fn current() -> io::Result<Self> {
        Ok(Self {
            user: Sid::from_token(TokenUser)?,
            default_owner: Sid::from_token(TokenOwner)?,
            system: Sid::well_known(WinLocalSystemSid)?,
            administrators: Sid::well_known(WinBuiltinAdministratorsSid)?,
        })
    }

    fn is_self(&self, sid: PSID) -> bool {
        self.user.is(sid) || self.default_owner.is(sid)
    }

    fn may_access(&self, sid: PSID) -> bool {
        self.is_self(sid) || self.system.is(sid) || self.administrators.is(sid)
    }
}

/// Owner and DACL of an open object; the pointers live in `_descriptor`.
struct Security {
    descriptor: LocalBox,
    owner: PSID,
    dacl: *const ACL,
}

impl Security {
    fn of(object: &File) -> io::Result<Self> {
        let mut owner: PSID = ptr::null_mut();
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: the handle is open with READ_CONTROL and every out pointer
        // is valid; the descriptor is released by LocalBox.
        let error = unsafe {
            GetSecurityInfo(
                raw(object),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        let descriptor = LocalBox(descriptor);
        win32(error)?;
        Ok(Self {
            descriptor,
            owner,
            dacl,
        })
    }

    fn protected(&self) -> io::Result<bool> {
        let (mut control, mut revision) = (0u16, 0u32);
        // SAFETY: the descriptor is valid while `self` lives; out pointers are valid.
        let ok =
            unsafe { GetSecurityDescriptorControl(self.descriptor.0, &mut control, &mut revision) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(control & SE_DACL_PROTECTED != 0)
    }

    /// Every allow entry names this user, SYSTEM or Administrators.
    fn owner_only(&self, id: &Identity) -> bool {
        if self.dacl.is_null() {
            return false; // A NULL DACL grants everyone full access.
        }
        // SAFETY: a non-null DACL from GetSecurityInfo points into the
        // descriptor, valid while `self` lives.
        let count = unsafe { (*self.dacl).AceCount };
        (0..u32::from(count)).all(|index| {
            let mut ace: *mut core::ffi::c_void = ptr::null_mut();
            // SAFETY: `index` is below the entry count of a valid ACL.
            if unsafe { GetAce(self.dacl, index, &mut ace) } == 0 || ace.is_null() {
                return false;
            }
            // SAFETY: GetAce returned an entry inside the ACL; every entry
            // starts with an ACE_HEADER.
            let header = unsafe { ace.cast::<ACE_HEADER>().read_unaligned() };
            match header.AceType {
                ACCESS_DENIED_ACE_TYPE => true,
                ACCESS_ALLOWED_ACE_TYPE => {
                    let sid = ace
                        .cast::<u8>()
                        .wrapping_add(offset_of!(ACCESS_ALLOWED_ACE, SidStart))
                        .cast();
                    id.may_access(sid)
                }
                _ => false,
            }
        })
    }
}

/// A self-relative security descriptor built from SDDL.
struct Descriptor(LocalBox);

impl Descriptor {
    fn from_sddl(sddl: &str) -> io::Result<Self> {
        let text: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: `text` is NUL-terminated; `descriptor` is a valid out
        // pointer and is released by LocalBox.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(LocalBox(descriptor)))
    }

    /// The DACL inside the descriptor (valid while `self` lives).
    fn dacl(&self) -> io::Result<*const ACL> {
        let (mut present, mut defaulted) = (0, 0);
        let mut dacl: *mut ACL = ptr::null_mut();
        // SAFETY: the descriptor is valid; out pointers are valid.
        let ok =
            unsafe { GetSecurityDescriptorDacl(self.0.0, &mut present, &mut dacl, &mut defaulted) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        if present == 0 || dacl.is_null() {
            return Err(io::Error::other("the descriptor has no DACL"));
        }
        Ok(dacl)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rms-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn well_known_sid_text() {
        let system = Sid::well_known(WinLocalSystemSid).map(|s| s.to_string());
        assert_eq!(system.ok().as_deref(), Some("S-1-5-18"));
        let admins = Sid::well_known(WinBuiltinAdministratorsSid).map(|s| s.to_string());
        assert_eq!(admins.ok().as_deref(), Some("S-1-5-32-544"));
        let user = Sid::from_token(TokenUser).map(|s| s.to_string());
        assert!(user.is_ok_and(|s| s.starts_with("S-1-5-")));
    }

    #[test]
    fn private_dir_and_file_round_trip() {
        let dir = scratch("private");
        let made = make_private_dir(&dir.join("nested"));
        assert!(made.is_ok(), "{made:?}");
        let dir = dir.join("nested");
        // Idempotent on an existing private directory.
        let again = make_private_dir(&dir);
        assert!(again.is_ok(), "{again:?}");
        let id = Identity::current();
        let opened = open_no_follow(&dir, READ_CONTROL);
        assert!(matches!((&id, &opened), (Ok(id), Ok(d)) if verify_private(d, id).is_ok()));

        let file = write_private_file(&dir, "grant", b"fictional-capability");
        let file = file.unwrap_or_default();
        assert_eq!(
            read_private_file(&file, 64).as_deref(),
            Ok("fictional-capability")
        );
        assert_eq!(read_private_file(&file, 4), Err(PrivateFileError::TooLarge));
        assert_eq!(
            read_own_file(&file, 64).as_deref(),
            Ok(&b"fictional-capability"[..])
        );
        assert_eq!(
            write_private_file(&dir, "grant", b"x").map_err(|e| e.kind()),
            Err(io::ErrorKind::AlreadyExists)
        );
        for bad in ["..", "a/b", "a\\b", ""] {
            assert_eq!(
                write_private_file(&dir, bad, b"x").map_err(|e| e.kind()),
                Err(io::ErrorKind::InvalidInput),
                "{bad:?}"
            );
        }
        assert_eq!(
            read_private_file(&dir, 64),
            Err(PrivateFileError::NotPrivate)
        );
        assert_eq!(
            read_private_file(&dir.join("missing"), 64),
            Err(PrivateFileError::Unreadable)
        );
        let _ = std::fs::remove_dir_all(dir.parent().unwrap_or(&dir));
    }

    #[test]
    fn world_readable_file_is_not_private() {
        let dir = scratch("public");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("grant");
        let _ = std::fs::write(&file, "fictional-capability");
        let shared = Identity::current().and_then(|id| {
            // Owner full control plus read for Everyone (WD).
            let sddl = format!("D:P(A;;FA;;;{})(A;;FR;;;WD)", id.user);
            let descriptor = Descriptor::from_sddl(&sddl)?;
            let dacl = descriptor.dacl()?;
            let handle = open_no_follow(&file, READ_CONTROL | WRITE_DAC)?;
            // SAFETY: as in `set_private_dacl`.
            let error = unsafe {
                SetSecurityInfo(
                    raw(&handle),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    dacl,
                    ptr::null(),
                )
            };
            win32(error)
        });
        assert!(shared.is_ok(), "{shared:?}");
        assert_eq!(
            read_private_file(&file, 64),
            Err(PrivateFileError::NotPrivate)
        );
        assert_eq!(
            read_own_file(&file, 64).as_deref(),
            Ok(&b"fictional-capability"[..])
        );
        // A symbolic link is never followed (creating one may need a privilege).
        let link = dir.join("link");
        if std::os::windows::fs::symlink_file(&file, &link).is_ok() {
            assert_eq!(read_own_file(&link, 64), Err(PrivateFileError::Unreadable));
            assert!(make_private_dir(&link).is_err());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
