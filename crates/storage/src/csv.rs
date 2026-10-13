//! Contact CSV preview: RFC 4180 subset, `name,number[,label]` header.

use crate::{
    ContactNumber, MAX_LABEL_LEN, MAX_NAME_LEN, MAX_NUMBERS_PER_CONTACT, NewContact, Result,
    StorageError, normalize_number, text_ok,
};
use std::collections::HashMap;

/// Maximum CSV input size in bytes (10 MiB).
pub const CSV_MAX_BYTES: usize = 10 * 1024 * 1024;
/// Maximum data records per CSV file, excluding the header.
pub const CSV_MAX_RECORDS: usize = 100_000;

/// Why a CSV row was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsvIssueKind {
    /// Broken quoting (stray or unterminated quote).
    Malformed,
    /// Field count differs from the header.
    ColumnCount,
    /// Name empty, too long or containing control characters.
    InvalidName,
    /// Number is not a valid dial target.
    InvalidNumber,
    /// Label too long or containing control characters.
    InvalidLabel,
    /// The contact would exceed the per-contact number limit.
    TooManyNumbers,
}

/// One rejected row; `line` is the 1-based line where the record starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CsvIssue {
    /// 1-based starting line of the record.
    pub line: usize,
    /// Problem found.
    pub kind: CsvIssueKind,
}

/// Validated CSV content ready for confirmation and [`crate::Store::import_csv`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CsvPreview {
    contacts: Vec<NewContact>,
    records: usize,
    duplicates: usize,
    issues: Vec<CsvIssue>,
}

impl CsvPreview {
    /// Contacts to import; rows with the same name are aggregated.
    pub fn contacts(&self) -> &[NewContact] {
        &self.contacts
    }
    /// Data records read (excluding header and blank lines).
    pub fn record_count(&self) -> usize {
        self.records
    }
    /// Exact duplicate rows (same name and number) that will be skipped.
    pub fn duplicate_count(&self) -> usize {
        self.duplicates
    }
    /// Rejected rows. Any issue blocks the whole import.
    pub fn issues(&self) -> &[CsvIssue] {
        &self.issues
    }
    /// True when there are no issues and at least one contact.
    pub fn is_importable(&self) -> bool {
        self.issues.is_empty() && !self.contacts.is_empty()
    }
}

struct Row {
    line: usize,
    fields: Vec<String>,
    malformed: bool,
}

/// Parse and validate CSV text. Input is UTF-8 by type; a leading BOM is
/// ignored. The first non-blank record must be the header `name,number` or
/// `name,number,label` (case-insensitive).
///
/// # Errors
/// [`StorageError::CsvTooLarge`], [`StorageError::CsvTooManyRecords`] or
/// [`StorageError::CsvHeader`]. Row-level problems are reported in
/// [`CsvPreview::issues`] instead.
pub fn preview_csv(input: &str) -> Result<CsvPreview> {
    if input.len() > CSV_MAX_BYTES {
        return Err(StorageError::CsvTooLarge);
    }
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut rows = parse(input)?.into_iter();
    let header = rows.next().ok_or(StorageError::CsvHeader)?;
    let names: Vec<String> = header
        .fields
        .iter()
        .map(|f| f.trim().to_ascii_lowercase())
        .collect();
    let columns = match names.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["name", "number"] if !header.malformed => 2,
        ["name", "number", "label"] if !header.malformed => 3,
        _ => return Err(StorageError::CsvHeader),
    };

    let mut preview = CsvPreview {
        contacts: Vec::new(),
        records: 0,
        duplicates: 0,
        issues: Vec::new(),
    };
    let mut by_name: HashMap<String, usize> = HashMap::new();
    for row in rows {
        preview.records += 1;
        let issue = |kind| CsvIssue {
            line: row.line,
            kind,
        };
        if row.malformed {
            preview.issues.push(issue(CsvIssueKind::Malformed));
            continue;
        }
        if row.fields.len() != columns {
            preview.issues.push(issue(CsvIssueKind::ColumnCount));
            continue;
        }
        let name = row.fields[0].trim();
        if !text_ok(name, MAX_NAME_LEN) {
            preview.issues.push(issue(CsvIssueKind::InvalidName));
            continue;
        }
        let Ok(number) = normalize_number(&row.fields[1]) else {
            preview.issues.push(issue(CsvIssueKind::InvalidNumber));
            continue;
        };
        let label = row
            .fields
            .get(2)
            .map(|l| l.trim())
            .filter(|l| !l.is_empty());
        if label.is_some_and(|l| !text_ok(l, MAX_LABEL_LEN)) {
            preview.issues.push(issue(CsvIssueKind::InvalidLabel));
            continue;
        }
        let index = *by_name.entry(name.to_owned()).or_insert_with(|| {
            preview.contacts.push(NewContact {
                name: name.to_owned(),
                favorite: false,
                numbers: Vec::new(),
            });
            preview.contacts.len() - 1
        });
        let numbers = &mut preview.contacts[index].numbers;
        if numbers.iter().any(|n| n.number == number) {
            preview.duplicates += 1;
        } else if numbers.len() >= MAX_NUMBERS_PER_CONTACT {
            preview.issues.push(issue(CsvIssueKind::TooManyNumbers));
        } else {
            numbers.push(ContactNumber {
                number,
                label: label.map(str::to_owned),
            });
        }
    }
    // A name whose every row was invalid leaves no entry; empty ones cannot occur.
    Ok(preview)
}

/// Split into records, skipping blank lines; bounded by [`CSV_MAX_RECORDS`].
fn parse(input: &str) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    let mut chars = input.chars().peekable();
    let mut line = 1;
    let mut row = Row {
        line,
        fields: Vec::new(),
        malformed: false,
    };
    let mut field = String::new();
    let (mut in_quotes, mut quoted, mut closed) = (false, false, false);
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => (in_quotes, closed) = (false, true),
                _ => {
                    if c == '\n' {
                        line += 1;
                    }
                    field.push(c);
                }
            }
            continue;
        }
        match c {
            ',' => {
                row.fields.push(std::mem::take(&mut field));
                (quoted, closed) = (false, false);
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                row.fields.push(std::mem::take(&mut field));
                (quoted, closed) = (false, false);
                line += 1;
                let done = std::mem::replace(
                    &mut row,
                    Row {
                        line,
                        fields: Vec::new(),
                        malformed: false,
                    },
                );
                push_row(done, &mut rows)?;
            }
            '"' if field.is_empty() && !quoted => (in_quotes, quoted) = (true, true),
            _ => {
                if c == '"' || closed {
                    row.malformed = true;
                }
                field.push(c);
            }
        }
    }
    if in_quotes {
        row.malformed = true;
    }
    if in_quotes || !field.is_empty() || !row.fields.is_empty() || quoted {
        row.fields.push(field);
        push_row(row, &mut rows)?;
    }
    Ok(rows)
}

fn push_row(row: Row, rows: &mut Vec<Row>) -> Result<()> {
    let blank = !row.malformed && row.fields.len() == 1 && row.fields[0].is_empty();
    if !blank {
        if rows.len() > CSV_MAX_RECORDS {
            return Err(StorageError::CsvTooManyRecords);
        }
        rows.push(row);
    }
    Ok(())
}
