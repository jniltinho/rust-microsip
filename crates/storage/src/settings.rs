//! User-edited settings: the single enabled account and UI/audio preferences.
//!
//! Values cross this boundary as strings keyed by the dotted configuration
//! keys of `rust-microsip-config` (`account.server`, `ui.theme`, ...); the
//! config crate parses and validates them as its SQLite layer. Only values the
//! user explicitly saved belong here, never the effective configuration that
//! includes command-line, environment or TOML overrides, and never a password:
//! the account row holds at most a `secret_ref` naming a keyring entry.

use crate::{InvalidField, Result, StorageError, Store, now_utc, text_ok};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::collections::BTreeMap;

/// Every settings key the store accepts, mirroring the configuration schema.
pub const CONFIG_KEYS: [&str; 16] = [
    "account.display_name",
    "account.username",
    "account.auth_username",
    "account.domain",
    "account.server",
    "account.port",
    "account.transport",
    "account.media",
    "account.expires",
    "account.password_env",
    "account.password_file",
    "account.ca_file",
    "ui.theme",
    "ui.dnd",
    "audio.input_device",
    "audio.output_device",
];

/// Longest accepted settings value, in bytes.
pub const MAX_CONFIG_VALUE_LEN: usize = 4096;
const MAX_SECRET_REF_LEN: usize = 512;

/// Account column for an `account.*` key (whitelisted by [`CONFIG_KEYS`]).
fn account_column(key: &str) -> Option<&str> {
    key.strip_prefix("account.")
}

/// Check the key and convert the value to its SQL form (`None` clears).
fn sql_value(key: &str, value: Option<&str>) -> Result<Value> {
    if !CONFIG_KEYS.contains(&key) {
        return Err(StorageError::Invalid(InvalidField::ConfigKey));
    }
    let Some(value) = value else {
        return Ok(Value::Null);
    };
    if value.len() > MAX_CONFIG_VALUE_LEN || value.chars().any(char::is_control) {
        return Err(StorageError::Invalid(InvalidField::ConfigValue));
    }
    Ok(match key {
        "account.port" | "account.expires" => Value::Integer(
            value
                .parse()
                .map_err(|_| StorageError::Invalid(InvalidField::ConfigValue))?,
        ),
        _ => Value::Text(value.to_owned()),
    })
}

/// Id of the enabled account, creating an empty one when there is none.
fn enabled_account(conn: &Connection) -> Result<i64> {
    let found = conn
        .query_row("SELECT id FROM accounts WHERE enabled = 1", [], |r| {
            r.get(0)
        })
        .optional()?;
    match found {
        Some(id) => Ok(id),
        None => {
            conn.execute(
                "INSERT INTO accounts (enabled, updated_utc) VALUES (1, ?1)",
                [now_utc()],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }
}

/// Validate everything first, then write; the caller owns the transaction.
fn write_values<'a>(
    conn: &Connection,
    changes: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
) -> Result<()> {
    let checked = changes
        .into_iter()
        .map(|(key, value)| Ok((key, sql_value(key, value)?)))
        .collect::<Result<Vec<_>>>()?;
    for (key, value) in checked {
        if let Some(column) = account_column(key) {
            let id = enabled_account(conn)?;
            // `column` comes from the CONFIG_KEYS whitelist checked above.
            conn.execute(
                &format!("UPDATE accounts SET {column} = ?1, updated_utc = ?2 WHERE id = ?3"),
                params![value, now_utc(), id],
            )?;
        } else if let Value::Text(text) = value {
            conn.execute(
                "INSERT INTO preferences (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, text],
            )?;
        } else {
            conn.execute("DELETE FROM preferences WHERE key = ?1", [key])?;
        }
    }
    Ok(())
}

impl Store {
    /// The stored settings layer: every non-NULL value of the enabled
    /// account plus the stored preferences, keyed by [`CONFIG_KEYS`].
    ///
    /// # Errors
    /// Returns a SQLite failure.
    pub fn load_config(&self) -> Result<BTreeMap<String, String>> {
        let keys: Vec<&str> = CONFIG_KEYS
            .into_iter()
            .filter(|k| account_column(k).is_some())
            .collect();
        let select = keys
            .iter()
            .filter_map(|k| account_column(k))
            .map(|c| format!("CAST({c} AS TEXT)"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut out = BTreeMap::new();
        let row: Option<Vec<Option<String>>> = self
            .conn
            .query_row(
                &format!("SELECT {select} FROM accounts WHERE enabled = 1"),
                [],
                |r| (0..keys.len()).map(|i| r.get(i)).collect(),
            )
            .optional()?;
        for (key, value) in keys.iter().zip(row.unwrap_or_default()) {
            if let Some(value) = value {
                out.insert((*key).to_owned(), value);
            }
        }
        let mut stmt = self.conn.prepare("SELECT key, value FROM preferences")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get(1)?)))?;
        for row in rows {
            let (key, value) = row?;
            if CONFIG_KEYS.contains(&key.as_str()) {
                out.insert(key, value);
            }
        }
        Ok(out)
    }

    /// Persist values the user explicitly edited; `None` removes a stored
    /// value. Other stored values are kept. All or nothing.
    ///
    /// Pass only real user edits that no higher-precedence source overrides;
    /// never the effective configuration.
    ///
    /// # Errors
    /// [`StorageError::Invalid`] with [`InvalidField::ConfigKey`] or
    /// [`InvalidField::ConfigValue`] (nothing written), [`StorageError::Busy`],
    /// [`StorageError::Full`] or other SQLite failures (rolled back).
    pub fn save_config(&mut self, changes: &BTreeMap<String, Option<String>>) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        write_values(&tx, changes.iter().map(|(k, v)| (k.as_str(), v.as_deref())))?;
        tx.commit()?;
        Ok(())
    }

    /// Replace all stored settings with `values` in one transaction (a
    /// confirmed import). The account's `secret_ref` is cleared too, because
    /// the imported account may be a different one.
    ///
    /// # Errors
    /// As [`Store::save_config`]; on any error the previous settings remain.
    pub fn replace_config(&mut self, values: &BTreeMap<String, String>) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let clear = CONFIG_KEYS.into_iter().map(|k| (k, None));
        write_values(&tx, clear)?;
        tx.execute(
            "UPDATE accounts SET secret_ref = NULL WHERE enabled = 1",
            [],
        )?;
        write_values(
            &tx,
            values.iter().map(|(k, v)| (k.as_str(), Some(v.as_str()))),
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Keyring reference of the enabled account's password, if stored.
    ///
    /// # Errors
    /// Returns a SQLite failure.
    pub fn secret_ref(&self) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT secret_ref FROM accounts WHERE enabled = 1",
                [],
                |r| r.get(0),
            )
            .optional()?
            .flatten())
    }

    /// Store or clear the keyring reference (never the password itself).
    ///
    /// # Errors
    /// [`StorageError::Invalid`] with [`InvalidField::SecretRef`], or SQLite failures.
    pub fn set_secret_ref(&mut self, reference: Option<&str>) -> Result<()> {
        if reference.is_some_and(|r| !text_ok(r, MAX_SECRET_REF_LEN)) {
            return Err(StorageError::Invalid(InvalidField::SecretRef));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = enabled_account(&tx)?;
        tx.execute(
            "UPDATE accounts SET secret_ref = ?1, updated_utc = ?2 WHERE id = ?3",
            params![reference, now_utc(), id],
        )?;
        tx.commit()?;
        Ok(())
    }
}
