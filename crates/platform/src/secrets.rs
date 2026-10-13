//! SIP password storage in the freedesktop Secret Service (`org.freedesktop.secrets`, CFG-05).
//!
//! Items live in the default collection (alias `default`) and are found by the
//! attributes `application = "rust-microsip"` and `account = <account_ref>`,
//! where `account_ref` is a stable non-secret reference such as `user@domain`.
//! The password never appears in attributes, labels, errors or `Debug` output.
//!
//! # Blocking
//!
//! Every operation blocks the caller for up to [`OPERATION_TIMEOUT`] (each
//! D-Bus call is also bounded by a 5 s method timeout). Call it at startup or
//! from a worker thread, never from the UI thread. D-Bus work runs on a helper
//! thread with zbus's own `async-io` executor (never Tokio).
//!
//! # Transport
//!
//! The session uses the `plain` algorithm: the secret crosses the local,
//! per-user session bus unencrypted. That bus is only reachable by the same
//! user, who could read the keyring anyway; a Diffie-Hellman
//! (`dh-ietf1024-sha256-aes128-cbc-pkcs7`) session is a later hardening step.
//!
//! # Locked keyrings
//!
//! Unlocking needs an interactive prompt from the keyring daemon. This module
//! never starts prompts: a locked collection or item yields
//! [`SecretError::Locked`] (any prompt the service returns is dismissed), and
//! callers fall back to a per-session password.
//!
//! # Memory hygiene
//!
//! Buffers owned here are overwritten with zeros before they are freed
//! ([`SecretBytes`]). Copies inside zbus message buffers and the keyring
//! daemon are outside this module's control.

use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, DBusError};

const SERVICE: &str = "org.freedesktop.secrets";
const SERVICE_PATH: &str = "/org/freedesktop/secrets";
const SERVICE_IFACE: &str = "org.freedesktop.Secret.Service";
const COLLECTION_IFACE: &str = "org.freedesktop.Secret.Collection";
const ITEM_IFACE: &str = "org.freedesktop.Secret.Item";
const SESSION_IFACE: &str = "org.freedesktop.Secret.Session";
const PROMPT_IFACE: &str = "org.freedesktop.Secret.Prompt";
const APPLICATION: &str = "rust-microsip";
const MAX_ACCOUNT_REF: usize = 256;
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// Upper bound for one complete [`SecretStore`] operation, including connecting.
pub const OPERATION_TIMEOUT: Duration = Duration::from_secs(10);

/// Secret Service failure. Never contains secret material.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SecretError {
    /// No session bus or no Secret Service provider.
    #[error("Secret Service is unavailable")]
    Unavailable,
    /// The collection or item is locked and unlocking needs a user prompt.
    #[error("the keyring is locked; unlock it in the desktop keyring and retry")]
    Locked,
    /// There is no default collection to store items in.
    #[error("the Secret Service has no default keyring")]
    NoDefaultCollection,
    /// The service did not answer in time.
    #[error("Secret Service did not respond in time")]
    Timeout,
    /// The account reference is empty, too long or contains control characters.
    #[error("invalid account reference")]
    InvalidAccount,
    /// The service returned an unexpected error; holds only the D-Bus error name.
    #[error("Secret Service error: {0}")]
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
        // ponytail: best-effort wipe without `unsafe`/zeroize (not a direct
        // dependency); switch to `zeroize::Zeroizing` if it becomes one.
        self.0.fill(0);
        std::hint::black_box(&mut self.0);
    }
}

/// Connection to the user's Secret Service. Operations block; see the module docs.
#[derive(Clone)]
pub struct SecretStore {
    conn: Connection,
}

impl std::fmt::Debug for SecretStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretStore").finish_non_exhaustive()
    }
}

impl SecretStore {
    /// Connect to the session bus (`DBUS_SESSION_BUS_ADDRESS`) and probe the service.
    ///
    /// Returns [`SecretError::Unavailable`] without a bus or Secret Service provider.
    pub fn connect() -> Result<Self, SecretError> {
        Self::open(None)
    }

    /// Connect to an explicit D-Bus address (for tests or sandboxed buses).
    pub fn with_address(address: &str) -> Result<Self, SecretError> {
        Self::open(Some(address.to_owned()))
    }

    fn open(address: Option<String>) -> Result<Self, SecretError> {
        run(async move {
            let builder = match address {
                Some(address) => zbus::connection::Builder::address(address.as_str()),
                None => zbus::connection::Builder::session(),
            }
            .map_err(|_| SecretError::Unavailable)?;
            let conn = builder
                .method_timeout(CALL_TIMEOUT)
                .build()
                .await
                .map_err(|_| SecretError::Unavailable)?;
            // Ping activates the provider if needed and fails when none exists.
            conn.call_method(
                Some(SERVICE),
                SERVICE_PATH,
                Some("org.freedesktop.DBus.Peer"),
                "Ping",
                &(),
            )
            .await
            .map_err(|e| match map_error(&e) {
                SecretError::Timeout => SecretError::Timeout,
                _ => SecretError::Unavailable,
            })?;
            Ok(Self { conn })
        })
    }

    /// Create or replace the password for `account_ref` in the default collection.
    ///
    /// `label` is shown in keyring managers and must not contain the secret.
    pub fn store(&self, account_ref: &str, label: &str, secret: &[u8]) -> Result<(), SecretError> {
        validate(account_ref)?;
        let (conn, account, label) = (self.conn.clone(), account_ref.to_owned(), label.to_owned());
        let secret = SecretBytes(secret.to_vec());
        run(async move {
            let session = open_session(&conn).await?;
            let result = create_item(&conn, &session, &account, &label, &secret).await;
            close_session(&conn, &session).await;
            result
        })
    }

    /// Read the password for `account_ref`; `Ok(None)` when no item exists.
    pub fn retrieve(&self, account_ref: &str) -> Result<Option<SecretBytes>, SecretError> {
        validate(account_ref)?;
        let (conn, account) = (self.conn.clone(), account_ref.to_owned());
        run(async move {
            let (unlocked, locked) = search(&conn, &account).await?;
            let Some(item) = unlocked.first() else {
                return if locked.is_empty() {
                    Ok(None)
                } else {
                    Err(SecretError::Locked)
                };
            };
            let session = open_session(&conn).await?;
            let result = get_secret(&conn, &session, item).await.map(Some);
            close_session(&conn, &session).await;
            result
        })
    }

    /// Delete every stored password for `account_ref`; succeeds when none exists.
    pub fn delete(&self, account_ref: &str) -> Result<(), SecretError> {
        validate(account_ref)?;
        let (conn, account) = (self.conn.clone(), account_ref.to_owned());
        run(async move {
            let (unlocked, locked) = search(&conn, &account).await?;
            for item in &unlocked {
                let prompt: OwnedObjectPath = call(&conn, item, ITEM_IFACE, "Delete", &()).await?;
                if needs_prompt(&prompt) {
                    dismiss(&conn, &prompt).await;
                    return Err(SecretError::Locked);
                }
            }
            if locked.is_empty() {
                Ok(())
            } else {
                Err(SecretError::Locked)
            }
        })
    }
}

/// Run `op` on a helper thread with zbus's executor, bounded by [`OPERATION_TIMEOUT`].
///
/// On timeout the helper thread finishes on its own once its method timeout expires.
fn run<T: Send + 'static>(
    op: impl Future<Output = Result<T, SecretError>> + Send + 'static,
) -> Result<T, SecretError> {
    let (tx, rx) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("secret-service".into())
        .spawn(move || {
            let _ = tx.send(zbus::block_on(op));
        })
        .map_err(|_| SecretError::Unavailable)?;
    match rx.recv_timeout(OPERATION_TIMEOUT) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(SecretError::Timeout),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(SecretError::Service("internal failure".into()))
        }
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

/// Lookup attributes; they never contain the secret.
fn attributes(account_ref: &str) -> HashMap<&str, &str> {
    HashMap::from([("application", APPLICATION), ("account", account_ref)])
}

fn needs_prompt(prompt: &OwnedObjectPath) -> bool {
    prompt.as_str() != "/"
}

fn map_error(error: &zbus::Error) -> SecretError {
    match error {
        zbus::Error::MethodError(name, _, _) => map_name(name.as_str()),
        zbus::Error::FDO(e) => map_name(e.name().as_str()),
        zbus::Error::InputOutput(e) if e.kind() == std::io::ErrorKind::TimedOut => {
            SecretError::Timeout
        }
        zbus::Error::InputOutput(_) | zbus::Error::Address(_) | zbus::Error::Handshake(_) => {
            SecretError::Unavailable
        }
        _ => SecretError::Service("unexpected reply".into()),
    }
}

fn map_name(name: &str) -> SecretError {
    match name {
        "org.freedesktop.DBus.Error.ServiceUnknown"
        | "org.freedesktop.DBus.Error.NameHasNoOwner"
        | "org.freedesktop.DBus.Error.Disconnected" => SecretError::Unavailable,
        "org.freedesktop.DBus.Error.NoReply" | "org.freedesktop.DBus.Error.TimedOut" => {
            SecretError::Timeout
        }
        "org.freedesktop.Secret.Error.IsLocked" => SecretError::Locked,
        "org.freedesktop.Secret.Error.NoSuchObject" => SecretError::NoDefaultCollection,
        other => SecretError::Service(other.to_owned()),
    }
}

async fn call<B, R>(
    conn: &Connection,
    path: &OwnedObjectPath,
    iface: &str,
    method: &str,
    body: &B,
) -> Result<R, SecretError>
where
    B: zbus::export::serde::Serialize + zbus::zvariant::DynamicType,
    R: for<'d> zbus::zvariant::DynamicDeserialize<'d>,
{
    conn.call_method(Some(SERVICE), path.as_str(), Some(iface), method, body)
        .await
        .map_err(|e| map_error(&e))?
        .body()
        .deserialize()
        .map_err(|e| map_error(&e))
}

fn service_path() -> OwnedObjectPath {
    OwnedObjectPath::from(zbus::zvariant::ObjectPath::from_static_str_unchecked(
        SERVICE_PATH,
    ))
}

async fn open_session(conn: &Connection) -> Result<OwnedObjectPath, SecretError> {
    let (_, session): (OwnedValue, OwnedObjectPath) = call(
        conn,
        &service_path(),
        SERVICE_IFACE,
        "OpenSession",
        &("plain", Value::from("")),
    )
    .await?;
    Ok(session)
}

async fn close_session(conn: &Connection, session: &OwnedObjectPath) {
    let _ = call::<_, ()>(conn, session, SESSION_IFACE, "Close", &()).await;
}

async fn dismiss(conn: &Connection, prompt: &OwnedObjectPath) {
    let _ = call::<_, ()>(conn, prompt, PROMPT_IFACE, "Dismiss", &()).await;
}

async fn search(
    conn: &Connection,
    account: &str,
) -> Result<(Vec<OwnedObjectPath>, Vec<OwnedObjectPath>), SecretError> {
    call(
        conn,
        &service_path(),
        SERVICE_IFACE,
        "SearchItems",
        &(attributes(account),),
    )
    .await
}

async fn create_item(
    conn: &Connection,
    session: &OwnedObjectPath,
    account: &str,
    label: &str,
    secret: &SecretBytes,
) -> Result<(), SecretError> {
    let collection: OwnedObjectPath = call(
        conn,
        &service_path(),
        SERVICE_IFACE,
        "ReadAlias",
        &("default",),
    )
    .await?;
    if collection.as_str() == "/" {
        return Err(SecretError::NoDefaultCollection);
    }
    let mut properties: HashMap<&str, Value<'_>> = HashMap::new();
    properties.insert("org.freedesktop.Secret.Item.Label", Value::from(label));
    properties.insert(
        "org.freedesktop.Secret.Item.Attributes",
        Value::from(attributes(account)),
    );
    let empty: &[u8] = &[];
    let value = (session, empty, secret.expose(), "text/plain");
    let (_, prompt): (OwnedObjectPath, OwnedObjectPath) = call(
        conn,
        &collection,
        COLLECTION_IFACE,
        "CreateItem",
        &(properties, value, true),
    )
    .await?;
    if needs_prompt(&prompt) {
        dismiss(conn, &prompt).await;
        return Err(SecretError::Locked);
    }
    Ok(())
}

async fn get_secret(
    conn: &Connection,
    session: &OwnedObjectPath,
    item: &OwnedObjectPath,
) -> Result<SecretBytes, SecretError> {
    let ((_, parameters, value, _),): ((OwnedObjectPath, Vec<u8>, Vec<u8>, String),) =
        call(conn, item, ITEM_IFACE, "GetSecret", &(session,)).await?;
    drop(SecretBytes(parameters));
    Ok(SecretBytes(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn attributes_identify_account_without_secret() {
        let attrs = attributes("alice@pbx.invalid");
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs["application"], "rust-microsip");
        assert_eq!(attrs["account"], "alice@pbx.invalid");
    }

    #[test]
    fn account_ref_is_validated() {
        assert_eq!(validate("alice@pbx.invalid"), Ok(()));
        for bad in ["", "  ", "a\nb", "a\0b", &"x".repeat(MAX_ACCOUNT_REF + 1)] {
            assert_eq!(validate(bad), Err(SecretError::InvalidAccount), "{bad:?}");
        }
    }

    #[test]
    fn errors_are_mapped() {
        assert_eq!(
            map_name("org.freedesktop.Secret.Error.IsLocked"),
            SecretError::Locked
        );
        assert_eq!(
            map_name("org.freedesktop.DBus.Error.ServiceUnknown"),
            SecretError::Unavailable
        );
        assert_eq!(
            map_name("org.freedesktop.DBus.Error.NoReply"),
            SecretError::Timeout
        );
        assert_eq!(
            map_name("org.example.Odd"),
            SecretError::Service("org.example.Odd".into())
        );
        let timeout = zbus::Error::InputOutput(Arc::new(std::io::ErrorKind::TimedOut.into()));
        assert_eq!(map_error(&timeout), SecretError::Timeout);
        let gone = zbus::Error::InputOutput(Arc::new(std::io::ErrorKind::BrokenPipe.into()));
        assert_eq!(map_error(&gone), SecretError::Unavailable);
        let unknown = zbus::Error::FDO(Box::new(zbus::fdo::Error::ServiceUnknown("x".into())));
        assert_eq!(map_error(&unknown), SecretError::Unavailable);
    }

    #[test]
    fn missing_bus_is_unavailable() {
        let path = std::env::temp_dir().join(format!("rmsip-no-secrets-{}", std::process::id()));
        for address in [
            format!("unix:path={}", path.display()),
            "not-an-address".to_owned(),
        ] {
            assert_eq!(
                SecretStore::with_address(&address).map(drop),
                Err(SecretError::Unavailable),
                "{address}"
            );
        }
    }

    #[test]
    fn secrets_are_redacted() {
        let secret = SecretBytes(b"s3cret-marker".to_vec());
        assert_eq!(format!("{secret:?}"), "SecretBytes(<redacted>)");
        assert_eq!(secret.expose(), b"s3cret-marker");
        for e in [SecretError::Locked, SecretError::Service("x".into())] {
            assert!(!e.to_string().contains("s3cret"));
        }
    }

    /// Round trip against a real, unlocked session keyring with fictional data.
    #[test]
    #[ignore = "needs a session bus with an unlocked Secret Service"]
    fn real_session_round_trip() -> Result<(), SecretError> {
        let store = SecretStore::connect()?;
        let account = "rmsip-test@pbx.invalid";
        store.store(account, "rust-microsip test", b"fictional-secret")?;
        let got = store.retrieve(account)?;
        assert_eq!(
            got.as_ref().map(SecretBytes::expose),
            Some(&b"fictional-secret"[..])
        );
        store.delete(account)?;
        assert!(store.retrieve(account)?.is_none());
        Ok(())
    }
}
