//! Minimal entry point for build/diagnostic verification, without telephony I/O.
//!
//! Does not read configuration, databases, SIP environment, devices, or XDG paths.
//! Real adapters will be connected during G0, before the complete UI.

use std::ffi::OsString;
use std::fmt;
use std::io::{self, Write};
use std::process::ExitCode;

const HELP: &str = "rust-microsip - experimental scaffold, G0 pending\n\
Usage: rust-microsip [--help | --version | --doctor [--json]]\n\n\
  --help, -h     Show this help.\n\
  --version, -V  Show the development version.\n\
  --doctor       Report compiled scaffold status only.\n\
  --json         JSON v1 format, only with --doctor.\n\n\
Without options, the desktop is not implemented yet.\n\
SIP, audio, persistence, control CLI, and MCP are not available yet.\n";

const DOCTOR_TEXT: &str = "Experimental scaffold; G0 has not passed.\n\
SIP, media, desktop, persistence, and IPC: not implemented yet.\n\
This diagnostic does not probe the host, settings, databases, network, or devices.\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Desktop,
    Help,
    Version,
    Doctor { json: bool },
}

#[derive(Debug)]
struct InvalidArguments;

impl fmt::Display for InvalidArguments {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Arguments may contain passwords: never echo input in errors.
        formatter.write_str("invalid or unsupported arguments; see --help")
    }
}

impl std::error::Error for InvalidArguments {}

fn parse_command(args: &[OsString]) -> Result<Command, InvalidArguments> {
    let args: Vec<&str> = args
        .iter()
        .map(|arg| arg.to_str().ok_or(InvalidArguments))
        .collect::<Result<_, _>>()?;
    match args.as_slice() {
        [] => Ok(Command::Desktop),
        ["--help" | "-h"] => Ok(Command::Help),
        ["--version" | "-V"] => Ok(Command::Version),
        ["--doctor"] => Ok(Command::Doctor { json: false }),
        ["--doctor", "--json"] | ["--json", "--doctor"] => Ok(Command::Doctor { json: true }),
        _ => Err(InvalidArguments),
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let command = match parse_command(&args) {
        Ok(command) => command,
        Err(error) => {
            if writeln!(io::stderr().lock(), "{error}").is_err() {
                return ExitCode::FAILURE;
            }
            return ExitCode::from(2);
        }
    };
    let output = match command {
        Command::Desktop => {
            let _ = writeln!(
                io::stderr().lock(),
                "Desktop not implemented yet; G0 pending. See --help or --doctor."
            );
            return ExitCode::FAILURE;
        }
        Command::Help => HELP,
        Command::Version => concat!("rust-microsip ", env!("CARGO_PKG_VERSION"), "\n"),
        Command::Doctor { json: false } => DOCTOR_TEXT,
        Command::Doctor { json: true } => rust_microsip::scaffold_diagnostic_json(),
    };
    match io::stdout().lock().write_all(output.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}
