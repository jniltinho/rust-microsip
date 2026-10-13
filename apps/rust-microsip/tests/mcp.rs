//! The stdio MCP server against a real client of the official SDK (AG-03),
//! with no owner running: fictional data, no network or devices.

use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use serde_json::json;
use std::process::Stdio;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn runtime_dir(tag: &str) -> std::io::Result<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("rust-microsip-mcp-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}

#[tokio::test(flavor = "current_thread")]
async fn official_client_lists_tools_and_gets_honest_errors() -> TestResult {
    let dir = runtime_dir("client")?;
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_rust-microsip"))
        .args(["mcp", "--stdio"])
        .env_clear()
        .env("XDG_RUNTIME_DIR", &dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let (stdout, stdin) = (
        child.stdout.take().ok_or("stdout")?,
        child.stdin.take().ok_or("stdin")?,
    );
    let client = ().serve((stdout, stdin)).await?;
    let info = client.peer_info().ok_or("no server info")?;
    let server = info.server_info.as_ref().ok_or("no server info")?;
    assert_eq!(server.name, "rust-microsip");
    let tools = client.list_all_tools().await?;
    assert_eq!(tools.len(), 15);
    // No owner: an honest, structured error, and nothing was started.
    let status = client
        .call_tool(CallToolRequestParams::new("status_get"))
        .await?;
    assert_eq!(status.is_error, Some(true));
    let body = status.structured_content.ok_or("no structured content")?;
    assert_eq!(body["error"]["code"], "engine_unavailable");
    assert!(!dir.join("rust-microsip").join("instance.lock").exists());
    // Unknown tools, extra fields and missing keys are refused before any I/O.
    for (name, arguments) in [
        ("approve", json!({})),
        ("call_dial", json!({"target": "1002"})),
        (
            "call_dial",
            json!({"target": "1002", "idempotency_key": "k", "confirm": true}),
        ),
    ] {
        let params = CallToolRequestParams::new(name)
            .with_arguments(arguments.as_object().cloned().unwrap_or_default());
        let result = client.call_tool(params).await?;
        assert_eq!(result.is_error, Some(true), "{name}");
        let body = result.structured_content.ok_or("no structured content")?;
        assert_eq!(body["error"]["code"], "validation", "{name}");
    }
    client.cancel().await?;
    let _ = child.wait().await;
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn stdout_carries_only_json_rpc_even_after_garbage() -> TestResult {
    use std::io::{BufRead, BufReader, Write};
    let dir = runtime_dir("raw")?;
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_rust-microsip"))
        .args(["mcp", "--stdio"])
        .env_clear()
        .env("XDG_RUNTIME_DIR", &dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdin = child.stdin.take().ok_or("stdin")?;
    for line in [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"raw-probe","version":"1"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"status_get","arguments":{}}}"#,
        "this is not json",
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#,
    ] {
        writeln!(stdin, "{line}")?;
    }
    stdin.flush()?;
    let reader = BufReader::new(child.stdout.take().ok_or("stdout")?);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in reader.lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut lines = Vec::new();
    // Responses may arrive in any order (tool calls run concurrently).
    while let Ok(line) = rx.recv_timeout(std::time::Duration::from_secs(5)) {
        lines.push(line);
        let seen = |id: &str| lines.iter().any(|l| l.contains(&format!("\"id\":{id}")));
        if seen("2") && seen("3") {
            break;
        }
    }
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    // The session survived the garbage line: the later request was answered.
    assert!(lines.iter().any(|l| l.contains("\"id\":3")), "{lines:?}");
    for line in &lines {
        let value: serde_json::Value = serde_json::from_str(line)?;
        assert_eq!(value["jsonrpc"], "2.0", "{line}");
    }
    assert!(
        lines.iter().any(|l| l.contains("engine_unavailable")),
        "{lines:?}"
    );
    std::fs::remove_dir_all(dir)?;
    Ok(())
}
