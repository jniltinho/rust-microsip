//! `config import`: validate a TOML file and preview what would be stored.
//!
//! The preview never writes anything; the caller applies
//! [`ImportPreview::sqlite_values`] with `Store::replace_config` (one
//! transaction) only after confirmation. The TOML file is only read, and
//! settings of other applications (MicroSIP/GOnnect INI) are never read.

use crate::{ConfigError, Field, Layer, Source, build, files, toml_layer};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// Printed after a successful `config import --apply`.
pub const IMPORT_APPLIED: &str = "Imported into the local database. The TOML file was not modified; \
settings it still provides keep precedence over the database.\n";

/// Validated, secret-free content of a TOML file to import.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportPreview {
    /// The file that was read.
    pub file: PathBuf,
    /// Fields present in the file (never a password).
    pub values: BTreeMap<Field, String>,
    /// The file had a literal `account.password`; it is not imported.
    pub literal_password_ignored: bool,
}

impl ImportPreview {
    /// The values keyed as `Store::replace_config` expects.
    pub fn sqlite_values(&self) -> BTreeMap<String, String> {
        self.values
            .iter()
            .map(|(f, v)| (f.key().to_owned(), v.clone()))
            .collect()
    }
}

impl fmt::Display for ImportPreview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "import preview of {} (nothing written yet):",
            self.file.display()
        )?;
        for (field, value) in &self.values {
            writeln!(f, "  {field} = {}", value.escape_debug())?;
        }
        if self.literal_password_ignored {
            writeln!(
                f,
                "warning: the literal password is not imported; use password_env or a private password_file"
            )?;
        }
        writeln!(
            f,
            "Run again with --apply to replace the settings stored in the local database."
        )
    }
}

/// Read and fully validate `path` as an import source.
///
/// The file must pass the same checks as `--config` (regular file, bounded,
/// not group/other-writable, private when it holds a literal password) and,
/// together with built-in defaults, form a valid account.
///
/// # Errors
/// [`ConfigError::FileMissing`], file/permission/syntax errors, or
/// [`ConfigError::Missing`]/[`ConfigError::Invalid`] naming the field; never
/// the values.
pub fn preview_import(path: &Path, uid: u32) -> Result<ImportPreview, ConfigError> {
    let loaded = files::read_bounded(path, files::MAX_CONFIG_BYTES, false, uid)?
        .ok_or_else(|| ConfigError::FileMissing(path.to_owned()))?;
    let (values, literal): (Layer, _) = toml_layer(&loaded.text, path)?;
    if literal.is_some() {
        files::require_private(path, &loaded.meta, uid)?;
    }
    build(vec![(Source::Toml, values.clone())])?;
    Ok(ImportPreview {
        file: path.to_owned(),
        values,
        literal_password_ignored: literal.is_some(),
    })
}
