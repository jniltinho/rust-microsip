//! Unit tests with fictional data and private temporary directories.

use super::*;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicUsize, Ordering};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rust-microsip-storage-test-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            now_utc()
        ));
        fs::create_dir(&dir)?;
        Ok(Self(dir))
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn mode(path: &Path) -> std::io::Result<u32> {
    Ok(fs::metadata(path)?.permissions().mode() & 0o777)
}

fn call(
    key: &str,
    started: i64,
    direction: Direction,
    remote: &str,
    outcome: CallOutcome,
) -> CallRecord {
    CallRecord {
        session_key: key.into(),
        started_utc: started,
        answered_utc: None,
        ended_utc: Some(started + 5),
        direction,
        remote: remote.into(),
        outcome,
        duration_ms: None,
        secure: false,
    }
}

fn contact(name: &str, favorite: bool, numbers: &[&str]) -> NewContact {
    NewContact {
        name: name.into(),
        favorite,
        numbers: numbers
            .iter()
            .map(|n| ContactNumber {
                number: (*n).into(),
                label: None,
            })
            .collect(),
    }
}

fn count(store: &Store, table: &str) -> rusqlite::Result<i64> {
    store
        .conn
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
}

#[test]
fn migrations_are_idempotent_and_future_schema_is_refused_untouched() -> TestResult {
    let tmp = TempDir::new()?;
    let path = tmp.0.join("data/rust-microsip.sqlite3");
    {
        let mut store = Store::open(&path)?;
        assert_eq!(store.schema_version()?, SCHEMA_VERSION);
        assert!(store.record_call(&call(
            "s:1",
            1_000,
            Direction::Outgoing,
            "1002",
            CallOutcome::Answered
        ))?);
    }
    {
        let store = Store::open(&path)?;
        assert_eq!(store.schema_version()?, SCHEMA_VERSION);
        assert_eq!(store.recent_calls(10, HistoryFilter::All, None)?.len(), 1);
    }
    {
        let raw = Connection::open(&path)?;
        raw.pragma_update(None, "user_version", 99)?;
    }
    let before = fs::read(&path)?;
    match Store::open(&path) {
        Err(StorageError::FutureSchema {
            found: 99,
            supported: SCHEMA_VERSION,
        }) => {}
        other => return Err(format!("unexpected: {other:?}").into()),
    }
    assert_eq!(fs::read(&path)?, before);
    let raw = Connection::open(&path)?;
    let version: i64 = raw.pragma_query_value(None, "user_version", |r| r.get(0))?;
    assert_eq!(version, 99);
    Ok(())
}

#[test]
fn corrupt_file_is_reported_and_preserved() -> TestResult {
    let tmp = TempDir::new()?;
    let path = tmp.0.join("broken.sqlite3");
    let garbage = vec![b'x'; 4096];
    fs::write(&path, &garbage)?;
    assert!(matches!(Store::open(&path), Err(StorageError::Corrupt)));
    assert_eq!(fs::read(&path)?, garbage);
    Ok(())
}

#[test]
fn files_and_directories_are_private() -> TestResult {
    let tmp = TempDir::new()?;
    let dir = tmp.0.join("a/b");
    let path = dir.join("db.sqlite3");
    let mut store = Store::open(&path)?;
    store.record_call(&call(
        "s:1",
        1,
        Direction::Incoming,
        "1001",
        CallOutcome::Missed,
    ))?;
    assert_eq!(mode(&tmp.0.join("a"))?, 0o700);
    assert_eq!(mode(&dir)?, 0o700);
    assert_eq!(mode(&path)?, 0o600);
    for suffix in ["-wal", "-shm"] {
        let companion = dir.join(format!("db.sqlite3{suffix}"));
        assert_eq!(mode(&companion)?, 0o600, "{suffix}");
    }
    drop(store);

    let loose = tmp.0.join("loose.sqlite3");
    fs::write(&loose, b"")?;
    fs::set_permissions(&loose, fs::Permissions::from_mode(0o644))?;
    drop(Store::open(&loose)?);
    assert_eq!(mode(&loose)?, 0o600);

    let link = tmp.0.join("link.sqlite3");
    std::os::unix::fs::symlink(&loose, &link)?;
    assert!(matches!(
        Store::open(&link),
        Err(StorageError::NotRegularFile)
    ));
    Ok(())
}

#[test]
fn outcome_mapping_follows_establishment_and_direction() {
    use CallOutcome as O;
    use Direction::{Incoming as In, Outgoing as Out};
    let cases = [
        (In, true, EndReason::Normal, O::Answered),
        (Out, true, EndReason::Failed, O::Answered),
        (In, false, EndReason::Normal, O::Missed),
        (In, false, EndReason::Canceled, O::Missed),
        (In, false, EndReason::NoAnswer, O::Missed),
        (In, false, EndReason::Rejected, O::Rejected),
        (Out, false, EndReason::Canceled, O::Canceled),
        (Out, false, EndReason::Busy, O::Busy),
        (Out, false, EndReason::NoAnswer, O::NoAnswer),
        (Out, false, EndReason::NotAcceptable, O::Failed),
        (Out, false, EndReason::Rejected, O::Rejected),
    ];
    for (direction, established, reason, expected) in cases {
        assert_eq!(
            CallOutcome::from_end(direction, established, reason),
            expected
        );
    }
}

#[test]
fn history_round_trip_dedup_filters_search_and_retention() -> TestResult {
    let mut store = Store::open_in_memory()?;
    let now = now_utc();
    let mut answered = call(
        "s:1",
        now - 60,
        Direction::Outgoing,
        "1002",
        CallOutcome::Answered,
    );
    answered.answered_utc = Some(now - 58);
    answered.duration_ms = Some(3_500);
    answered.secure = true;
    assert!(store.record_call(&answered)?);
    assert!(
        !store.record_call(&answered)?,
        "duplicate session key is ignored"
    );
    store.record_call(&call(
        "s:2",
        now - 30,
        Direction::Incoming,
        "100%_x",
        CallOutcome::Missed,
    ))?;
    store.record_call(&call(
        "s:3",
        now - 100 * 86_400,
        Direction::Incoming,
        "1003",
        CallOutcome::Interrupted,
    ))?;

    let all = store.recent_calls(10, HistoryFilter::All, None)?;
    assert_eq!(
        all.iter()
            .map(|e| e.call.session_key.as_str())
            .collect::<Vec<_>>(),
        ["s:2", "s:1", "s:3"]
    );
    assert_eq!(all[1].call, answered);
    assert_eq!(store.recent_calls(1, HistoryFilter::All, None)?.len(), 1);
    assert_eq!(
        store.recent_calls(10, HistoryFilter::Outgoing, None)?.len(),
        1
    );
    assert_eq!(
        store.recent_calls(10, HistoryFilter::Incoming, None)?.len(),
        2
    );
    let missed = store.recent_calls(10, HistoryFilter::Missed, None)?;
    assert_eq!(missed.len(), 1);
    assert_eq!(missed[0].call.remote, "100%_x");
    assert_eq!(
        store.recent_calls(10, HistoryFilter::All, Some("%"))?.len(),
        1
    );
    assert_eq!(
        store
            .recent_calls(10, HistoryFilter::All, Some("0_"))?
            .len(),
        0
    );
    assert_eq!(
        store
            .recent_calls(10, HistoryFilter::All, Some("100"))?
            .len(),
        3
    );

    assert_eq!(store.prune_older_than(DEFAULT_RETENTION_DAYS)?, 1);
    let id = all[0].id;
    store.delete_call(id)?;
    assert!(matches!(store.delete_call(id), Err(StorageError::NotFound)));
    assert_eq!(store.clear_history()?, 1);
    assert!(store.recent_calls(10, HistoryFilter::All, None)?.is_empty());

    let mut bad = call(
        "s:4",
        now,
        Direction::Incoming,
        "evil\r\nX",
        CallOutcome::Missed,
    );
    let error = store.record_call(&bad).err().ok_or("expected error")?;
    assert!(!error.to_string().contains("evil"));
    bad.remote = "ok".into();
    bad.session_key.clear();
    assert!(matches!(
        store.record_call(&bad),
        Err(StorageError::Invalid(InvalidField::SessionKey))
    ));
    Ok(())
}

#[test]
fn contacts_crud_favorites_and_literal_search() -> TestResult {
    let mut store = Store::open_in_memory()?;
    let mut alice = contact(" Alice 100% ", true, &["1001", "+5511999990000"]);
    alice.numbers[0].label = Some("work".into());
    let a = store.add_contact(&alice)?;
    let b = store.add_contact(&contact("bob_lab", false, &["2002"]))?;
    let c = store.add_contact(&contact("Carol", false, &["3003"]))?;

    let got = store.contact(a)?.ok_or("missing")?;
    assert_eq!(got.name, "Alice 100%");
    assert!(got.favorite);
    assert_eq!(got.numbers.len(), 2);
    assert_eq!(got.numbers[0].label.as_deref(), Some("work"));

    let names = |list: Vec<Contact>| list.into_iter().map(|c| c.name).collect::<Vec<_>>();
    assert_eq!(
        names(store.search_contacts("", 10)?),
        ["Alice 100%", "bob_lab", "Carol"]
    );
    assert_eq!(names(store.search_contacts("%", 10)?), ["Alice 100%"]);
    assert_eq!(names(store.search_contacts("b_l", 10)?), ["bob_lab"]);
    assert_eq!(
        names(store.search_contacts("o_", 10)?),
        Vec::<String>::new()
    );
    assert_eq!(names(store.search_contacts("CAROL", 10)?), ["Carol"]);
    assert_eq!(names(store.search_contacts("99999", 10)?), ["Alice 100%"]);
    assert_eq!(store.search_contacts("", 1)?.len(), 1);

    store.set_favorite(c, true)?;
    store.set_favorite(a, false)?;
    assert_eq!(names(store.search_contacts("", 10)?)[0], "Carol");

    store.update_contact(b, &contact("Bob", true, &["2003", "2004"]))?;
    let bob = store.contact(b)?.ok_or("missing")?;
    assert_eq!(
        (bob.name.as_str(), bob.favorite, bob.numbers.len()),
        ("Bob", true, 2)
    );
    assert!(matches!(
        store.update_contact(999, &contact("X", false, &["1"])),
        Err(StorageError::NotFound)
    ));

    store.delete_contact(b)?;
    assert!(store.contact(b)?.is_none());
    assert_eq!(
        count(&store, "contact_numbers")?,
        3,
        "numbers cascade with the contact"
    );
    assert!(matches!(
        store.delete_contact(b),
        Err(StorageError::NotFound)
    ));
    assert!(matches!(
        store.set_favorite(b, true),
        Err(StorageError::NotFound)
    ));

    for (bad, field) in [
        (contact("X", false, &["sip:1@x"]), InvalidField::Number),
        (
            contact("X", false, &["1", " 1 "]),
            InvalidField::DuplicateNumber,
        ),
        (contact("  ", false, &["1"]), InvalidField::Name),
        (contact("X", false, &[]), InvalidField::NumberCount),
    ] {
        assert!(matches!(store.add_contact(&bad), Err(StorageError::Invalid(f)) if f == field));
    }
    assert_eq!(count(&store, "contacts")?, 2);
    Ok(())
}

#[test]
fn csv_preview_handles_quotes_crlf_bom_duplicates_and_issues() -> TestResult {
    let text = "\u{feff}Name,Number,Label\r\n\
                \"Doe, Jane\",1001,\"say \"\"hi\"\"\"\r\n\
                \"Doe, Jane\",1002,\r\n\
                \"Doe, Jane\",1001,dup\r\n\
                \r\n\
                Bad,sip:1@x,\n\
                \"multi\nline\",1003,\n\
                Short,1004\n\
                Stray\"quote,1005,\n\
                Ok,+5511,\n\
                \"open,1006,\n";
    let preview = preview_csv(text)?;
    assert_eq!(preview.record_count(), 9);
    assert_eq!(preview.duplicate_count(), 1);
    assert_eq!(preview.contacts().len(), 2);
    let jane = &preview.contacts()[0];
    assert_eq!(jane.name, "Doe, Jane");
    assert_eq!(jane.numbers.len(), 2);
    assert_eq!(jane.numbers[0].label.as_deref(), Some("say \"hi\""));
    assert_eq!(jane.numbers[1].label, None);
    let issues: Vec<(usize, CsvIssueKind)> =
        preview.issues().iter().map(|i| (i.line, i.kind)).collect();
    assert_eq!(
        issues,
        [
            (6, CsvIssueKind::InvalidNumber),
            (7, CsvIssueKind::InvalidName),
            (9, CsvIssueKind::ColumnCount),
            (10, CsvIssueKind::Malformed),
            (12, CsvIssueKind::Malformed),
        ]
    );
    assert!(!preview.is_importable());

    let mut store = Store::open_in_memory()?;
    assert!(matches!(
        store.import_csv(&preview),
        Err(StorageError::CsvInvalid)
    ));
    assert_eq!(count(&store, "contacts")?, 0);
    Ok(())
}

#[test]
fn csv_limits_and_header_are_enforced() {
    assert!(matches!(preview_csv(""), Err(StorageError::CsvHeader)));
    assert!(matches!(
        preview_csv("number,name\n1,a\n"),
        Err(StorageError::CsvHeader)
    ));
    let big = format!("name,number\n{}", "x".repeat(CSV_MAX_BYTES));
    assert!(matches!(preview_csv(&big), Err(StorageError::CsvTooLarge)));
    let many = format!("name,number\n{}", "a,1\n".repeat(CSV_MAX_RECORDS + 1));
    assert!(matches!(
        preview_csv(&many),
        Err(StorageError::CsvTooManyRecords)
    ));
    let max = format!("name,number\n{}", "a,1\n".repeat(CSV_MAX_RECORDS));
    assert!(preview_csv(&max).is_ok_and(|p| p.duplicate_count() == CSV_MAX_RECORDS - 1));
}

#[test]
fn csv_import_merges_by_name_and_is_atomic() -> TestResult {
    let mut store = Store::open_in_memory()?;
    store.add_contact(&contact("Jane", true, &["1001"]))?;
    let preview = preview_csv("name,number\nJane,1001\nJane,1002\nZed,2001\nZed,2002\n")?;
    let summary = store.import_csv(&preview)?;
    assert_eq!(
        summary,
        ImportSummary {
            contacts_created: 1,
            contacts_merged: 1,
            numbers_added: 3
        }
    );
    let jane = store.search_contacts("Jane", 10)?;
    assert_eq!(jane.len(), 1);
    assert_eq!(jane[0].numbers.len(), 2);
    assert!(jane[0].favorite);

    // Force a failure on the last insert: nothing from this import may remain.
    store.conn.execute_batch(
        "CREATE TEMP TRIGGER fail BEFORE INSERT ON contact_numbers
         WHEN NEW.number = '9999' BEGIN SELECT RAISE(ABORT, 'forced'); END;",
    )?;
    let (contacts, numbers) = (
        count(&store, "contacts")?,
        count(&store, "contact_numbers")?,
    );
    let preview = preview_csv("name,number\nNew One,5001\nJane,1003\nLast,9999\n")?;
    assert!(store.import_csv(&preview).is_err());
    assert_eq!(
        (
            count(&store, "contacts")?,
            count(&store, "contact_numbers")?
        ),
        (contacts, numbers)
    );
    assert!(store.search_contacts("New One", 10)?.is_empty());
    Ok(())
}

#[test]
fn backup_is_private_readable_and_never_overwrites() -> TestResult {
    let tmp = TempDir::new()?;
    let mut store = Store::open(&tmp.0.join("db.sqlite3"))?;
    store.add_contact(&contact("Backup Test", false, &["1001"]))?;
    let target = tmp.0.join("backup.sqlite3");
    store.backup_to(&target)?;
    assert_eq!(mode(&target)?, 0o600);
    assert!(matches!(
        store.backup_to(&target),
        Err(StorageError::BackupExists)
    ));
    let copy = Store::open(&target)?;
    assert_eq!(copy.schema_version()?, SCHEMA_VERSION);
    assert_eq!(copy.search_contacts("Backup", 10)?.len(), 1);
    Ok(())
}
