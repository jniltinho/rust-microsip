//! Durable control ledger and audit (AG-05).
//!
//! Every mutation requested over the control socket is recorded here before
//! it has any effect, keyed by owner epoch and idempotency key. The payload is
//! represented only by an HMAC computed by the owner with a key that lives in
//! memory for one epoch, so low-entropy values (DTMF digits, numbers) cannot be
//! recovered from the database. Outcomes and audit details are sanitized by
//! the caller; this module rejects control characters and oversized text.
//!
//! Each owner start opens a new epoch: operations left waiting for approval
//! expire, and operations that may have been sent become `outcome_unknown` —
//! never retried. Operations are kept for [`LEDGER_RETENTION_SECS`]; audit
//! rows for [`AUDIT_RETENTION_DAYS`] days.

use crate::{InvalidField, Result, StorageError, Store, now_utc, text_ok};
use rusqlite::{OptionalExtension, TransactionBehavior, params};

/// How long operation outcomes stay queryable.
pub const LEDGER_RETENTION_SECS: i64 = 24 * 60 * 60;
/// How long audit rows are kept.
pub const AUDIT_RETENTION_DAYS: i64 = 90;
const MAX_TEXT: usize = 1024;

/// Lifecycle of a recorded operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationState {
    /// Waiting for a person to approve it in the window.
    AwaitingApproval,
    /// Approved or granted; about to be sent.
    Executing,
    /// The effect was handed to the engine and confirmed accepted.
    Succeeded,
    /// The engine or owner refused it; no effect.
    Failed,
    /// A person denied it; no effect.
    Denied,
    /// Nobody decided in time, or the owner restarted first; no effect.
    Expired,
    /// The owner stopped after it may have been sent; never retried.
    OutcomeUnknown,
}
impl OperationState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::AwaitingApproval => "awaiting_approval",
            Self::Executing => "executing",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Denied => "denied",
            Self::Expired => "expired",
            Self::OutcomeUnknown => "outcome_unknown",
        }
    }
    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "awaiting_approval" => Self::AwaitingApproval,
            "executing" => Self::Executing,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "denied" => Self::Denied,
            "expired" => Self::Expired,
            "outcome_unknown" => Self::OutcomeUnknown,
            _ => return None,
        })
    }
    /// Whether the operation can no longer change.
    #[must_use]
    pub const fn is_final(self) -> bool {
        !matches!(self, Self::AwaitingApproval | Self::Executing)
    }
}

/// A recorded operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationRecord {
    /// Opaque identifier given to the client.
    pub operation_id: String,
    /// Owner epoch that accepted it.
    pub epoch: i64,
    /// Client idempotency key.
    pub idempotency_key: String,
    /// Operation name (`call_dial`, ...).
    pub operation: String,
    /// HMAC of the payload under the epoch's in-memory key.
    pub payload_mac: Vec<u8>,
    /// Current state.
    pub state: OperationState,
    /// Sanitized outcome (JSON text), if any.
    pub outcome: Option<String>,
    /// Creation time, Unix seconds.
    pub created_utc: i64,
}

fn checked(text: &str, field: InvalidField) -> Result<()> {
    if !text_ok(text, MAX_TEXT) {
        return Err(StorageError::Invalid(field));
    }
    Ok(())
}

impl Store {
    /// Open a new owner epoch: expire waiting operations, mark possibly sent
    /// ones `outcome_unknown`, and drop rows past their retention.
    ///
    /// # Errors
    /// SQLite failures; nothing changes on error.
    pub fn control_begin_epoch(&mut self) -> Result<i64> {
        let now = now_utc();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let epoch: i64 = tx.query_row(
            "SELECT COALESCE(MAX(epoch), 0) + 1 FROM control_epochs",
            [],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT INTO control_epochs (epoch, started_utc) VALUES (?1, ?2)",
            params![epoch, now],
        )?;
        tx.execute(
            "UPDATE control_operations SET state = 'expired', updated_utc = ?1
             WHERE state = 'awaiting_approval'",
            params![now],
        )?;
        tx.execute(
            "UPDATE control_operations SET state = 'outcome_unknown', updated_utc = ?1
             WHERE state = 'executing'",
            params![now],
        )?;
        tx.execute(
            "DELETE FROM control_operations WHERE created_utc < ?1",
            params![now - LEDGER_RETENTION_SECS],
        )?;
        tx.execute(
            "DELETE FROM control_audit WHERE at_utc < ?1",
            params![now - AUDIT_RETENTION_DAYS * 86_400],
        )?;
        tx.commit()?;
        Ok(epoch)
    }

    /// Newest record for `idempotency_key` within the retention window.
    ///
    /// # Errors
    /// SQLite failures.
    pub fn control_find_key(&self, idempotency_key: &str) -> Result<Option<OperationRecord>> {
        self.control_query(
            "WHERE idempotency_key = ?1 AND created_utc >= ?2 ORDER BY epoch DESC LIMIT 1",
            idempotency_key,
        )
    }

    /// Whether `idempotency_key` was ever recorded in `epoch` (ignores the
    /// retention window, which only limits what is served).
    ///
    /// # Errors
    /// SQLite failures.
    pub fn control_key_used(&self, epoch: i64, idempotency_key: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM control_operations WHERE epoch = ?1 AND idempotency_key = ?2",
                params![epoch, idempotency_key],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Record by operation id within the retention window.
    ///
    /// # Errors
    /// SQLite failures.
    pub fn control_get(&self, operation_id: &str) -> Result<Option<OperationRecord>> {
        self.control_query(
            "WHERE operation_id = ?1 AND created_utc >= ?2",
            operation_id,
        )
    }

    fn control_query(&self, filter: &str, value: &str) -> Result<Option<OperationRecord>> {
        let sql = format!(
            "SELECT operation_id, epoch, idempotency_key, operation, payload_mac, state,
                    outcome, created_utc FROM control_operations {filter}"
        );
        let row = self
            .conn
            .query_row(
                &sql,
                params![value, now_utc() - LEDGER_RETENTION_SECS],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, Vec<u8>>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, Option<String>>(6)?,
                        r.get::<_, i64>(7)?,
                    ))
                },
            )
            .optional()?;
        row.map(
            |(operation_id, epoch, key, operation, mac, state, outcome, created)| {
                Ok(OperationRecord {
                    operation_id,
                    epoch,
                    idempotency_key: key,
                    operation,
                    payload_mac: mac,
                    state: OperationState::parse(&state)
                        .ok_or(StorageError::Invalid(InvalidField::ControlText))?,
                    outcome,
                    created_utc: created,
                })
            },
        )
        .transpose()
    }

    /// Record a new operation before it has any effect.
    ///
    /// # Errors
    /// [`StorageError::Invalid`] for bad text; a SQLite constraint error when
    /// the epoch already has this idempotency key.
    pub fn control_insert(&mut self, record: &OperationRecord) -> Result<()> {
        for text in [
            &record.operation_id,
            &record.idempotency_key,
            &record.operation,
        ] {
            checked(text, InvalidField::ControlText)?;
        }
        if let Some(outcome) = &record.outcome {
            checked(outcome, InvalidField::ControlText)?;
        }
        self.conn.execute(
            "INSERT INTO control_operations (operation_id, epoch, idempotency_key, operation,
                 payload_mac, state, outcome, created_utc, updated_utc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![
                record.operation_id,
                record.epoch,
                record.idempotency_key,
                record.operation,
                record.payload_mac,
                record.state.as_str(),
                record.outcome,
                record.created_utc,
            ],
        )?;
        Ok(())
    }

    /// Move an operation to `state`. Final states never change again.
    ///
    /// # Errors
    /// [`StorageError::Invalid`] for bad text; [`StorageError::NotFound`] when
    /// the operation is unknown or already final.
    pub fn control_update(
        &mut self,
        operation_id: &str,
        state: OperationState,
        outcome: Option<&str>,
    ) -> Result<()> {
        if let Some(outcome) = outcome {
            checked(outcome, InvalidField::ControlText)?;
        }
        let changed = self.conn.execute(
            "UPDATE control_operations SET state = ?2, outcome = COALESCE(?3, outcome),
                 updated_utc = ?4
             WHERE operation_id = ?1 AND state IN ('awaiting_approval', 'executing')",
            params![operation_id, state.as_str(), outcome, now_utc()],
        )?;
        if changed == 0 {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }

    /// Append a sanitized audit row.
    ///
    /// # Errors
    /// [`StorageError::Invalid`] for bad text, otherwise SQLite failures.
    pub fn control_audit(&mut self, operation: &str, decision: &str, detail: &str) -> Result<()> {
        for text in [operation, decision, detail] {
            checked(text, InvalidField::ControlText)?;
        }
        self.conn.execute(
            "INSERT INTO control_audit (at_utc, operation, decision, detail) VALUES (?1, ?2, ?3, ?4)",
            params![now_utc(), operation, decision, detail],
        )?;
        Ok(())
    }

    /// Audit rows, newest first: `(at_utc, operation, decision, detail)`.
    ///
    /// # Errors
    /// SQLite failures.
    pub fn control_audit_rows(&self, limit: u32) -> Result<Vec<(i64, String, String, String)>> {
        let mut statement = self.conn.prepare(
            "SELECT at_utc, operation, decision, detail FROM control_audit
             ORDER BY at_utc DESC, id DESC LIMIT ?1",
        )?;
        let rows = statement.query_map(params![limit], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Delete every audit row (confirmed by the user in the window).
    ///
    /// # Errors
    /// SQLite failures.
    pub fn control_audit_clear(&mut self) -> Result<usize> {
        Ok(self.conn.execute("DELETE FROM control_audit", [])?)
    }
}
