//! Configuration resolution for rust-microsip, without persisting secrets.
//!
//! Every field is resolved independently, first source wins:
//!
//! 1. per-field command-line overrides ([`ResolveContext::flags`]),
//! 2. the process environment (`RUST_MICROSIP_<SECTION>_<KEY>`, see [`Field::env_name`]),
//! 3. an explicit `.env` file (only from `--env-file`; the current directory is never searched),
//! 4. one TOML file: the explicit `--config` path *replaces* the XDG default
//!    `$XDG_CONFIG_HOME/rust-microsip/config.toml` (fallback `$HOME/.config/...`),
//! 5. built-in defaults.
//!
//! SQLite persistence will slot in between TOML and defaults in a later slice; the
//! resolver already treats sources as ordered layers so it is one more layer.
//!
//! Each resolved field records its [`Source`]. A missing required field
//! ([`ConfigError::Missing`]) is distinct from a present but invalid one
//! ([`ConfigError::Invalid`]). Errors identify fields, sources, files and line
//! numbers, never the offending values, because those may contain secrets.
//!
//! # Secrets
//!
//! Passwords are never written by [`save`], [`StoredConfig::to_toml`] or the
//! [`DoctorReport`]. A password is taken, in order, from the environment variable
//! named by `account.password_env` (process environment, then the `.env` file), from
//! the private file `account.password_file`, or from a literal `account.password` in
//! TOML (accepted in memory only, flagged by
//! [`EffectiveConfig::literal_password_warning`]). Otherwise the UI asks per session.
//! Any file that actually contains a secret must be a regular file owned by the
//! current user with mode `0600` (no group/other bits); the password file must also
//! not be a symlink.
//!
//! # File permissions
//!
//! Configuration and `.env` files that are group- or other-writable are rejected,
//! because another user could inject settings such as the registrar host.

mod cli;
mod doctor;
mod dotenv;
mod files;

pub use cli::{CliArgs, CliCommand, CliError, HELP, parse_args};
pub use doctor::{DoctorField, DoctorReport};
pub use dotenv::{DotEnvError, parse_dotenv};
pub use files::{FileProblem, MAX_CA_BYTES, read_ca_file, save};

use rust_microsip_core::{AccountSettings, MediaSecurity, Transport, ValidationError};
use secrecy::SecretString;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// Supported value of the top-level `schema` key.
pub const SCHEMA_VERSION: i64 = 1;
/// Environment variable read for the password when `account.password_env` is unset.
pub const DEFAULT_PASSWORD_ENV: &str = "RUST_MICROSIP_SIP_PASSWORD";
/// Default registration lifetime in seconds.
pub const DEFAULT_EXPIRES: u32 = 300;

/// Where an effective value came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// A per-field command-line override.
    Flag,
    /// The process environment.
    Env,
    /// The explicit `.env` file.
    DotEnv,
    /// The TOML configuration file.
    Toml,
    /// The built-in default (or unset for optional fields).
    Default,
}
impl Source {
    /// Stable lower-case label used in diagnostics and JSON.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Flag => "flag",
            Self::Env => "env",
            Self::DotEnv => "dotenv",
            Self::Toml => "toml",
            Self::Default => "default",
        }
    }
}
impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Str,
    Int,
    Bool,
}

/// One configurable, non-secret field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Field {
    /// `account.display_name`
    DisplayName,
    /// `account.username`
    Username,
    /// `account.auth_username`
    AuthUsername,
    /// `account.domain`
    Domain,
    /// `account.server` (defaults to the domain)
    Server,
    /// `account.port` (defaults to the transport's port)
    Port,
    /// `account.transport`: `udp`, `tcp` or `tls`
    Transport,
    /// `account.media`: `rtp` or `srtp`
    Media,
    /// `account.expires`, seconds
    Expires,
    /// `account.password_env`: name of the variable holding the password
    PasswordEnv,
    /// `account.password_file`: absolute path of a private (0600) password file
    PasswordFile,
    /// `account.ca_file`: absolute path of a PEM CA bundle for TLS
    CaFile,
    /// `ui.theme`: `system`, `light` or `dark`
    Theme,
    /// `ui.dnd`: do-not-disturb
    Dnd,
    /// `audio.input_device`
    InputDevice,
    /// `audio.output_device`
    OutputDevice,
}
impl Field {
    /// Every field, in schema order.
    pub const ALL: [Self; 16] = [
        Self::DisplayName,
        Self::Username,
        Self::AuthUsername,
        Self::Domain,
        Self::Server,
        Self::Port,
        Self::Transport,
        Self::Media,
        Self::Expires,
        Self::PasswordEnv,
        Self::PasswordFile,
        Self::CaFile,
        Self::Theme,
        Self::Dnd,
        Self::InputDevice,
        Self::OutputDevice,
    ];

    /// Dotted TOML key, such as `account.server`.
    pub const fn key(self) -> &'static str {
        match self {
            Self::DisplayName => "account.display_name",
            Self::Username => "account.username",
            Self::AuthUsername => "account.auth_username",
            Self::Domain => "account.domain",
            Self::Server => "account.server",
            Self::Port => "account.port",
            Self::Transport => "account.transport",
            Self::Media => "account.media",
            Self::Expires => "account.expires",
            Self::PasswordEnv => "account.password_env",
            Self::PasswordFile => "account.password_file",
            Self::CaFile => "account.ca_file",
            Self::Theme => "ui.theme",
            Self::Dnd => "ui.dnd",
            Self::InputDevice => "audio.input_device",
            Self::OutputDevice => "audio.output_device",
        }
    }

    /// Environment variable name: `RUST_MICROSIP_` plus the upper-cased key with
    /// `.` replaced by `_`, such as `RUST_MICROSIP_ACCOUNT_SERVER`.
    pub fn env_name(self) -> String {
        format!(
            "RUST_MICROSIP_{}",
            self.key().replace('.', "_").to_ascii_uppercase()
        )
    }

    const fn kind(self) -> Kind {
        match self {
            Self::Port | Self::Expires => Kind::Int,
            Self::Dnd => Kind::Bool,
            _ => Kind::Str,
        }
    }

    fn split_key(self) -> (&'static str, &'static str) {
        self.key().split_once('.').unwrap_or(("", self.key()))
    }
}
impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// UI color theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    /// Follow the desktop preference.
    #[default]
    System,
    /// Light theme.
    Light,
    /// Dark theme.
    Dark,
}
impl Theme {
    /// Configuration label.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

/// `[ui]` section.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UiSettings {
    /// Color theme.
    pub theme: Theme,
    /// Do-not-disturb: reject new incoming calls.
    pub dnd: bool,
}

/// `[audio]` section. `None` means the system default device.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AudioSettings {
    /// Capture device name.
    pub input_device: Option<String>,
    /// Playback device name.
    pub output_device: Option<String>,
}

/// Secret-free configuration: what [`save`] writes and `config export` prints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredConfig {
    /// Validated account settings.
    pub account: AccountSettings,
    /// Name of the environment variable that holds the password.
    pub password_env: String,
    /// Optional absolute path of a private password file.
    pub password_file: Option<PathBuf>,
    /// Optional absolute path of a PEM CA bundle for TLS.
    pub ca_file: Option<PathBuf>,
    /// UI preferences.
    pub ui: UiSettings,
    /// Audio device preferences.
    pub audio: AudioSettings,
}
impl StoredConfig {
    /// The value of `field` as a typed TOML value, or `None` when unset.
    pub fn value(&self, field: Field) -> Option<toml::Value> {
        use toml::Value as V;
        let path = |p: &Option<PathBuf>| {
            p.as_deref()
                .map(|p| V::String(p.to_string_lossy().into_owned()))
        };
        let a = &self.account;
        Some(match field {
            Field::DisplayName => V::String(a.display_name.clone()),
            Field::Username => V::String(a.username.clone()),
            Field::AuthUsername => V::String(a.auth_username.clone()?),
            Field::Domain => V::String(a.domain.clone()),
            Field::Server => V::String(a.server.clone()),
            Field::Port => V::Integer(a.port.into()),
            Field::Transport => V::String(a.transport.as_str().into()),
            Field::Media => V::String(media_str(a.media).into()),
            Field::Expires => V::Integer(a.expires.into()),
            Field::PasswordEnv => V::String(self.password_env.clone()),
            Field::PasswordFile => path(&self.password_file)?,
            Field::CaFile => path(&self.ca_file)?,
            Field::Theme => V::String(self.ui.theme.as_str().into()),
            Field::Dnd => V::Boolean(self.ui.dnd),
            Field::InputDevice => V::String(self.audio.input_device.clone()?),
            Field::OutputDevice => V::String(self.audio.output_device.clone()?),
        })
    }

    /// Render as schema-1 TOML. Never contains a password.
    ///
    /// # Errors
    /// [`ConfigError::NonUtf8Path`] when a path cannot be represented in TOML.
    pub fn to_toml(&self) -> Result<String, ConfigError> {
        for path in [&self.password_file, &self.ca_file].into_iter().flatten() {
            if path.to_str().is_none() {
                return Err(ConfigError::NonUtf8Path);
            }
        }
        let mut root = toml::Table::new();
        root.insert("schema".into(), toml::Value::Integer(SCHEMA_VERSION));
        for field in Field::ALL {
            if let Some(value) = self.value(field) {
                let (section, key) = field.split_key();
                if let toml::Value::Table(table) = root
                    .entry(section)
                    .or_insert_with(|| toml::Value::Table(toml::Table::new()))
                {
                    table.insert(key.into(), value);
                }
            }
        }
        toml::to_string(&root).map_err(|_| ConfigError::Serialize)
    }
}

const fn media_str(media: MediaSecurity) -> &'static str {
    match media {
        MediaSecurity::PlainRtp => "rtp",
        MediaSecurity::Srtp => "srtp",
    }
}

/// Where the effective password came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasswordSource {
    /// The process environment variable named by `password_env`.
    Env,
    /// The same variable in the private `.env` file.
    DotEnv,
    /// The private `password_file`.
    File,
    /// A literal `account.password` in a private TOML file (discouraged).
    TomlLiteral,
}
impl PasswordSource {
    /// Stable lower-case label used in diagnostics and JSON.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::DotEnv => "dotenv",
            Self::File => "file",
            Self::TomlLiteral => "toml_literal",
        }
    }
}

/// Fully resolved configuration plus the optional session password.
///
/// `Debug` never shows the password. Persist only [`EffectiveConfig::config`]
/// deliberately: it contains values that may come from temporary overrides.
#[derive(Debug)]
pub struct EffectiveConfig {
    /// Effective, validated, secret-free values.
    pub config: StoredConfig,
    /// The password, when one of the configured sources provided it.
    pub password: Option<SecretString>,
    /// Which source provided [`EffectiveConfig::password`].
    pub password_source: Option<PasswordSource>,
    /// The TOML file held a literal password; recommend `password_env` instead.
    pub literal_password_warning: bool,
    /// The TOML file that was loaded, if any.
    pub config_file: Option<PathBuf>,
    provenance: BTreeMap<Field, Source>,
}
impl EffectiveConfig {
    /// Where the effective value of `field` came from.
    pub fn source(&self, field: Field) -> Source {
        self.provenance
            .get(&field)
            .copied()
            .unwrap_or(Source::Default)
    }
}

/// Configuration failure. Messages never include configuration values.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// An explicitly requested file does not exist.
    #[error("file not found: {}", .0.display())]
    FileMissing(PathBuf),
    /// A file could not be read or written.
    #[error("cannot access {}: {kind}", path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// The I/O failure class.
        kind: std::io::ErrorKind,
    },
    /// A file exceeds its size bound.
    #[error("file too large: {}", .0.display())]
    FileTooLarge(PathBuf),
    /// A file is not valid UTF-8.
    #[error("file is not valid UTF-8: {}", .0.display())]
    NotUtf8(PathBuf),
    /// A file has unsafe ownership, type or permissions.
    #[error("refusing {}: {problem}", path.display())]
    InsecureFile {
        /// The file.
        path: PathBuf,
        /// What is wrong and how to fix it.
        problem: FileProblem,
    },
    /// TOML syntax error.
    #[error("invalid TOML in {} at line {line}", path.display())]
    TomlSyntax {
        /// The file.
        path: PathBuf,
        /// 1-based line, 0 when unknown.
        line: usize,
    },
    /// `.env` syntax error.
    #[error("invalid .env syntax in {} at line {line}", path.display())]
    DotEnvSyntax {
        /// The file.
        path: PathBuf,
        /// 1-based line.
        line: usize,
    },
    /// Missing or unsupported `schema` version.
    #[error("{}: missing or unsupported schema version (expected `schema = {SCHEMA_VERSION}`)", .0.display())]
    UnsupportedSchema(PathBuf),
    /// A TOML key outside the schema.
    #[error("{}: unknown key `{key}`", path.display())]
    UnknownKey {
        /// The file.
        path: PathBuf,
        /// Dotted key (replaced by a placeholder when not printable).
        key: String,
    },
    /// A TOML value of the wrong type (for example a string for `port`).
    #[error("{}: wrong value type for `{key}`", path.display())]
    WrongType {
        /// The file.
        path: PathBuf,
        /// Dotted key.
        key: String,
    },
    /// A required field has no value in any source.
    #[error("missing required setting `{0}`")]
    Missing(Field),
    /// A field value could not be parsed.
    #[error("invalid value for `{field}` from {origin}")]
    Invalid {
        /// The field.
        field: Field,
        /// The source that supplied the invalid value.
        origin: Source,
    },
    /// The account settings failed domain validation.
    #[error("invalid account settings: {0}")]
    Validation(#[from] ValidationError),
    /// The password file is empty.
    #[error("password file is empty: {}", .0.display())]
    EmptyPassword(PathBuf),
    /// A path is not valid UTF-8 and cannot be saved in TOML.
    #[error("a configured path is not valid UTF-8")]
    NonUtf8Path,
    /// TOML serialization failed.
    #[error("cannot serialize configuration")]
    Serialize,
}

/// Inputs for [`resolve`]. Inject everything here; nothing else is read from the
/// process except the files it names.
#[derive(Default)]
pub struct ResolveContext {
    /// Environment snapshot (also provides `HOME` and `XDG_CONFIG_HOME`).
    pub env: BTreeMap<String, String>,
    /// Per-field command-line overrides (highest precedence).
    pub flags: BTreeMap<Field, String>,
    /// Explicit TOML path (`--config`); replaces the XDG default.
    pub config_path: Option<PathBuf>,
    /// Explicit `.env` path (`--env-file`).
    pub env_file: Option<PathBuf>,
    /// Effective user id that must own secret-bearing files.
    pub uid: u32,
}
impl ResolveContext {
    /// Snapshot the process environment and user id for the parsed CLI arguments.
    /// Non-UTF-8 environment entries are ignored.
    ///
    /// # Errors
    /// [`ConfigError::Io`] when the user id cannot be determined.
    pub fn from_process(args: &CliArgs) -> Result<Self, ConfigError> {
        Ok(Self {
            env: std::env::vars_os()
                .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
                .collect(),
            flags: BTreeMap::new(),
            config_path: args.config.clone(),
            env_file: args.env_file.clone(),
            uid: files::current_uid()?,
        })
    }

    /// The XDG default TOML path, if `XDG_CONFIG_HOME` or `HOME` is an absolute path.
    pub fn default_config_path(&self) -> Option<PathBuf> {
        let abs = |name: &str| {
            self.env
                .get(name)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
        };
        let base =
            abs("XDG_CONFIG_HOME").or_else(|| abs("HOME").map(|home| home.join(".config")))?;
        Some(base.join("rust-microsip").join("config.toml"))
    }
}

struct Layers {
    layers: Vec<(Source, BTreeMap<Field, String>)>,
    provenance: BTreeMap<Field, Source>,
}
impl Layers {
    /// First source holding `field`, parsed; an unparsable value is an error, not a fallthrough.
    fn take<T>(
        &mut self,
        field: Field,
        parse: impl Fn(&str) -> Option<T>,
    ) -> Result<Option<T>, ConfigError> {
        for (source, map) in &self.layers {
            if let Some(raw) = map.get(&field) {
                let value = parse(raw).ok_or(ConfigError::Invalid {
                    field,
                    origin: *source,
                })?;
                self.provenance.insert(field, *source);
                return Ok(Some(value));
            }
        }
        Ok(None)
    }
    fn required<T>(
        &mut self,
        field: Field,
        parse: impl Fn(&str) -> Option<T>,
    ) -> Result<T, ConfigError> {
        self.take(field, parse)?.ok_or(ConfigError::Missing(field))
    }
    /// Optional field where an empty value means "unset".
    fn optional<T>(
        &mut self,
        field: Field,
        parse: impl Fn(&str) -> Option<T>,
    ) -> Result<Option<T>, ConfigError> {
        Ok(self
            .take(field, |s| {
                if s.is_empty() {
                    Some(None)
                } else {
                    parse(s).map(Some)
                }
            })?
            .flatten())
    }
}

fn text(s: &str) -> Option<String> {
    Some(s.to_owned())
}
fn device(s: &str) -> Option<String> {
    (s.len() <= 256 && !s.chars().any(char::is_control)).then(|| s.to_owned())
}
fn abs_path(s: &str) -> Option<PathBuf> {
    Some(PathBuf::from(s)).filter(|p| p.is_absolute())
}
fn env_var_name(s: &str) -> Option<String> {
    let mut chars = s.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    (first_ok && s.len() <= 128 && chars.all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .then(|| s.to_owned())
}
fn boolean(s: &str) -> Option<bool> {
    match s {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

/// Resolve the effective configuration from all sources.
///
/// # Errors
/// Any [`ConfigError`]; the caller must not register with SIP on error.
pub fn resolve(ctx: &ResolveContext) -> Result<EffectiveConfig, ConfigError> {
    let env_layer = |map: &BTreeMap<String, String>| -> BTreeMap<Field, String> {
        Field::ALL
            .into_iter()
            .filter_map(|f| Some((f, map.get(&f.env_name())?.clone())))
            .collect()
    };

    let (dotenv, dotenv_meta) = match &ctx.env_file {
        Some(path) => {
            let loaded = files::read_bounded(path, files::MAX_CONFIG_BYTES, false, ctx.uid)?
                .ok_or_else(|| ConfigError::FileMissing(path.clone()))?;
            let map = parse_dotenv(&loaded.text).map_err(|e| ConfigError::DotEnvSyntax {
                path: path.clone(),
                line: e.line,
            })?;
            (map, Some((path.as_path(), loaded.meta)))
        }
        None => (BTreeMap::new(), None),
    };

    let toml_file = match &ctx.config_path {
        Some(path) => Some(
            files::read_bounded(path, files::MAX_CONFIG_BYTES, false, ctx.uid)?
                .map(|loaded| (path.clone(), loaded))
                .ok_or_else(|| ConfigError::FileMissing(path.clone()))?,
        ),
        None => match ctx.default_config_path() {
            Some(path) => files::read_bounded(&path, files::MAX_CONFIG_BYTES, false, ctx.uid)?
                .map(|l| (path, l)),
            None => None,
        },
    };
    let (toml_layer, literal) = match &toml_file {
        Some((path, loaded)) => {
            let (layer, literal) = toml_layer(&loaded.text, path)?;
            if literal.is_some() {
                files::require_private(path, &loaded.meta, ctx.uid)?;
            }
            (layer, literal)
        }
        None => (BTreeMap::new(), None),
    };

    let mut l = Layers {
        layers: vec![
            (Source::Flag, ctx.flags.clone()),
            (Source::Env, env_layer(&ctx.env)),
            (Source::DotEnv, env_layer(&dotenv)),
            (Source::Toml, toml_layer),
        ],
        provenance: BTreeMap::new(),
    };

    let transport = l.take(Field::Transport, |s| match s {
        "udp" => Some(Transport::Udp),
        "tcp" => Some(Transport::Tcp),
        "tls" => Some(Transport::Tls),
        _ => None,
    })?;
    let media = l.take(Field::Media, |s| match s {
        "rtp" => Some(MediaSecurity::PlainRtp),
        "srtp" => Some(MediaSecurity::Srtp),
        _ => None,
    })?;
    let transport = transport.unwrap_or(Transport::Udp);
    let domain = l.required(Field::Domain, text)?;
    let account = AccountSettings {
        display_name: l.take(Field::DisplayName, text)?.unwrap_or_default(),
        username: l.required(Field::Username, text)?,
        auth_username: l.optional(Field::AuthUsername, text)?,
        server: l
            .take(Field::Server, text)?
            .unwrap_or_else(|| domain.clone()),
        domain,
        port: l
            .take(Field::Port, |s| s.parse().ok())?
            .unwrap_or(transport.default_port()),
        transport,
        media: media.unwrap_or(MediaSecurity::PlainRtp),
        expires: l
            .take(Field::Expires, |s| {
                s.parse().ok().filter(|v| (1..=86_400).contains(v))
            })?
            .unwrap_or(DEFAULT_EXPIRES),
    };
    account.validate()?;
    let config = StoredConfig {
        account,
        password_env: l
            .take(Field::PasswordEnv, env_var_name)?
            .unwrap_or_else(|| DEFAULT_PASSWORD_ENV.into()),
        password_file: l.optional(Field::PasswordFile, abs_path)?,
        ca_file: l.optional(Field::CaFile, abs_path)?,
        ui: UiSettings {
            theme: l
                .take(Field::Theme, |s| match s {
                    "system" => Some(Theme::System),
                    "light" => Some(Theme::Light),
                    "dark" => Some(Theme::Dark),
                    _ => None,
                })?
                .unwrap_or_default(),
            dnd: l.take(Field::Dnd, boolean)?.unwrap_or(false),
        },
        audio: AudioSettings {
            input_device: l.optional(Field::InputDevice, device)?,
            output_device: l.optional(Field::OutputDevice, device)?,
        },
    };

    // Password: environment, then a private .env, then a private file, then a TOML literal.
    let non_empty = |v: &&String| !v.is_empty();
    let dotenv_secret = dotenv.get(&config.password_env).filter(non_empty);
    if let (Some(_), Some((path, meta))) = (dotenv_secret, &dotenv_meta) {
        files::require_private(path, meta, ctx.uid)?;
    }
    let (password, password_source) =
        if let Some(v) = ctx.env.get(&config.password_env).filter(non_empty) {
            (
                Some(SecretString::from(v.as_str())),
                Some(PasswordSource::Env),
            )
        } else if let Some(v) = dotenv_secret {
            (
                Some(SecretString::from(v.as_str())),
                Some(PasswordSource::DotEnv),
            )
        } else if let Some(path) = &config.password_file {
            (
                Some(files::read_password_file(path, ctx.uid)?),
                Some(PasswordSource::File),
            )
        } else if literal.is_some() {
            (literal.clone(), Some(PasswordSource::TomlLiteral))
        } else {
            (None, None)
        };

    Ok(EffectiveConfig {
        config,
        password,
        password_source,
        literal_password_warning: literal.is_some(),
        config_file: toml_file.map(|(path, _)| path),
        provenance: l.provenance,
    })
}

/// Printable form of a user-supplied TOML key for error messages.
fn safe_key(section: &str, key: &str) -> String {
    let ok = |s: &str| {
        s.len() <= 64
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    match (section.is_empty(), ok(section) && ok(key)) {
        (_, false) => "<unprintable key>".into(),
        (true, true) => key.into(),
        (false, true) => format!("{section}.{key}"),
    }
}

type TomlLayer = (BTreeMap<Field, String>, Option<SecretString>);

/// Parse schema-1 TOML into per-field strings (types already checked) and an
/// optional literal password.
fn toml_layer(text: &str, path: &Path) -> Result<TomlLayer, ConfigError> {
    let root: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| ConfigError::TomlSyntax {
            path: path.to_owned(),
            line: e.span().map_or(0, |span| {
                text[..span.start.min(text.len())].matches('\n').count() + 1
            }),
        })?;
    if root.get("schema").and_then(toml::Value::as_integer) != Some(SCHEMA_VERSION) {
        return Err(ConfigError::UnsupportedSchema(path.to_owned()));
    }
    let wrong = |key: String| ConfigError::WrongType {
        path: path.to_owned(),
        key,
    };
    let mut layer = BTreeMap::new();
    let mut literal = None;
    for (section, value) in &root {
        if section == "schema" {
            continue;
        }
        if !matches!(section.as_str(), "account" | "ui" | "audio") {
            return Err(ConfigError::UnknownKey {
                path: path.to_owned(),
                key: safe_key("", section),
            });
        }
        let toml::Value::Table(table) = value else {
            return Err(wrong(section.clone()));
        };
        for (key, value) in table {
            if section == "account" && key == "password" {
                let toml::Value::String(secret) = value else {
                    return Err(wrong("account.password".into()));
                };
                literal = Some(SecretString::from(secret.as_str()));
                continue;
            }
            let field = Field::ALL
                .into_iter()
                .find(|f| f.split_key() == (section.as_str(), key.as_str()))
                .ok_or_else(|| ConfigError::UnknownKey {
                    path: path.to_owned(),
                    key: safe_key(section, key),
                })?;
            let raw = match (field.kind(), value) {
                (Kind::Str, toml::Value::String(s)) => s.clone(),
                (Kind::Int, toml::Value::Integer(i)) => i.to_string(),
                (Kind::Bool, toml::Value::Boolean(b)) => b.to_string(),
                _ => return Err(wrong(field.key().into())),
            };
            layer.insert(field, raw);
        }
    }
    Ok((layer, literal))
}

#[cfg(test)]
mod tests;
