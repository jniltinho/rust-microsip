//! Local SQLite persistence for call history and contacts.
//!
//! [`Store`] owns one connection. Writes take `&mut self`, so the application
//! serializes them through a single owner (the storage worker). Every query is
//! parameterized; user data never appears in [`StorageError`] messages.
//!
//! On disk the database lives in a `0700` directory as a `0600` file; SQLite
//! creates its `-wal`/`-shm` companions with the same mode as the database
//! file. Schema upgrades run in one transaction keyed by `PRAGMA user_version`,
//! and a database written by a newer release is refused without modification.

use rusqlite::{Connection, ErrorCode, OptionalExtension, TransactionBehavior, params};
use rust_microsip_core::{DialTarget, Direction, EndReason};
use std::fs::{self, DirBuilder, OpenOptions};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod csv;
#[cfg(test)]
mod tests;

pub use csv::{CSV_MAX_BYTES, CSV_MAX_RECORDS, CsvIssue, CsvIssueKind, CsvPreview, preview_csv};

/// Newest schema version this release can read and write.
pub const SCHEMA_VERSION: i64 = 1;
/// Default call-history retention, in days.
pub const DEFAULT_RETENTION_DAYS: u32 = 90;
/// Maximum numbers stored for one contact.
pub const MAX_NUMBERS_PER_CONTACT: usize = 32;
/// Maximum length, in bytes, of a contact name.
pub const MAX_NAME_LEN: usize = 128;
/// Maximum length, in bytes, of a number label.
pub const MAX_LABEL_LEN: usize = 64;
/// Maximum length, in bytes, of a call's remote party text.
pub const MAX_REMOTE_LEN: usize = 256;
/// Maximum length, in bytes, of a call session key.
pub const MAX_SESSION_KEY_LEN: usize = 128;

const BUSY_TIMEOUT: Duration = Duration::from_secs(2);

const MIGRATIONS: [&str; 1] = [r"
CREATE TABLE calls (
    id INTEGER PRIMARY KEY,
    session_key TEXT NOT NULL UNIQUE,
    started_utc INTEGER NOT NULL,
    answered_utc INTEGER,
    ended_utc INTEGER,
    direction TEXT NOT NULL CHECK (direction IN ('incoming', 'outgoing')),
    remote TEXT NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN
        ('answered', 'missed', 'canceled', 'rejected', 'busy', 'no_answer', 'failed', 'interrupted')),
    duration_ms INTEGER CHECK (duration_ms >= 0),
    secure INTEGER NOT NULL CHECK (secure IN (0, 1))
);
CREATE INDEX calls_started ON calls (started_utc DESC, id DESC);
CREATE INDEX calls_remote ON calls (remote);
CREATE TABLE contacts (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    favorite INTEGER NOT NULL DEFAULT 0 CHECK (favorite IN (0, 1)),
    created_utc INTEGER NOT NULL
);
CREATE INDEX contacts_order ON contacts (favorite DESC, name COLLATE NOCASE, id);
CREATE TABLE contact_numbers (
    id INTEGER PRIMARY KEY,
    contact_id INTEGER NOT NULL REFERENCES contacts (id) ON DELETE CASCADE,
    number TEXT NOT NULL,
    label TEXT,
    UNIQUE (contact_id, number)
);
CREATE INDEX contact_numbers_number ON contact_numbers (number);
"];

/// Which input field failed validation. Carries no user data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidField {
    /// Session key empty, too long or containing control characters.
    #[error("session key")]
    SessionKey,
    /// Remote party text too long or containing control characters.
    #[error("remote party")]
    Remote,
    /// Contact name empty, too long or containing control characters.
    #[error("contact name")]
    Name,
    /// A number is not a valid dial target.
    #[error("phone number")]
    Number,
    /// A label is too long or contains control characters.
    #[error("number label")]
    Label,
    /// A contact has no numbers or more than [`MAX_NUMBERS_PER_CONTACT`].
    #[error("number count")]
    NumberCount,
    /// A contact lists the same number twice.
    #[error("duplicate number")]
    DuplicateNumber,
    /// A duration does not fit the database integer range.
    #[error("duration")]
    Duration,
}

/// Storage failure. `Display` never includes user data or paths.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// Directory or file preparation failed.
    #[error("the storage directory or file could not be prepared")]
    Io(#[source] std::io::Error),
    /// The path exists but is not a regular file (for example, a symlink).
    #[error("the storage path is not a regular file")]
    NotRegularFile,
    /// The database was written by a newer release; it was left untouched.
    #[error("the database schema version {found} is newer than the supported version {supported}")]
    FutureSchema {
        /// Version found in the file.
        found: i64,
        /// Newest version this release supports.
        supported: i64,
    },
    /// Another process holds the database lock beyond the busy timeout.
    #[error("the database is locked by another process")]
    Busy,
    /// The file is corrupt or not a SQLite database.
    #[error("the database file is corrupt or not a database")]
    Corrupt,
    /// The disk is full.
    #[error("the disk is full")]
    Full,
    /// Any other SQLite failure.
    #[error("a database operation failed")]
    Sqlite(#[source] rusqlite::Error),
    /// Input failed validation.
    #[error("invalid {0}")]
    Invalid(InvalidField),
    /// The referenced record does not exist.
    #[error("the record was not found")]
    NotFound,
    /// The backup destination already exists; it is never overwritten.
    #[error("the backup destination already exists")]
    BackupExists,
    /// The CSV input exceeds [`CSV_MAX_BYTES`].
    #[error("the CSV file exceeds the size limit")]
    CsvTooLarge,
    /// The CSV input exceeds [`CSV_MAX_RECORDS`].
    #[error("the CSV file exceeds the record limit")]
    CsvTooManyRecords,
    /// The first CSV line is not `name,number` or `name,number,label`.
    #[error("the CSV header must be name,number or name,number,label")]
    CsvHeader,
    /// The preview has invalid rows; nothing was imported.
    #[error("the CSV preview contains invalid rows")]
    CsvInvalid,
}

impl From<rusqlite::Error> for StorageError {
    fn from(error: rusqlite::Error) -> Self {
        match error.sqlite_error_code() {
            Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => Self::Busy,
            Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => Self::Corrupt,
            Some(ErrorCode::DiskFull) => Self::Full,
            _ => Self::Sqlite(error),
        }
    }
}

impl From<std::io::Error> for StorageError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Result alias for storage operations.
pub type Result<T, E = StorageError> = std::result::Result<T, E>;

/// Final outcome of a call as shown in history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallOutcome {
    /// The call was established at some point.
    Answered,
    /// Incoming call never established and not rejected locally.
    Missed,
    /// Outgoing call canceled before answer.
    Canceled,
    /// Rejected (locally for incoming, by the peer for outgoing).
    Rejected,
    /// The peer was busy.
    Busy,
    /// The peer did not answer.
    NoAnswer,
    /// Negotiation, signaling or media failure before answer.
    Failed,
    /// The process stopped before the call ended (crash recovery).
    Interrupted,
}

impl CallOutcome {
    /// Map an engine end reason to a history outcome.
    ///
    /// Any call that was ever established is [`CallOutcome::Answered`]. An
    /// incoming call that was never established is
    /// [`CallOutcome::Missed`] unless the local user rejected it.
    pub const fn from_end(direction: Direction, established: bool, reason: EndReason) -> Self {
        if established {
            return Self::Answered;
        }
        match (direction, reason) {
            (_, EndReason::Rejected) => Self::Rejected,
            (Direction::Incoming, _) => Self::Missed,
            (Direction::Outgoing, EndReason::Busy) => Self::Busy,
            (Direction::Outgoing, EndReason::NoAnswer) => Self::NoAnswer,
            (Direction::Outgoing, EndReason::NotAcceptable | EndReason::Failed) => Self::Failed,
            (
                Direction::Outgoing,
                EndReason::Canceled | EndReason::Normal | EndReason::Transferred,
            ) => Self::Canceled,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Answered => "answered",
            Self::Missed => "missed",
            Self::Canceled => "canceled",
            Self::Rejected => "rejected",
            Self::Busy => "busy",
            Self::NoAnswer => "no_answer",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        [
            Self::Answered,
            Self::Missed,
            Self::Canceled,
            Self::Rejected,
            Self::Busy,
            Self::NoAnswer,
            Self::Failed,
            Self::Interrupted,
        ]
        .into_iter()
        .find(|o| o.as_str() == value)
    }
}

/// One finished (or crash-recovered) call to record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallRecord {
    /// Unique key per call, e.g. `"<engine session id>:<call id>"`.
    /// Recording the same key twice is a no-op.
    pub session_key: String,
    /// Start time, Unix seconds UTC.
    pub started_utc: i64,
    /// Time the call was established, Unix seconds UTC.
    pub answered_utc: Option<i64>,
    /// End time, Unix seconds UTC; `None` when interrupted.
    pub ended_utc: Option<i64>,
    /// Origination side.
    pub direction: Direction,
    /// Remote party user part or sanitized display text.
    pub remote: String,
    /// Final outcome.
    pub outcome: CallOutcome,
    /// Established time only, in milliseconds; `None` if unknown.
    pub duration_ms: Option<u64>,
    /// Media was SRTP-protected.
    pub secure: bool,
}

/// A stored history entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    /// Row identifier, for [`Store::delete_call`].
    pub id: i64,
    /// Recorded data.
    pub call: CallRecord,
}

/// History list filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HistoryFilter {
    /// Every call.
    #[default]
    All,
    /// Incoming calls only.
    Incoming,
    /// Outgoing calls only.
    Outgoing,
    /// Missed calls only.
    Missed,
}

/// One phone number of a contact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContactNumber {
    /// Dial string, validated with [`DialTarget::parse`] and stored trimmed.
    pub number: String,
    /// Optional label such as `work`.
    pub label: Option<String>,
}

/// Contact data for insert or update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewContact {
    /// Display name.
    pub name: String,
    /// Shown first in lists.
    pub favorite: bool,
    /// One to [`MAX_NUMBERS_PER_CONTACT`] distinct numbers.
    pub numbers: Vec<ContactNumber>,
}

/// A stored contact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contact {
    /// Row identifier.
    pub id: i64,
    /// Display name.
    pub name: String,
    /// Shown first in lists.
    pub favorite: bool,
    /// Creation time, Unix seconds UTC.
    pub created_utc: i64,
    /// Numbers in insertion order.
    pub numbers: Vec<ContactNumber>,
}

/// Result of [`Store::import_csv`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportSummary {
    /// New contacts created.
    pub contacts_created: usize,
    /// Existing contacts (same exact name) that received numbers.
    pub contacts_merged: usize,
    /// Numbers added across all contacts.
    pub numbers_added: usize,
}

/// Handle to the history and contacts database.
pub struct Store {
    conn: Connection,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Store(..)")
    }
}

impl Store {
    /// Open or create the database at `path`, then migrate it.
    ///
    /// Creates a missing parent directory with mode `0700` and the file with
    /// mode `0600` (an existing broader file mode is tightened to `0600`).
    /// Before upgrading an existing older schema, a consistent backup is
    /// written next to the file as `<file>.v<old>.bak`.
    ///
    /// # Errors
    /// [`StorageError::FutureSchema`] for a newer database (left untouched),
    /// [`StorageError::Busy`], [`StorageError::Corrupt`], [`StorageError::Full`],
    /// [`StorageError::NotRegularFile`] or I/O failures. Nothing is deleted.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty() && !p.exists())
        {
            DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)?;
        }
        create_private_file(path, false)?;
        let mut store = Self {
            conn: Connection::open(path)?,
        };
        store.configure()?;
        let version = store.user_version()?;
        if version > 0 && version < SCHEMA_VERSION {
            store.backup_to(&backup_path(path, version))?;
        }
        store.migrate()?;
        Ok(store)
    }

    /// Open a migrated in-memory database (tests and previews).
    ///
    /// # Errors
    /// Returns a SQLite failure.
    pub fn open_in_memory() -> Result<Self> {
        let mut store = Self {
            conn: Connection::open_in_memory()?,
        };
        store.configure()?;
        store.migrate()?;
        Ok(store)
    }

    fn configure(&self) -> Result<()> {
        self.conn.busy_timeout(BUSY_TIMEOUT)?;
        self.conn.pragma_update(None, "foreign_keys", true)?;
        // Read the version before anything that could write the header.
        let found = self.user_version()?;
        if found > SCHEMA_VERSION {
            return Err(StorageError::FutureSchema {
                found,
                supported: SCHEMA_VERSION,
            });
        }
        self.conn
            .query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        Ok(())
    }

    fn user_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    fn migrate(&mut self) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: i64 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if current > SCHEMA_VERSION {
            return Err(StorageError::FutureSchema {
                found: current,
                supported: SCHEMA_VERSION,
            });
        }
        for (version, sql) in (1..).zip(MIGRATIONS) {
            if version > current {
                tx.execute_batch(sql)?;
            }
        }
        if current < SCHEMA_VERSION {
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Schema version of the open database.
    ///
    /// # Errors
    /// Returns a SQLite failure.
    pub fn schema_version(&self) -> Result<i64> {
        self.user_version()
    }

    /// Write a consistent copy to a new `0600` file using SQLite's online
    /// backup API. An existing destination is never overwritten.
    ///
    /// # Errors
    /// [`StorageError::BackupExists`], I/O or SQLite failures; a partial
    /// destination file is removed.
    pub fn backup_to(&self, path: &Path) -> Result<()> {
        create_private_file(path, true)?;
        self.conn
            .backup(rusqlite::MAIN_DB, path, None)
            .map_err(|error| {
                let _ = fs::remove_file(path);
                StorageError::from(error)
            })
    }

    // ---- history ----

    /// Record one call. Returns `false` when `session_key` was already
    /// recorded (the existing entry is kept unchanged).
    ///
    /// # Errors
    /// [`StorageError::Invalid`] for bad keys or remote text, or SQLite failures.
    pub fn record_call(&mut self, call: &CallRecord) -> Result<bool> {
        if !text_ok(&call.session_key, MAX_SESSION_KEY_LEN) {
            return Err(StorageError::Invalid(InvalidField::SessionKey));
        }
        if call.remote.len() > MAX_REMOTE_LEN || call.remote.chars().any(char::is_control) {
            return Err(StorageError::Invalid(InvalidField::Remote));
        }
        let duration = call
            .duration_ms
            .map(i64::try_from)
            .transpose()
            .map_err(|_| StorageError::Invalid(InvalidField::Duration))?;
        let inserted = self.conn.execute(
            "INSERT OR IGNORE INTO calls (session_key, started_utc, answered_utc, ended_utc,
                 direction, remote, outcome, duration_ms, secure)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                call.session_key,
                call.started_utc,
                call.answered_utc,
                call.ended_utc,
                direction_str(call.direction),
                call.remote,
                call.outcome.as_str(),
                duration,
                call.secure,
            ],
        )?;
        Ok(inserted == 1)
    }

    /// Newest calls first, at most `limit`, optionally filtered and searched
    /// by remote party (case-insensitive substring, wildcards literal).
    ///
    /// # Errors
    /// Returns a SQLite failure.
    pub fn recent_calls(
        &self,
        limit: u32,
        filter: HistoryFilter,
        search: Option<&str>,
    ) -> Result<Vec<HistoryEntry>> {
        let (direction, outcome) = match filter {
            HistoryFilter::All => (None, None),
            HistoryFilter::Incoming => (Some("incoming"), None),
            HistoryFilter::Outgoing => (Some("outgoing"), None),
            HistoryFilter::Missed => (None, Some("missed")),
        };
        let pattern = like_pattern(search.unwrap_or("").trim());
        let mut stmt = self.conn.prepare(
            "SELECT id, session_key, started_utc, answered_utc, ended_utc, direction, remote,
                    outcome, duration_ms, secure
             FROM calls
             WHERE (?1 IS NULL OR direction = ?1) AND (?2 IS NULL OR outcome = ?2)
               AND remote LIKE ?3 ESCAPE '\\'
             ORDER BY started_utc DESC, id DESC LIMIT ?4",
        )?;
        let rows = stmt.query_map(params![direction, outcome, pattern, limit], |row| {
            let direction: String = row.get(5)?;
            let outcome: String = row.get(7)?;
            let duration: Option<i64> = row.get(8)?;
            Ok(HistoryEntry {
                id: row.get(0)?,
                call: CallRecord {
                    session_key: row.get(1)?,
                    started_utc: row.get(2)?,
                    answered_utc: row.get(3)?,
                    ended_utc: row.get(4)?,
                    direction: if direction == "incoming" {
                        Direction::Incoming
                    } else {
                        Direction::Outgoing
                    },
                    remote: row.get(6)?,
                    // The CHECK constraint guarantees a known value.
                    outcome: CallOutcome::parse(&outcome).unwrap_or(CallOutcome::Failed),
                    duration_ms: duration.and_then(|d| u64::try_from(d).ok()),
                    secure: row.get(9)?,
                },
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Delete one history entry.
    ///
    /// # Errors
    /// [`StorageError::NotFound`] or a SQLite failure.
    pub fn delete_call(&mut self, id: i64) -> Result<()> {
        match self.conn.execute("DELETE FROM calls WHERE id = ?1", [id])? {
            0 => Err(StorageError::NotFound),
            _ => Ok(()),
        }
    }

    /// Delete every history entry; returns the count removed. Removal is not
    /// forensic erasure of WAL pages or existing backups.
    ///
    /// # Errors
    /// Returns a SQLite failure.
    pub fn clear_history(&mut self) -> Result<usize> {
        Ok(self.conn.execute("DELETE FROM calls", [])?)
    }

    /// Delete calls that started more than `days` days ago; returns the count.
    ///
    /// # Errors
    /// Returns a SQLite failure.
    pub fn prune_older_than(&mut self, days: u32) -> Result<usize> {
        let cutoff = now_utc() - i64::from(days) * 86_400;
        Ok(self
            .conn
            .execute("DELETE FROM calls WHERE started_utc < ?1", [cutoff])?)
    }

    // ---- contacts ----

    /// Insert a contact and its numbers; returns the new id.
    ///
    /// # Errors
    /// [`StorageError::Invalid`] or a SQLite failure; nothing is written on error.
    pub fn add_contact(&mut self, contact: &NewContact) -> Result<i64> {
        let contact = validate_contact(contact)?;
        let tx = self.conn.transaction()?;
        let id = insert_contact(&tx, &contact)?;
        tx.commit()?;
        Ok(id)
    }

    /// Replace a contact's name, favorite flag and numbers.
    ///
    /// # Errors
    /// [`StorageError::NotFound`], [`StorageError::Invalid`] or a SQLite failure.
    pub fn update_contact(&mut self, id: i64, contact: &NewContact) -> Result<()> {
        let contact = validate_contact(contact)?;
        let tx = self.conn.transaction()?;
        let changed = tx.execute(
            "UPDATE contacts SET name = ?1, favorite = ?2 WHERE id = ?3",
            params![contact.name, contact.favorite, id],
        )?;
        if changed == 0 {
            return Err(StorageError::NotFound);
        }
        tx.execute("DELETE FROM contact_numbers WHERE contact_id = ?1", [id])?;
        insert_numbers(&tx, id, &contact.numbers)?;
        tx.commit()?;
        Ok(())
    }

    /// Delete a contact and (by cascade) its numbers.
    ///
    /// # Errors
    /// [`StorageError::NotFound`] or a SQLite failure.
    pub fn delete_contact(&mut self, id: i64) -> Result<()> {
        match self
            .conn
            .execute("DELETE FROM contacts WHERE id = ?1", [id])?
        {
            0 => Err(StorageError::NotFound),
            _ => Ok(()),
        }
    }

    /// Set or clear the favorite flag.
    ///
    /// # Errors
    /// [`StorageError::NotFound`] or a SQLite failure.
    pub fn set_favorite(&mut self, id: i64, favorite: bool) -> Result<()> {
        match self.conn.execute(
            "UPDATE contacts SET favorite = ?1 WHERE id = ?2",
            params![favorite, id],
        )? {
            0 => Err(StorageError::NotFound),
            _ => Ok(()),
        }
    }

    /// Fetch one contact.
    ///
    /// # Errors
    /// Returns a SQLite failure.
    pub fn contact(&self, id: i64) -> Result<Option<Contact>> {
        let head = self
            .conn
            .query_row(
                "SELECT id, name, favorite, created_utc FROM contacts WHERE id = ?1",
                [id],
                |row| {
                    Ok(Contact {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        favorite: row.get(2)?,
                        created_utc: row.get(3)?,
                        numbers: Vec::new(),
                    })
                },
            )
            .optional()?;
        head.map(|c| self.with_numbers(c)).transpose()
    }

    /// Contacts whose name or any number contains `query` (case-insensitive,
    /// `%`/`_` matched literally); favorites first, then by name. An empty
    /// query lists all contacts.
    ///
    /// # Errors
    /// Returns a SQLite failure.
    pub fn search_contacts(&self, query: &str, limit: u32) -> Result<Vec<Contact>> {
        let pattern = like_pattern(query.trim());
        let mut stmt = self.conn.prepare(
            "SELECT id, name, favorite, created_utc FROM contacts c
             WHERE name LIKE ?1 ESCAPE '\\'
                OR EXISTS (SELECT 1 FROM contact_numbers n
                           WHERE n.contact_id = c.id AND n.number LIKE ?1 ESCAPE '\\')
             ORDER BY favorite DESC, name COLLATE NOCASE, id LIMIT ?2",
        )?;
        let heads = stmt
            .query_map(params![pattern, limit], |row| {
                Ok(Contact {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    favorite: row.get(2)?,
                    created_utc: row.get(3)?,
                    numbers: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // ponytail: one numbers query per contact; bounded by `limit`.
        heads.into_iter().map(|c| self.with_numbers(c)).collect()
    }

    fn with_numbers(&self, mut contact: Contact) -> Result<Contact> {
        let mut stmt = self.conn.prepare(
            "SELECT number, label FROM contact_numbers WHERE contact_id = ?1 ORDER BY id",
        )?;
        contact.numbers = stmt
            .query_map([contact.id], |row| {
                Ok(ContactNumber {
                    number: row.get(0)?,
                    label: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(contact)
    }

    /// Apply a clean CSV preview in one transaction (all or nothing).
    ///
    /// A preview contact whose exact name already exists is merged into the
    /// oldest such contact: only numbers it does not already have are added.
    ///
    /// # Errors
    /// [`StorageError::CsvInvalid`] when the preview has issues, or any
    /// validation/SQLite failure; on error nothing is written.
    pub fn import_csv(&mut self, preview: &CsvPreview) -> Result<ImportSummary> {
        if !preview.is_importable() {
            return Err(StorageError::CsvInvalid);
        }
        let contacts = preview
            .contacts()
            .iter()
            .map(validate_contact)
            .collect::<Result<Vec<_>>>()?;
        let mut summary = ImportSummary::default();
        let tx = self.conn.transaction()?;
        for contact in &contacts {
            let existing: Option<i64> = tx
                .query_row(
                    "SELECT id FROM contacts WHERE name = ?1 ORDER BY id LIMIT 1",
                    [&contact.name],
                    |row| row.get(0),
                )
                .optional()?;
            match existing {
                Some(id) => {
                    let mut stmt = tx.prepare(
                        "INSERT OR IGNORE INTO contact_numbers (contact_id, number, label)
                         VALUES (?1, ?2, ?3)",
                    )?;
                    let mut added = 0;
                    for n in &contact.numbers {
                        added += stmt.execute(params![id, n.number, n.label])?;
                    }
                    let total: i64 = tx.query_row(
                        "SELECT COUNT(*) FROM contact_numbers WHERE contact_id = ?1",
                        [id],
                        |row| row.get(0),
                    )?;
                    // The per-contact bound holds for merged contacts too;
                    // returning rolls back the whole import.
                    if total > MAX_NUMBERS_PER_CONTACT as i64 {
                        return Err(StorageError::Invalid(InvalidField::NumberCount));
                    }
                    if added > 0 {
                        summary.contacts_merged += 1;
                        summary.numbers_added += added;
                    }
                }
                None => {
                    insert_contact(&tx, contact)?;
                    summary.contacts_created += 1;
                    summary.numbers_added += contact.numbers.len();
                }
            }
        }
        tx.commit()?;
        Ok(summary)
    }
}

fn insert_contact(conn: &Connection, contact: &NewContact) -> Result<i64> {
    conn.execute(
        "INSERT INTO contacts (name, favorite, created_utc) VALUES (?1, ?2, ?3)",
        params![contact.name, contact.favorite, now_utc()],
    )?;
    let id = conn.last_insert_rowid();
    insert_numbers(conn, id, &contact.numbers)?;
    Ok(id)
}

fn insert_numbers(conn: &Connection, id: i64, numbers: &[ContactNumber]) -> Result<()> {
    let mut stmt = conn
        .prepare("INSERT INTO contact_numbers (contact_id, number, label) VALUES (?1, ?2, ?3)")?;
    for n in numbers {
        stmt.execute(params![id, n.number, n.label])?;
    }
    Ok(())
}

/// Validate and normalize (trim) a contact.
pub(crate) fn validate_contact(contact: &NewContact) -> Result<NewContact> {
    let name = contact.name.trim();
    if !text_ok(name, MAX_NAME_LEN) {
        return Err(StorageError::Invalid(InvalidField::Name));
    }
    if contact.numbers.is_empty() || contact.numbers.len() > MAX_NUMBERS_PER_CONTACT {
        return Err(StorageError::Invalid(InvalidField::NumberCount));
    }
    let mut numbers: Vec<ContactNumber> = Vec::with_capacity(contact.numbers.len());
    for n in &contact.numbers {
        let number = normalize_number(&n.number)?;
        if numbers.iter().any(|x| x.number == number) {
            return Err(StorageError::Invalid(InvalidField::DuplicateNumber));
        }
        let label = n.label.as_deref().map(str::trim).filter(|l| !l.is_empty());
        if label.is_some_and(|l| !text_ok(l, MAX_LABEL_LEN)) {
            return Err(StorageError::Invalid(InvalidField::Label));
        }
        numbers.push(ContactNumber {
            number,
            label: label.map(str::to_owned),
        });
    }
    Ok(NewContact {
        name: name.to_owned(),
        favorite: contact.favorite,
        numbers,
    })
}

pub(crate) fn normalize_number(number: &str) -> Result<String> {
    DialTarget::parse(number)
        .map(|t| t.as_str().to_owned())
        .map_err(|_| StorageError::Invalid(InvalidField::Number))
}

/// Non-empty, bounded, no control characters.
pub(crate) fn text_ok(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

fn like_pattern(query: &str) -> String {
    let mut pattern = String::with_capacity(query.len() + 2);
    pattern.push('%');
    for c in query.chars() {
        if matches!(c, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern.push('%');
    pattern
}

const fn direction_str(direction: Direction) -> &'static str {
    match direction {
        Direction::Incoming => "incoming",
        Direction::Outgoing => "outgoing",
    }
}

fn now_utc() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn backup_path(path: &Path, version: i64) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".v{version}.bak"));
    PathBuf::from(name)
}

/// Create (or, unless `new_only`, reuse) a regular file with mode `0600`.
fn create_private_file(path: &Path, new_only: bool) -> Result<()> {
    // ponytail: symlink check then open is not race-free; O_NOFOLLOW needs libc.
    match fs::symlink_metadata(path) {
        Ok(_) if new_only => return Err(StorageError::BackupExists),
        Ok(meta) if !meta.file_type().is_file() => return Err(StorageError::NotRegularFile),
        Ok(_) | Err(_) => {}
    }
    let mut options = OpenOptions::new();
    options.write(true).mode(0o600);
    if new_only {
        options.create_new(true);
    } else {
        options.create(true);
    }
    let file = options.open(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => StorageError::BackupExists,
        _ => StorageError::Io(e),
    })?;
    let mode = file.metadata()?.permissions().mode();
    if mode & 0o077 != 0 {
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
