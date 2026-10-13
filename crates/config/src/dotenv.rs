//! Minimal `.env` parser.

use std::collections::BTreeMap;

/// `.env` syntax error; carries only the line number, never the content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("invalid .env syntax at line {line}")]
pub struct DotEnvError {
    /// 1-based line number.
    pub line: usize,
}

/// Parse `KEY=VALUE` lines.
///
/// Supported: blank lines, `#` comments, an optional `export ` prefix, CRLF line
/// endings, unquoted values (trimmed; ` #` starts a comment), single-quoted
/// literal values, and double-quoted values with `\n`, `\r`, `\t`, `\"` and `\\`
/// escapes. Multi-line values and variable expansion are not supported. Later
/// duplicates win. The result may contain secrets; do not log it. Size is bounded
/// by the caller.
///
/// # Errors
/// [`DotEnvError`] with the first offending line.
pub fn parse_dotenv(text: &str) -> Result<BTreeMap<String, String>, DotEnvError> {
    let mut map = BTreeMap::new();
    for (index, raw) in text.split('\n').enumerate() {
        let err = DotEnvError { line: index + 1 };
        let line = raw.strip_suffix('\r').unwrap_or(raw).trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").map_or(line, str::trim_start);
        let (key, rest) = line.split_once('=').ok_or(err)?;
        let key = key.trim_end();
        let mut chars = key.chars();
        let key_ok = chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !key_ok {
            return Err(err);
        }
        let rest = rest.trim_start();
        let (value, tail) = match rest.chars().next() {
            Some('\'') => {
                let body = &rest[1..];
                let end = body.find('\'').ok_or(err)?;
                (body[..end].to_owned(), &body[end + 1..])
            }
            Some('"') => {
                let mut value = String::new();
                let mut iter = rest[1..].char_indices();
                let mut tail = None;
                while let Some((i, c)) = iter.next() {
                    match c {
                        '"' => {
                            tail = Some(&rest[1 + i + 1..]);
                            break;
                        }
                        '\\' => value.push(match iter.next().ok_or(err)?.1 {
                            'n' => '\n',
                            'r' => '\r',
                            't' => '\t',
                            c @ ('"' | '\\') => c,
                            _ => return Err(err),
                        }),
                        c => value.push(c),
                    }
                }
                (value, tail.ok_or(err)?)
            }
            _ => {
                let end = rest
                    .char_indices()
                    .find(|&(i, c)| {
                        c == '#' && (i == 0 || rest[..i].ends_with(char::is_whitespace))
                    })
                    .map_or(rest.len(), |(i, _)| i);
                (rest[..end].trim_end().to_owned(), "")
            }
        };
        let tail = tail.trim_start();
        if !(tail.is_empty() || tail.starts_with('#')) {
            return Err(err);
        }
        map.insert(key.to_owned(), value);
    }
    Ok(map)
}
