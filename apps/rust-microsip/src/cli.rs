//! Control CLI (AG-02): parses `rust-microsip <command>` into a control
//! [`Request`] and renders the owner's [`Response`]. It never opens the
//! database, registers SIP or starts an owner; without one it reports
//! `engine_unavailable` (exit 4).
//!
//! Mutations carry an idempotency key: `--key KEY` to retry safely, otherwise
//! a random one that is printed so the operator can query or retry with it.
//! DTMF digits and dial/transfer targets can come from standard input (`-`)
//! to keep them out of the process list. Keys are stored in the local ledger
//! for 24 hours: never put personal data into a key.

use rust_microsip_control::{
    CallRef, ErrorCode, HistoryFilter, Operation, Request, Response, SCHEMA_VERSION, Status,
};
use std::fmt::Write as _;

/// Usage text for the control commands.
pub const HELP: &str = "\
Control commands (talk to the running rust-microsip; never start it):
  rust-microsip status [--json]
  rust-microsip operation get OPERATION_ID [--json]
  rust-microsip call dial TARGET|- [--key KEY] [--json]
  rust-microsip call answer|reject|hangup CALL_ID VERSION [--key KEY] [--json]
  rust-microsip call dtmf CALL_ID VERSION DIGITS|- [--key KEY] [--json]
  rust-microsip call mute|unmute|hold|resume CALL_ID VERSION [--key KEY] [--json]
  rust-microsip call transfer CALL_ID VERSION TARGET|- [--key KEY] [--json]
  rust-microsip dnd on|off [--key KEY] [--json]
  rust-microsip devices list | contacts search [TEXT] | history list [--filter F]
      | config show   (need a read grant)   [--limit N] [--cursor C] [--json]
  rust-microsip mcp --stdio   (MCP server for agents; same rules as the CLI)
CALL_ID and VERSION come from `status`. Mutations wait for approval in the
window; query the printed operation id. Exit: 0 done/accepted, 2 invalid,
3 not authorized or awaiting approval, 4 unavailable, 5 timeout, 1 internal.
";

/// A parsed control command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlCommand {
    /// What to ask.
    pub operation: Operation,
    /// `--key`, if given.
    pub key: Option<String>,
    /// `--json` output.
    pub json: bool,
    /// Read DTMF digits or the dial/transfer target from standard input
    /// (`-`), keeping them out of the process list.
    pub from_stdin: bool,
}

/// Whether `first` starts a control command.
#[must_use]
pub fn is_control(first: &str) -> bool {
    matches!(
        first,
        "status" | "operation" | "call" | "dnd" | "devices" | "contacts" | "history"
    )
}

fn number<T: std::str::FromStr>(value: Option<&String>, name: &str) -> Result<T, String> {
    value
        .ok_or_else(|| format!("missing {name}"))?
        .parse()
        .map_err(|_| format!("{name} must be a number"))
}

/// Parse arguments after the program name. `config show` is recognised by
/// the caller and passed here as `["config", "show", ...]`.
///
/// # Errors
/// A message for the operator (exit 2).
pub fn parse(args: &[String]) -> Result<ControlCommand, String> {
    let mut words = Vec::new();
    let (mut key, mut json, mut limit, mut cursor, mut filter) = (None, false, None, None, None);
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = |name: &str| {
            iter.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--json" => json = true,
            "--key" => key = Some(value("--key")?),
            "--limit" => {
                limit = Some(
                    value("--limit")?
                        .parse()
                        .map_err(|_| "--limit must be a number")?,
                )
            }
            "--cursor" => cursor = Some(value("--cursor")?),
            "--filter" => {
                filter = Some(match value("--filter")?.as_str() {
                    "all" => HistoryFilter::All,
                    "incoming" => HistoryFilter::Incoming,
                    "outgoing" => HistoryFilter::Outgoing,
                    "missed" => HistoryFilter::Missed,
                    _ => return Err("--filter must be all, incoming, outgoing or missed".into()),
                });
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            _ => words.push(arg.clone()),
        }
    }
    let call = |words: &[String]| -> Result<CallRef, String> {
        Ok(CallRef {
            call_id: number(words.get(2), "CALL_ID")?,
            expected_version: number(words.get(3), "VERSION")?,
        })
    };
    let arity = |n: usize| {
        if words.len() == n {
            Ok(())
        } else {
            Err("wrong number of arguments; see --help".to_owned())
        }
    };
    let words_str: Vec<&str> = words.iter().map(String::as_str).collect();
    let mut from_stdin = false;
    let operation = match words_str.as_slice() {
        ["status"] => Operation::StatusGet,
        ["operation", "get", id] => Operation::OperationGet {
            operation_id: (*id).to_owned(),
        },
        ["devices", "list"] => Operation::DevicesList,
        ["config", "show"] => Operation::ConfigShow,
        ["contacts", "search", rest @ ..] if rest.len() <= 1 => Operation::ContactsSearch {
            query: rest.first().map_or_else(String::new, |q| (*q).to_owned()),
            cursor,
            limit,
        },
        ["history", "list"] => Operation::HistoryList {
            filter,
            cursor,
            limit,
        },
        ["call", "dial", target] => {
            from_stdin = *target == "-";
            Operation::CallDial {
                target: if from_stdin {
                    String::new()
                } else {
                    (*target).to_owned()
                },
            }
        }
        ["call", "answer", ..] => {
            arity(4)?;
            Operation::CallAnswer(call(&words)?)
        }
        ["call", "reject", ..] => {
            arity(4)?;
            Operation::CallReject(call(&words)?)
        }
        ["call", "hangup", ..] => {
            arity(4)?;
            Operation::CallHangup(call(&words)?)
        }
        ["call", action @ ("mute" | "unmute" | "hold" | "resume"), ..] => {
            arity(4)?;
            let CallRef {
                call_id,
                expected_version,
            } = call(&words)?;
            match *action {
                "mute" | "unmute" => Operation::CallMute {
                    call_id,
                    expected_version,
                    muted: *action == "mute",
                },
                _ => Operation::CallHold {
                    call_id,
                    expected_version,
                    held: *action == "hold",
                },
            }
        }
        ["call", "dtmf", _, _, digits] => {
            let CallRef {
                call_id,
                expected_version,
            } = call(&words)?;
            from_stdin = *digits == "-";
            Operation::CallDtmf {
                call_id,
                expected_version,
                digits: if from_stdin {
                    String::new()
                } else {
                    (*digits).to_owned()
                },
            }
        }
        ["call", "transfer", _, _, target] => {
            let CallRef {
                call_id,
                expected_version,
            } = call(&words)?;
            from_stdin = *target == "-";
            Operation::CallTransfer {
                call_id,
                expected_version,
                target: if from_stdin {
                    String::new()
                } else {
                    (*target).to_owned()
                },
            }
        }
        ["dnd", state @ ("on" | "off")] => Operation::DndSet {
            enabled: *state == "on",
        },
        _ => return Err("unknown control command; see --help".into()),
    };
    Ok(ControlCommand {
        operation,
        key,
        json,
        from_stdin,
    })
}

/// Build the request; `key` is used only for mutations.
#[must_use]
pub fn request(command: &ControlCommand, request_id: &str, key: &str) -> Request {
    Request {
        schema_version: SCHEMA_VERSION,
        request_id: request_id.to_owned(),
        idempotency_key: command.operation.is_mutation().then(|| key.to_owned()),
        operation: command.operation.clone(),
    }
}

/// Human rendering of a response.
#[must_use]
pub fn render(response: &Response, key: Option<&str>) -> String {
    let mut out = String::new();
    match response.status {
        Status::Ok => out.push_str("ok\n"),
        Status::Accepted => out.push_str("accepted\n"),
        Status::ApprovalRequired => {
            out.push_str("waiting for approval in the rust-microsip window\n");
        }
        Status::Error => {
            if let Some(error) = &response.error {
                let _ = writeln!(out, "error: {} ({})", error.message, code_name(error.code));
            }
        }
    }
    if let Some(id) = &response.operation_id {
        let _ = writeln!(out, "operation: {id}");
    }
    if let Some(key) = key {
        let _ = writeln!(out, "idempotency key: {key}");
    }
    if let Some(data) = &response.data {
        let text = serde_json::to_string_pretty(data).unwrap_or_default();
        out.push_str(&text);
        out.push('\n');
    }
    out
}

/// The JSON name of an error code.
#[must_use]
pub fn code_name(code: ErrorCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &str) -> Vec<String> {
        text.split(' ').map(str::to_owned).collect()
    }

    #[test]
    fn commands_map_to_operations() -> Result<(), String> {
        assert_eq!(
            parse(&args("status --json"))?.operation,
            Operation::StatusGet
        );
        assert!(parse(&args("status --json"))?.json);
        let dial = parse(&args("call dial 1002 --key retry-1"))?;
        assert_eq!(
            dial.operation,
            Operation::CallDial {
                target: "1002".into()
            }
        );
        assert_eq!(dial.key.as_deref(), Some("retry-1"));
        assert_eq!(
            parse(&args("call hold 4 9"))?.operation,
            Operation::CallHold {
                call_id: 4,
                expected_version: 9,
                held: true
            }
        );
        assert_eq!(
            parse(&args("call unmute 4 9"))?.operation,
            Operation::CallMute {
                call_id: 4,
                expected_version: 9,
                muted: false
            }
        );
        let dtmf = parse(&args("call dtmf 4 9 -"))?;
        assert!(dtmf.from_stdin);
        assert!(parse(&args("call dial -"))?.from_stdin);
        assert!(parse(&args("call transfer 4 9 -"))?.from_stdin);
        assert!(!parse(&args("call dial 1002"))?.from_stdin);
        assert_eq!(
            parse(&args("history list --filter missed --limit 20"))?.operation,
            Operation::HistoryList {
                filter: Some(HistoryFilter::Missed),
                cursor: None,
                limit: Some(20)
            }
        );
        assert_eq!(
            parse(&args("dnd off"))?.operation,
            Operation::DndSet { enabled: false }
        );
        assert!(is_control("call") && !is_control("doctor"));
        Ok(())
    }

    #[test]
    fn bad_input_is_refused_and_there_is_no_approve_command() {
        for bad in [
            "call answer 4",
            "call answer x 1",
            "call dial",
            "approve op-1",
            "grant call_dial",
            "call dial 1002 --yes",
            "call dial 1002 --confirm",
            "history list --filter recent",
        ] {
            assert!(parse(&args(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn keys_go_only_on_mutations_and_rendering_is_plain() -> Result<(), String> {
        let status = parse(&args("status"))?;
        assert_eq!(request(&status, "r", "k").idempotency_key, None);
        let dial = parse(&args("call dial 1002"))?;
        assert_eq!(
            request(&dial, "r", "k").idempotency_key.as_deref(),
            Some("k")
        );
        let pending = Response::pending("r", Status::ApprovalRequired, "op-1-ab");
        let text = render(&pending, Some("k"));
        assert!(
            text.contains("waiting for approval") && text.contains("op-1-ab") && text.contains("k")
        );
        let error = Response::error(
            "r",
            &rust_microsip_control::ControlError::new(ErrorCode::EngineUnavailable, "no owner"),
        );
        assert!(render(&error, None).contains("(engine_unavailable)"));
        Ok(())
    }
}
