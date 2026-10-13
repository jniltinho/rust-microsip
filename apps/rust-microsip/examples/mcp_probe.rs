//! Lab probe: drives `rust-microsip mcp --stdio` with the official MCP SDK
//! client against a running owner. Dials once with a fixed idempotency key,
//! waits up to 60 s for the person's approval through `operation_get`,
//! repeats the same request (which must not dial again) and disconnects.
//! Prints one JSON object per step. Fictional lab data only.

use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn arguments(value: &Value) -> serde_json::Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let binary = std::env::args()
        .nth(1)
        .ok_or("usage: mcp_probe BINARY TARGET KEY")?;
    let target = std::env::args().nth(2).ok_or("target")?;
    let key = std::env::args().nth(3).ok_or("key")?;
    let mut child = tokio::process::Command::new(binary)
        .args(["mcp", "--stdio"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let io = (
        child.stdout.take().ok_or("stdout")?,
        child.stdin.take().ok_or("stdin")?,
    );
    let client = ().serve(io).await?;
    let call = |name: &'static str, args: Value| {
        let params = CallToolRequestParams::new(name).with_arguments(arguments(&args));
        let client = &client;
        async move {
            let result = client.call_tool(params).await?;
            Ok::<Value, Box<dyn std::error::Error>>(
                result.structured_content.unwrap_or(Value::Null),
            )
        }
    };
    let tools = client.list_all_tools().await?.len();
    println!("{}", json!({"step": "tools", "count": tools}));
    let dial = json!({"target": target, "idempotency_key": key});
    let first = call("call_dial", dial.clone()).await?;
    println!("{}", json!({"step": "dial", "status": first["status"]}));
    let operation_id = first["operation_id"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut state = Value::Null;
    while Instant::now() < deadline {
        let got = call("operation_get", json!({"operation_id": operation_id})).await?;
        state = got["data"]["state"].clone();
        if got["data"]["final"] == true {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    println!("{}", json!({"step": "approved", "state": state}));
    tokio::time::sleep(Duration::from_secs(4)).await;
    let replay = call("call_dial", dial).await?;
    println!(
        "{}",
        json!({"step": "replay", "status": replay["status"], "same_operation": replay["operation_id"] == first["operation_id"]})
    );
    let status = call("status_get", json!({})).await?;
    println!(
        "{}",
        json!({"step": "status", "phase": status["data"]["call"]["phase"]})
    );
    client.cancel().await?;
    let _ = child.wait().await;
    println!("{}", json!({"step": "disconnected"}));
    Ok(())
}
