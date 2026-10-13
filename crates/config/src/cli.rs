//! Command-line argument parsing for configuration-related flags.

use std::ffi::OsString;
use std::path::PathBuf;

/// Usage text for `--help`.
pub const HELP: &str = "\
Usage: rust-microsip [OPTIONS] [COMMAND]

Options:
  --config PATH     Use this TOML file instead of the XDG default
  --env-file PATH   Load settings from this .env file (never discovered implicitly)
  --doctor [--json] Print a sanitized configuration report and exit
  -h, --help        Print this help and exit
  -V, --version     Print the version and exit

Commands:
  config validate   Validate the effective configuration and exit
  config export     Print the effective configuration as TOML (without secrets)

Passwords are never accepted as arguments; use password_env, a private
password_file, or the interactive prompt.
";

/// What the program should do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CliCommand {
    /// Start the application.
    #[default]
    Run,
    /// Print [`HELP`].
    Help,
    /// Print the version.
    Version,
    /// Print the [`crate::DoctorReport`], as JSON when `json` is set.
    Doctor {
        /// Emit JSON instead of text.
        json: bool,
    },
    /// Validate the configuration.
    ConfigValidate,
    /// Print sanitized TOML.
    ConfigExport,
}

/// Parsed arguments.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CliArgs {
    /// Selected command.
    pub command: CliCommand,
    /// `--config PATH`.
    pub config: Option<PathBuf>,
    /// `--env-file PATH`.
    pub env_file: Option<PathBuf>,
}

/// Argument error. Never includes the offending argument, which may be a secret.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CliError {
    /// Unrecognized argument.
    #[error("unrecognized argument (see --help)")]
    UnknownArgument,
    /// An option is missing its value.
    #[error("option {0} requires a value")]
    MissingValue(&'static str),
    /// An option was given twice.
    #[error("option {0} was given more than once")]
    Duplicate(&'static str),
    /// More than one command, or `--json` without `--doctor`.
    #[error("conflicting commands or options (see --help)")]
    Conflict,
    /// A password-like option was given.
    #[error(
        "passwords are not accepted on the command line; use password_env or a private password_file"
    )]
    PasswordNotAccepted,
}

/// Parse arguments, excluding the program name (`std::env::args_os().skip(1)`).
/// Supports `--opt VALUE` and `--opt=VALUE`.
///
/// # Errors
/// [`CliError`] for unknown, duplicate, conflicting or password arguments.
pub fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<CliArgs, CliError> {
    let mut out = CliArgs::default();
    let mut json = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let text = arg.to_str().ok_or(CliError::UnknownArgument)?;
        let (name, inline) = match text.split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name, Some(OsString::from(value))),
            _ => (text, None),
        };
        if name.starts_with('-') && name.to_ascii_lowercase().contains("pass") {
            return Err(CliError::PasswordNotAccepted);
        }
        match name {
            "--config" | "--env-file" => {
                let flag = if name == "--config" {
                    "--config"
                } else {
                    "--env-file"
                };
                let value = inline.or_else(|| args.next()).filter(|v| !v.is_empty());
                let slot = if name == "--config" {
                    &mut out.config
                } else {
                    &mut out.env_file
                };
                if slot.is_some() {
                    return Err(CliError::Duplicate(flag));
                }
                *slot = Some(PathBuf::from(value.ok_or(CliError::MissingValue(flag))?));
            }
            _ if inline.is_some() => return Err(CliError::UnknownArgument),
            "--doctor" => set(&mut out, CliCommand::Doctor { json: false })?,
            "--json" if !json => json = true,
            "--json" => return Err(CliError::Duplicate("--json")),
            "-h" | "--help" => set(&mut out, CliCommand::Help)?,
            "-V" | "--version" => set(&mut out, CliCommand::Version)?,
            "config" => {
                let sub = args.next().ok_or(CliError::UnknownArgument)?;
                match sub.to_str() {
                    Some("validate") => set(&mut out, CliCommand::ConfigValidate)?,
                    Some("export") => set(&mut out, CliCommand::ConfigExport)?,
                    _ => return Err(CliError::UnknownArgument),
                }
            }
            _ => return Err(CliError::UnknownArgument),
        }
    }
    if json {
        match &mut out.command {
            CliCommand::Doctor { json } => *json = true,
            _ => return Err(CliError::Conflict),
        }
    }
    Ok(out)
}

fn set(out: &mut CliArgs, command: CliCommand) -> Result<(), CliError> {
    if out.command != CliCommand::Run {
        return Err(CliError::Conflict);
    }
    out.command = command;
    Ok(())
}
