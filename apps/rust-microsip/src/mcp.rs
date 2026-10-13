//! stdio MCP server (AG-03) built on the official Rust SDK (`rmcp`).
//!
//! Every tool maps to exactly one control [`Operation`] and is executed by
//! the running owner through the same authenticated socket as the CLI, so
//! the owner's policies apply unchanged: mutations wait for a person in the
//! window, personal reads need grants, nothing here can approve anything.
//! The server never starts an owner, never opens the database and exposes no
//! shell, file or network access. stdout carries only MCP messages; nothing
//! else is written there. Input lines are bounded to [`MAX_LINE`] bytes and
//! calls to [`RATE_PER_SECOND`] per second.

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, Implementation, JsonObject,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    ToolAnnotations,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt};
use rust_microsip_control::{
    ControlError, ErrorCode, Operation, Request, Response, SCHEMA_VERSION, Status,
};
use rust_microsip_platform::{ControlClient, ControlSocketError};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, ReadBuf};

/// Longest accepted input line (one JSON-RPC message).
pub const MAX_LINE: usize = 1 << 20;
/// Tool calls accepted per second; more get a `rate_limited` result.
pub const RATE_PER_SECOND: u32 = 10;

const KEY: &str = "idempotency_key";

fn schema(properties: &Value, required: &[&str]) -> JsonObject {
    let object = json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    });
    object.as_object().cloned().unwrap_or_default()
}

fn key_property() -> Value {
    json!({
        "type": "string", "pattern": "^[A-Za-z0-9._:-]{1,128}$",
        "description": "Reuse the same key to retry safely; a new key is a new request."
    })
}

fn call_properties(extra: &Value) -> Value {
    let mut properties = json!({
        "call_id": {"type": "integer", "minimum": 0, "description": "From status_get."},
        "expected_version": {"type": "integer", "minimum": 0, "description": "From status_get."},
        KEY: key_property(),
    });
    if let (Some(base), Some(more)) = (properties.as_object_mut(), extra.as_object()) {
        base.extend(more.clone());
    }
    properties
}

fn annotations(read_only: bool, destructive: bool, open_world: bool) -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(read_only)
        .destructive(destructive)
        .idempotent(read_only)
        .open_world(open_world)
}

/// The advertised tools, one per control operation.
#[must_use]
pub fn tools() -> Vec<Tool> {
    let target = json!({"type": "string", "minLength": 1, "maxLength": 64,
        "description": "Number or SIP user."});
    let page = json!({
        "cursor": {"type": "string", "pattern": "^[A-Za-z0-9._:-]{1,64}$"},
        "limit": {"type": "integer", "minimum": 1, "maximum": 100},
    });
    let mut contacts = page.clone();
    if let Some(map) = contacts.as_object_mut() {
        map.insert("query".into(), json!({"type": "string", "maxLength": 64}));
    }
    let mut history = page;
    if let Some(map) = history.as_object_mut() {
        map.insert(
            "filter".into(),
            json!({"type": "string", "enum": ["all", "incoming", "outgoing", "missed"]}),
        );
    }
    let read = annotations(true, false, false);
    let pending = "Waits for a person to approve it in the rust-microsip window; \
        query operation_get with the returned operation_id.";
    let call_ref = || {
        schema(
            &call_properties(&json!({})),
            &["call_id", "expected_version", KEY],
        )
    };
    let tool =
        |name: &'static str, description: String, input: JsonObject, notes: ToolAnnotations| {
            Tool::new(name, description, input).with_annotations(notes)
        };
    vec![
        tool(
            "status_get",
            "Registration, DND and the current call (no names or numbers).".into(),
            schema(&json!({}), &[]),
            read.clone(),
        ),
        tool(
            "operation_get",
            "State and sanitized outcome of an earlier request.".into(),
            schema(
                &json!({"operation_id": {"type": "string", "pattern": "^[A-Za-z0-9._:-]{1,64}$"}}),
                &["operation_id"],
            ),
            read.clone(),
        ),
        tool(
            "devices_list",
            "Audio devices (needs a read grant).".into(),
            schema(&json!({}), &[]),
            read.clone(),
        ),
        tool(
            "contacts_search",
            "Search contacts (needs a read grant).".into(),
            schema(&contacts, &["query"]),
            read.clone(),
        ),
        tool(
            "history_list",
            "Call history (needs a read grant).".into(),
            schema(&history, &[]),
            read.clone(),
        ),
        tool(
            "config_show",
            "Sanitized configuration (needs a read grant).".into(),
            schema(&json!({}), &[]),
            read,
        ),
        tool(
            "call_dial",
            format!("Place a call. {pending}"),
            schema(
                &json!({"target": target, KEY: key_property()}),
                &["target", KEY],
            ),
            annotations(false, false, true),
        ),
        tool(
            "call_answer",
            format!("Answer the ringing call; starts the microphone. {pending}"),
            call_ref(),
            annotations(false, false, true),
        ),
        tool(
            "call_reject",
            format!("Reject the ringing call. {pending}"),
            call_ref(),
            annotations(false, true, true),
        ),
        tool(
            "call_hangup",
            format!("End the call. {pending}"),
            call_ref(),
            annotations(false, true, true),
        ),
        tool(
            "call_dtmf",
            format!("Send DTMF digits. {pending}"),
            schema(
                &call_properties(
                    &json!({"digits": {"type": "string", "pattern": "^[0-9*#A-D]{1,32}$"}}),
                ),
                &["call_id", "expected_version", "digits", KEY],
            ),
            annotations(false, false, true),
        ),
        tool(
            "call_mute",
            format!("Mute or unmute the microphone. {pending}"),
            schema(
                &call_properties(&json!({"muted": {"type": "boolean"}})),
                &["call_id", "expected_version", "muted", KEY],
            ),
            annotations(false, false, false),
        ),
        tool(
            "call_hold",
            format!("Hold or resume the call. {pending}"),
            schema(
                &call_properties(&json!({"held": {"type": "boolean"}})),
                &["call_id", "expected_version", "held", KEY],
            ),
            annotations(false, false, true),
        ),
        tool(
            "call_transfer",
            format!("Blind-transfer the call. {pending}"),
            schema(
                &call_properties(&json!({"target": target})),
                &["call_id", "expected_version", "target", KEY],
            ),
            annotations(false, true, true),
        ),
        tool(
            "dnd_set",
            format!("Turn do-not-disturb on or off. {pending}"),
            schema(
                &json!({"enabled": {"type": "boolean"}, KEY: key_property()}),
                &["enabled", KEY],
            ),
            annotations(false, false, false),
        ),
    ]
}

/// Map a tool call to a control request. Unknown tools and arguments fail.
///
/// # Errors
/// [`ErrorCode::Validation`] without echoing the input.
pub fn to_request(
    name: &str,
    arguments: Option<JsonObject>,
    request_id: &str,
) -> Result<Request, ControlError> {
    let invalid = |message: &str| ControlError::new(ErrorCode::Validation, message);
    if !tools().iter().any(|tool| tool.name == name) {
        return Err(invalid("unknown tool"));
    }
    let mut arguments = arguments.unwrap_or_default();
    let key = match arguments.remove(KEY) {
        None => None,
        Some(Value::String(key)) => Some(key),
        Some(_) => return Err(invalid("idempotency_key must be a string")),
    };
    let operation: Operation =
        if arguments.is_empty() && !matches!(name, "contacts_search" | "history_list") {
            serde_json::from_value(json!({ "op": name }))
        } else {
            serde_json::from_value(json!({ "op": name, "args": arguments }))
        }
        .map_err(|_| invalid("arguments do not match the tool schema"))?;
    let request = Request {
        schema_version: SCHEMA_VERSION,
        request_id: request_id.to_owned(),
        idempotency_key: key,
        operation,
        capability: None,
    };
    request.validate()?;
    Ok(request)
}

/// Tool result for an owner response: structured, flagged as an error when
/// the request failed. `approval_required` is not an error.
#[must_use]
pub fn to_result(response: &Response) -> CallToolResult {
    let value = serde_json::to_value(response).unwrap_or(Value::Null);
    if response.status == Status::Error {
        CallToolResult::structured_error(value)
    } else {
        CallToolResult::structured(value)
    }
}

/// The MCP server state.
pub struct Server {
    runtime_dir: Option<PathBuf>,
    capability: Option<String>,
    window: Mutex<(Instant, u32)>,
    next: Mutex<u64>,
}
impl Server {
    /// Serve the owner of `runtime_dir` (`None`: no runtime directory),
    /// presenting `capability` (from `--grant-file`) with every request.
    #[must_use]
    pub fn new(runtime_dir: Option<PathBuf>, capability: Option<String>) -> Self {
        Self {
            runtime_dir,
            capability,
            window: Mutex::new((Instant::now(), 0)),
            next: Mutex::new(0),
        }
    }

    fn allow(&self) -> bool {
        let Ok(mut window) = self.window.lock() else {
            return false;
        };
        if window.0.elapsed() >= Duration::from_secs(1) {
            *window = (Instant::now(), 0);
        }
        window.1 += 1;
        window.1 <= RATE_PER_SECOND
    }

    fn request_id(&self) -> String {
        let next = self.next.lock().map_or(0, |mut n| {
            *n += 1;
            *n
        });
        format!("mcp-{}-{next}", std::process::id())
    }

    fn execute(&self, request: &Request) -> Response {
        let id = &request.request_id;
        let Some(dir) = &self.runtime_dir else {
            return Response::error(
                id,
                &ControlError::new(ErrorCode::EngineUnavailable, "no runtime directory"),
            );
        };
        match ControlClient::connect(dir) {
            Ok(mut client) => client.call(request).unwrap_or_else(|error| {
                let code = match error.kind() {
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
                        ErrorCode::Timeout
                    }
                    _ => ErrorCode::EngineUnavailable,
                };
                Response::error(
                    id,
                    &ControlError::new(
                        code,
                        "the running instance did not answer; query the operation before retrying",
                    ),
                )
            }),
            Err(ControlSocketError::Unavailable) => Response::error(
                id,
                &ControlError::new(
                    ErrorCode::EngineUnavailable,
                    "rust-microsip is not running; nothing was started",
                ),
            ),
            Err(_) => Response::error(
                id,
                &ControlError::new(
                    ErrorCode::EngineUnavailable,
                    "cannot reach the running instance",
                ),
            ),
        }
    }

    fn call(&self, params: CallToolRequestParams) -> CallToolResult {
        let id = self.request_id();
        if !self.allow() {
            return to_result(&Response::error(
                &id,
                &ControlError::new(ErrorCode::RateLimited, "too many tool calls"),
            ));
        }
        match to_request(&params.name, params.arguments, &id) {
            Ok(mut request) => {
                request.capability.clone_from(&self.capability);
                to_result(&self.execute(&request))
            }
            Err(error) => to_result(&Response::error(&id, &error)),
        }
    }
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "rust-microsip",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Controls the running rust-microsip softphone. Changes wait for a person to \
                 approve them in the window; read status_get first for call ids and versions.",
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(tools()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        // Socket I/O blocks for at most the owner's reply deadline.
        Ok(tokio::task::block_in_place(|| self.call(request)).into())
    }
}

/// `AsyncRead` that fails a line longer than [`MAX_LINE`] instead of
/// buffering it.
pub struct BoundedLines<R> {
    inner: R,
    current: usize,
}
impl<R> BoundedLines<R> {
    /// Wrap `inner`.
    pub const fn new(inner: R) -> Self {
        Self { inner, current: 0 }
    }
}
impl<R: AsyncRead + Unpin> AsyncRead for BoundedLines<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let polled = Pin::new(&mut self.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &polled {
            let mut current = self.current;
            let overflow = buf.filled()[before..].iter().any(|byte| {
                current = if *byte == b'\n' { 0 } else { current + 1 };
                current > MAX_LINE
            });
            if overflow {
                // Hand nothing of the oversized line to the reader.
                buf.set_filled(before);
                return Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "message too large",
                )));
            }
            self.current = current;
        }
        polled
    }
}

/// Serve MCP on stdin/stdout until the client disconnects.
///
/// # Errors
/// A user-safe message when the transport fails to start.
pub fn run_stdio(runtime_dir: Option<PathBuf>, capability: Option<String>) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|_| "cannot start the MCP runtime".to_owned())?;
    runtime.block_on(async move {
        let transport = (BoundedLines::new(tokio::io::stdin()), tokio::io::stdout());
        let service = Server::new(runtime_dir, capability)
            .serve(transport)
            .await
            .map_err(|_| "the MCP client did not initialize".to_owned())?;
        service
            .waiting()
            .await
            .map(|_| ())
            .map_err(|_| "the MCP session failed".to_owned())
    })
}

#[cfg(test)]
mod tests;
