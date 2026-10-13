//! Sanitized configuration report (`--doctor`).

use crate::{ConfigError, EffectiveConfig, Field, PasswordSource, Source};
use std::fmt::{self, Write as _};

/// One effective field in a [`DoctorReport`].
#[derive(Clone, Debug, PartialEq)]
pub struct DoctorField {
    /// The field.
    pub field: Field,
    /// Effective value (never a secret), `None` when unset.
    pub value: Option<toml::Value>,
    /// Where the value came from.
    pub source: Source,
}

/// Sanitized report: effective values with provenance, secrets only as booleans.
/// It never registers or dials.
#[derive(Clone, Debug, PartialEq)]
pub struct DoctorReport {
    /// Whether the configuration resolved and validated.
    pub ok: bool,
    /// The (value-free) error message when `ok` is false.
    pub error: Option<String>,
    /// The TOML file that was loaded.
    pub config_file: Option<String>,
    /// Effective fields; empty when resolution failed.
    pub fields: Vec<DoctorField>,
    /// A password is available without prompting.
    pub password_present: bool,
    /// Which source provided the password.
    pub password_source: Option<PasswordSource>,
    /// A literal password was found in TOML.
    pub literal_password_warning: bool,
}
impl DoctorReport {
    /// Build a report from a resolution result.
    pub fn new(result: &Result<EffectiveConfig, ConfigError>) -> Self {
        match result {
            Ok(cfg) => Self {
                ok: true,
                error: None,
                config_file: cfg.config_file.as_ref().map(|p| p.display().to_string()),
                fields: Field::ALL
                    .into_iter()
                    .map(|field| DoctorField {
                        field,
                        value: cfg.config.value(field),
                        source: cfg.source(field),
                    })
                    .collect(),
                password_present: cfg.password.is_some(),
                password_source: cfg.password_source,
                literal_password_warning: cfg.literal_password_warning,
            },
            Err(e) => Self {
                ok: false,
                error: Some(e.to_string()),
                config_file: None,
                fields: Vec::new(),
                password_present: false,
                password_source: None,
                literal_password_warning: false,
            },
        }
    }

    /// Render as a single JSON object.
    pub fn to_json(&self) -> String {
        let opt = |s: Option<&str>| s.map_or_else(|| "null".to_owned(), json_string);
        let mut out = format!(
            "{{\"ok\":{},\"error\":{},\"config_file\":{},\"password_present\":{},\"password_source\":{},\"literal_password_warning\":{},\"fields\":[",
            self.ok,
            opt(self.error.as_deref()),
            opt(self.config_file.as_deref()),
            self.password_present,
            opt(self.password_source.map(PasswordSource::as_str)),
            self.literal_password_warning,
        );
        for (i, f) in self.fields.iter().enumerate() {
            let value = match &f.value {
                None => "null".to_owned(),
                Some(toml::Value::String(s)) => json_string(s),
                Some(toml::Value::Integer(n)) => n.to_string(),
                Some(toml::Value::Boolean(b)) => b.to_string(),
                Some(_) => "null".to_owned(),
            };
            let sep = if i == 0 { "" } else { "," };
            let _ = write!(
                out,
                "{sep}{{\"key\":{},\"value\":{value},\"source\":\"{}\"}}",
                json_string(f.field.key()),
                f.source
            );
        }
        out.push_str("]}");
        out
    }
}
impl fmt::Display for DoctorReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(error) = &self.error {
            return writeln!(f, "configuration: INVALID\nerror: {error}");
        }
        writeln!(f, "configuration: OK")?;
        writeln!(
            f,
            "config file: {}",
            self.config_file.as_deref().unwrap_or("(none)")
        )?;
        for field in &self.fields {
            let value = field
                .value
                .as_ref()
                .map_or_else(|| "(unset)".to_owned(), ToString::to_string);
            writeln!(
                f,
                "{} = {} [{}]",
                field.field,
                value.escape_debug(),
                field.source
            )?;
        }
        let source = self
            .password_source
            .map_or("none (prompt per session)", PasswordSource::as_str);
        writeln!(
            f,
            "password: {} [{source}]",
            if self.password_present {
                "present"
            } else {
                "absent"
            }
        )?;
        if self.literal_password_warning {
            writeln!(f, "warning: literal password in TOML; prefer password_env")?;
        }
        Ok(())
    }
}

/// Quote and escape `s` as a JSON string.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{7f}' || c == '\u{2028}' || c == '\u{2029}' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::json_string;

    #[test]
    fn json_strings_are_escaped() {
        assert_eq!(
            json_string("a\"b\\c\n\u{1}\u{2028}é"),
            "\"a\\\"b\\\\c\\n\\u0001\\u2028é\""
        );
    }
}
