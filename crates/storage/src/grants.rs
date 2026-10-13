//! Control grants (AG-04): issued by a person in the window, presented by a
//! client as a secret capability, stored here only as its SHA-256.
//!
//! A grant names read and control scopes, exact allowed destinations for
//! dialing and transfers, the incoming callers it may answer or reject, an
//! expiry (at most one hour after issue), a call quota and the owner epoch it
//! is valid in. A *bootstrap* grant is exported for the headless service: it
//! has no epoch until the service consumes its single-use id, atomically, in
//! the service's new epoch. Revoked, expired, consumed or other-epoch grants
//! never authorize anything.

use crate::{InvalidField, Result, StorageError, Store, now_utc, text_ok};
use rusqlite::{OptionalExtension, params};

/// Longest grant validity.
pub const MAX_GRANT_SECS: i64 = 60 * 60;
const MAX_LIST: usize = 64;

/// A stored grant (never the capability itself).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    /// Public identifier shown in the window.
    pub id: String,
    /// Scope names (`read:contacts`, `control:dial`, ...).
    pub scopes: Vec<String>,
    /// Exact destinations allowed for dial and transfer.
    pub destinations: Vec<String>,
    /// Exact incoming callers that may be answered or rejected.
    pub incoming: Vec<String>,
    /// Expiry, Unix seconds.
    pub expires_utc: i64,
    /// Calls (dial/answer) allowed in total.
    pub quota: u32,
    /// Calls used so far.
    pub used: u32,
    /// Owner epoch it is bound to; `None` for an unconsumed bootstrap.
    pub epoch: Option<i64>,
    /// Single-use id of a bootstrap grant.
    pub bootstrap_id: Option<String>,
    /// Revoked in the window.
    pub revoked: bool,
}

fn list_ok(items: &[String]) -> bool {
    items.len() <= MAX_LIST && items.iter().all(|i| text_ok(i, 128) && !i.contains('\n'))
}

impl Store {
    /// Store a new grant keyed by the SHA-256 of its capability.
    ///
    /// # Errors
    /// [`StorageError::Invalid`] for bad text, too long a validity or a hash
    /// that is not 32 bytes; SQLite failures.
    pub fn grant_insert(&mut self, grant: &Grant, capability_hash: &[u8]) -> Result<()> {
        let now = now_utc();
        let bad = || StorageError::Invalid(InvalidField::ControlText);
        if capability_hash.len() != 32
            || !text_ok(&grant.id, 64)
            || !list_ok(&grant.scopes)
            || !list_ok(&grant.destinations)
            || !list_ok(&grant.incoming)
            || grant.expires_utc <= now
            || grant.expires_utc > now + MAX_GRANT_SECS
            || grant.bootstrap_id.as_ref().is_some_and(|b| !text_ok(b, 64))
        {
            return Err(bad());
        }
        self.conn.execute(
            "INSERT INTO control_grants (id, capability_hash, scopes, destinations, incoming,
                 expires_utc, quota, used, epoch, bootstrap_id, bootstrap_consumed, revoked,
                 created_utc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, 0, 0, ?10)",
            params![
                grant.id,
                capability_hash,
                grant.scopes.join("\n"),
                grant.destinations.join("\n"),
                grant.incoming.join("\n"),
                grant.expires_utc,
                grant.quota,
                grant.epoch,
                grant.bootstrap_id,
                now,
            ],
        )?;
        Ok(())
    }

    /// The grant for a capability hash, if it is usable in `epoch` now:
    /// not revoked, not expired, bound to `epoch`.
    ///
    /// # Errors
    /// SQLite failures.
    pub fn grant_active(&self, capability_hash: &[u8], epoch: i64) -> Result<Option<Grant>> {
        self.grant_query(
            "WHERE capability_hash = ?1 AND epoch = ?2 AND revoked = 0 AND expires_utc > ?3",
            params![capability_hash, epoch, now_utc()],
        )
        .map(|mut v| v.pop())
    }

    /// Grants usable in `epoch`, newest first (for the window's list).
    ///
    /// # Errors
    /// SQLite failures.
    pub fn grants_listed(&self, epoch: i64) -> Result<Vec<Grant>> {
        self.grant_query(
            "WHERE (epoch = ?1 OR (epoch IS NULL AND bootstrap_consumed = 0))
                 AND revoked = 0 AND expires_utc > ?2 ORDER BY created_utc DESC",
            params![epoch, now_utc()],
        )
    }

    fn grant_query(&self, filter: &str, values: impl rusqlite::Params) -> Result<Vec<Grant>> {
        let sql = format!(
            "SELECT id, scopes, destinations, incoming, expires_utc, quota, used, epoch,
                    bootstrap_id, revoked FROM control_grants {filter}"
        );
        let mut statement = self.conn.prepare(&sql)?;
        let split = |text: String| -> Vec<String> {
            text.split('\n')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        };
        let rows = statement.query_map(values, |r| {
            Ok(Grant {
                id: r.get(0)?,
                scopes: split(r.get(1)?),
                destinations: split(r.get(2)?),
                incoming: split(r.get(3)?),
                expires_utc: r.get(4)?,
                quota: r.get(5)?,
                used: r.get(6)?,
                epoch: r.get(7)?,
                bootstrap_id: r.get(8)?,
                revoked: r.get(9)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Count one call against the grant's quota; `false` when exhausted.
    ///
    /// # Errors
    /// SQLite failures.
    pub fn grant_use_call(&mut self, id: &str) -> Result<bool> {
        Ok(self.conn.execute(
            "UPDATE control_grants SET used = used + 1 WHERE id = ?1 AND used < quota",
            params![id],
        )? == 1)
    }

    /// Revoke a grant (also an unconsumed bootstrap).
    ///
    /// # Errors
    /// [`StorageError::NotFound`] for an unknown id; SQLite failures.
    pub fn grant_revoke(&mut self, id: &str) -> Result<()> {
        if self.conn.execute(
            "UPDATE control_grants SET revoked = 1 WHERE id = ?1",
            params![id],
        )? == 0
        {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }

    /// Consume a bootstrap once and bind it to `epoch`. Returns the grant, or
    /// `None` when the id is unknown, consumed, revoked or expired, or the
    /// capability does not match.
    ///
    /// # Errors
    /// SQLite failures.
    pub fn bootstrap_consume(
        &mut self,
        bootstrap_id: &str,
        capability_hash: &[u8],
        epoch: i64,
    ) -> Result<Option<Grant>> {
        let changed = self.conn.execute(
            "UPDATE control_grants SET bootstrap_consumed = 1, epoch = ?3
             WHERE bootstrap_id = ?1 AND capability_hash = ?2 AND bootstrap_consumed = 0
                 AND revoked = 0 AND expires_utc > ?4",
            params![bootstrap_id, capability_hash, epoch, now_utc()],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        self.grant_active(capability_hash, epoch)
    }

    /// Whether a bootstrap id was already consumed (for a precise error).
    ///
    /// # Errors
    /// SQLite failures.
    pub fn bootstrap_consumed(&self, bootstrap_id: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT bootstrap_consumed FROM control_grants WHERE bootstrap_id = ?1",
                params![bootstrap_id],
                |r| r.get::<_, bool>(0),
            )
            .optional()?
            .unwrap_or(false))
    }
}
