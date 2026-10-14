//! SIP password storage in Windows Credential Manager (design W6, CFG-05).
//!
//! Each password is a generic credential with the target name
//! `rust-microsip:<account_ref>`, where `account_ref` is a stable non-secret
//! reference such as `user@domain`. Credentials persist with
//! `CRED_PERSIST_LOCAL_MACHINE`: they survive sign-out but never roam to other
//! machines. The password never appears in the target, comment, user name,
//! errors or `Debug` output.
//!
//! # Blocking
//!
//! Credential Manager calls are local and normally return at once; callers
//! still treat every operation as blocking for up to [`OPERATION_TIMEOUT`],
//! exactly as with the Secret Service on Unix.
//!
//! # Memory hygiene
//!
//! [`SecretStore::store`] hands the caller's bytes to `CredWriteW` without a
//! copy. [`SecretStore::retrieve`] copies the blob once into [`SecretBytes`]
//! and zeroes the system buffer before `CredFree`. Copies inside the
//! Credential Manager service are outside this module's control.

use std::io;
use std::iter::once;
use std::ptr;
use std::time::Duration;
use windows_sys::Win32::Foundation::{ERROR_NO_SUCH_LOGON_SESSION, ERROR_NOT_FOUND};
use windows_sys::Win32::Security::Credentials::{
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW,
    CredWriteW,
};

const TARGET_PREFIX: &str = "rust-microsip:";
const USER_NAME: &str = "rust-microsip";
const MAX_ACCOUNT_REF: usize = 256;
/// Credential Manager limits the comment to 256 UTF-16 code units
/// (`CRED_MAX_STRING_LENGTH`) including the NUL.
const MAX_COMMENT_UNITS: usize = 255;

/// Upper bound for one complete [`SecretStore`] operation, including connecting.
pub const OPERATION_TIMEOUT: Duration = Duration::from_secs(10);

/// Credential Manager failure. Never contains secret material.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SecretError {
    /// Credential Manager is unavailable (for example no logon session).
    #[error("Windows Credential Manager is unavailable")]
    Unavailable,
    /// The store is locked (never produced on Windows; kept for API parity).
    #[error("the keyring is locked")]
    Locked,
    /// No default store (never produced on Windows; kept for API parity).
    #[error("there is no default keyring")]
    NoDefaultCollection,
    /// No answer in time (never produced on Windows; kept for API parity).
    #[error("Windows Credential Manager did not respond in time")]
    Timeout,
    /// The account reference is empty, too long or contains control characters.
    #[error("invalid account reference")]
    InvalidAccount,
    /// Credential Manager returned an unexpected error; holds only the Win32 code.
    #[error("Windows Credential Manager error: {0}")]
    Service(String),
}

/// Secret bytes that are overwritten with zeros when dropped and redacted in `Debug`.
pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    /// Borrow the secret; avoid keeping copies.
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretBytes(<redacted>)")
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        for byte in &mut self.0 {
            // SAFETY: `byte` is a valid, aligned, exclusive reference to an
            // initialized `u8`; a volatile write is not elided.
            unsafe { ptr::write_volatile(byte, 0) };
        }
        // Keep the zeroing ordered before the deallocation that follows.
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}

/// Handle to the user's Credential Manager. Operations block; see the module docs.
#[derive(Clone, Debug)]
pub struct SecretStore;

impl SecretStore {
    /// Open the user's Credential Manager (always present on Windows).
    ///
    /// # Errors
    /// None today; the `Result` keeps the Unix signature.
    pub fn connect() -> Result<Self, SecretError> {
        Ok(Self)
    }

    /// Same as [`SecretStore::connect`]; the address is a Unix (D-Bus) concept
    /// and is ignored on Windows.
    ///
    /// # Errors
    /// None today; the `Result` keeps the Unix signature.
    pub fn with_address(_address: &str) -> Result<Self, SecretError> {
        Self::connect()
    }

    /// Create or replace the password for `account_ref`.
    ///
    /// `label` becomes the credential comment shown in Credential Manager
    /// (truncated to its limit) and must not contain the secret.
    ///
    /// # Errors
    /// [`SecretError::InvalidAccount`] for a bad reference,
    /// [`SecretError::Unavailable`] without a logon session, otherwise
    /// [`SecretError::Service`] with the Win32 error code.
    pub fn store(&self, account_ref: &str, label: &str, secret: &[u8]) -> Result<(), SecretError> {
        let mut target = target(account_ref)?;
        let mut comment = wide(truncate_utf16(label, MAX_COMMENT_UNITS));
        let mut user = wide(USER_NAME);
        let size = u32::try_from(secret.len())
            .map_err(|_| SecretError::Service("the secret is too large".into()))?;
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            Comment: comment.as_mut_ptr(),
            CredentialBlobSize: size,
            // CredWriteW only reads the blob.
            CredentialBlob: secret.as_ptr().cast_mut(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: user.as_mut_ptr(),
            ..CREDENTIALW::default()
        };
        // SAFETY: every pointer in `credential` refers to a NUL-terminated
        // buffer or to `secret` of `size` bytes, all alive for this call;
        // CredWriteW does not keep them.
        if unsafe { CredWriteW(&credential, 0) } == 0 {
            return Err(last_error());
        }
        Ok(())
    }

    /// Read the password for `account_ref`; `Ok(None)` when none is stored.
    ///
    /// # Errors
    /// As [`SecretStore::store`].
    pub fn retrieve(&self, account_ref: &str) -> Result<Option<SecretBytes>, SecretError> {
        let target = target(account_ref)?;
        let mut raw: *mut CREDENTIALW = ptr::null_mut();
        // SAFETY: `target` is NUL-terminated and `raw` is a valid out pointer.
        if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut raw) } == 0 {
            return match io::Error::last_os_error().raw_os_error() {
                Some(code) if code == ERROR_NOT_FOUND as i32 => Ok(None),
                code => Err(map_code(code)),
            };
        }
        let credential = Credential(raw);
        let blob = credential.blob();
        let mut secret = Vec::with_capacity(blob.len());
        secret.extend_from_slice(blob);
        Ok(Some(SecretBytes(secret)))
    }

    /// Delete the password for `account_ref`; succeeds when none exists.
    ///
    /// # Errors
    /// As [`SecretStore::store`].
    pub fn delete(&self, account_ref: &str) -> Result<(), SecretError> {
        let target = target(account_ref)?;
        // SAFETY: `target` is NUL-terminated and alive for the call.
        if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
            return match io::Error::last_os_error().raw_os_error() {
                Some(code) if code == ERROR_NOT_FOUND as i32 => Ok(()),
                code => Err(map_code(code)),
            };
        }
        Ok(())
    }
}

/// A credential returned by `CredReadW`; its blob is zeroed and the buffer
/// released with `CredFree` on drop.
struct Credential(*mut CREDENTIALW);

impl Credential {
    fn blob(&self) -> &[u8] {
        // SAFETY: `self.0` is the non-null result of a successful CredReadW,
        // valid until CredFree in `drop`.
        let credential = unsafe { &*self.0 };
        if credential.CredentialBlob.is_null() || credential.CredentialBlobSize == 0 {
            return &[];
        }
        // SAFETY: Credential Manager guarantees `CredentialBlobSize` readable
        // bytes at `CredentialBlob`, owned by this allocation.
        unsafe {
            std::slice::from_raw_parts(
                credential.CredentialBlob,
                credential.CredentialBlobSize as usize,
            )
        }
    }
}

impl Drop for Credential {
    fn drop(&mut self) {
        // SAFETY: as in `blob`; the allocation is still alive.
        let credential = unsafe { &*self.0 };
        if !credential.CredentialBlob.is_null() {
            // SAFETY: the blob is `CredentialBlobSize` writable bytes of our
            // own CredReadW allocation.
            unsafe {
                ptr::write_bytes(
                    credential.CredentialBlob,
                    0,
                    credential.CredentialBlobSize as usize,
                );
            }
        }
        // SAFETY: `self.0` came from CredReadW and is freed exactly once.
        unsafe { CredFree(self.0.cast_const().cast()) };
    }
}

fn validate(account_ref: &str) -> Result<(), SecretError> {
    if account_ref.trim().is_empty()
        || account_ref.len() > MAX_ACCOUNT_REF
        || account_ref.chars().any(char::is_control)
    {
        return Err(SecretError::InvalidAccount);
    }
    Ok(())
}

fn target(account_ref: &str) -> Result<Vec<u16>, SecretError> {
    validate(account_ref)?;
    Ok(wide(&format!("{TARGET_PREFIX}{account_ref}")))
}

/// The longest prefix of `text` that fits in `max` UTF-16 code units, never
/// splitting a character (and so never a surrogate pair).
fn truncate_utf16(text: &str, max: usize) -> &str {
    let mut units = 0;
    for (index, c) in text.char_indices() {
        units += c.len_utf16();
        if units > max {
            return &text[..index];
        }
    }
    text
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(once(0)).collect()
}

fn last_error() -> SecretError {
    map_code(io::Error::last_os_error().raw_os_error())
}

fn map_code(code: Option<i32>) -> SecretError {
    match code {
        Some(code) if code == ERROR_NO_SUCH_LOGON_SESSION as i32 => SecretError::Unavailable,
        Some(code) => SecretError::Service(format!("Win32 error {code}")),
        None => SecretError::Service("unknown error".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_references_are_validated() {
        for bad in ["", "  ", "a\nb", "a\0b", &"x".repeat(MAX_ACCOUNT_REF + 1)] {
            assert_eq!(validate(bad), Err(SecretError::InvalidAccount), "{bad:?}");
        }
        assert_eq!(validate("alice@pbx.example.invalid"), Ok(()));
        let store = SecretStore;
        assert_eq!(store.store("", "x", b"x"), Err(SecretError::InvalidAccount));
        assert_eq!(
            store.retrieve("a\nb").err(),
            Some(SecretError::InvalidAccount)
        );
        assert_eq!(store.delete(" "), Err(SecretError::InvalidAccount));
    }

    #[test]
    fn comments_are_truncated_by_utf16_units() {
        assert_eq!(truncate_utf16("abc", 255), "abc");
        let ascii = "x".repeat(300);
        assert_eq!(truncate_utf16(&ascii, 255).len(), 255);
        // Each U+1F600 is a surrogate pair: 127 fit in 254 units, the 128th
        // would need 256.
        let emoji = "\u{1F600}".repeat(200);
        let cut = truncate_utf16(&emoji, 255);
        assert_eq!(cut.encode_utf16().count(), 254);
        assert_eq!(cut.chars().count(), 127);
        assert!(wide(cut).len() <= 256);
    }

    #[test]
    fn secrets_are_redacted() {
        let secret = SecretBytes(b"fictional-password".to_vec());
        assert_eq!(format!("{secret:?}"), "SecretBytes(<redacted>)");
    }

    #[test]
    fn credential_round_trip_and_delete() {
        let account = format!("rms-test-{}@pbx.example.invalid", std::process::id());
        let store = SecretStore::connect().unwrap_or(SecretStore);
        let label = format!("rust-microsip test ({account})");
        assert_eq!(store.store(&account, &label, b"fictional-1"), Ok(()));
        assert_eq!(store.store(&account, &label, b"fictional-2"), Ok(()));
        let read = store.retrieve(&account);
        assert_eq!(
            read.as_ref()
                .ok()
                .and_then(|s| s.as_ref())
                .map(SecretBytes::expose),
            Some(&b"fictional-2"[..])
        );
        assert_eq!(store.delete(&account), Ok(()));
        assert!(matches!(store.retrieve(&account), Ok(None)));
        assert_eq!(store.delete(&account), Ok(()));
    }
}
