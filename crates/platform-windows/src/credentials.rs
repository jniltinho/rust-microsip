//! Credential Manager password storage (design W6).
// ponytail: stand-in until W-B task 2.5.

use std::time::Duration;
// --- secrets ---------------------------------------------------------------

/// How long a keyring operation may take.
pub const OPERATION_TIMEOUT: Duration = Duration::from_secs(10);

/// Keyring failure. Never contains secret material.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SecretError {
    /// No keyring available.
    #[error("the keyring is unavailable")]
    Unavailable,
    /// The keyring is locked.
    #[error("the keyring is locked")]
    Locked,
    /// No default keyring.
    #[error("there is no default keyring")]
    NoDefaultCollection,
    /// No answer in time.
    #[error("the keyring did not respond in time")]
    Timeout,
    /// Invalid account reference.
    #[error("invalid account reference")]
    InvalidAccount,
    /// Other keyring error.
    #[error("keyring error: {0}")]
    Service(String),
}

/// Secret bytes, redacted in `Debug` and wiped on drop.
pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    /// Borrow the secret.
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
        self.0.fill(0);
        std::hint::black_box(&mut self.0);
    }
}

/// Keyring handle (Credential Manager arrives in milestone W-B).
#[derive(Clone, Debug)]
pub struct SecretStore;

impl SecretStore {
    /// Connect to the user's keyring.
    ///
    /// # Errors
    /// Always [`SecretError::Unavailable`] on Windows for now.
    pub fn connect() -> Result<Self, SecretError> {
        Err(SecretError::Unavailable)
    }
    /// Connect to an explicit address (tests).
    ///
    /// # Errors
    /// Always [`SecretError::Unavailable`] on Windows for now.
    pub fn with_address(_address: &str) -> Result<Self, SecretError> {
        Err(SecretError::Unavailable)
    }
    /// Store a secret.
    ///
    /// # Errors
    /// Always [`SecretError::Unavailable`].
    pub fn store(
        &self,
        _account_ref: &str,
        _label: &str,
        _secret: &[u8],
    ) -> Result<(), SecretError> {
        Err(SecretError::Unavailable)
    }
    /// Read a secret.
    ///
    /// # Errors
    /// Always [`SecretError::Unavailable`].
    pub fn retrieve(&self, _account_ref: &str) -> Result<Option<SecretBytes>, SecretError> {
        Err(SecretError::Unavailable)
    }
    /// Delete a secret.
    ///
    /// # Errors
    /// Always [`SecretError::Unavailable`].
    pub fn delete(&self, _account_ref: &str) -> Result<(), SecretError> {
        Err(SecretError::Unavailable)
    }
}
