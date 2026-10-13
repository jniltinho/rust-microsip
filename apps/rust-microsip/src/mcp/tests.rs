use super::*;

#[test]
fn every_operation_has_one_strict_tool() {
    let tools = tools();
    assert_eq!(tools.len(), 15);
    for tool in &tools {
        assert_eq!(
            tool.input_schema.get("additionalProperties"),
            Some(&json!(false)),
            "{}",
            tool.name
        );
        let annotations = tool.annotations.as_ref();
        let read_only = annotations.and_then(|a| a.read_only_hint) == Some(true);
        let mutation = !read_only;
        let required = tool.input_schema["required"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert_eq!(
            required.contains(&json!(KEY)),
            mutation,
            "{} key",
            tool.name
        );
    }
    let names: Vec<_> = tools.iter().map(|t| t.name.to_string()).collect();
    for forbidden in [
        "approve",
        "grant",
        "shell",
        "exec",
        "read_file",
        "config_set",
    ] {
        assert!(!names.iter().any(|n| n.contains(forbidden)), "{forbidden}");
    }
}

#[test]
fn tool_calls_map_to_validated_requests() -> Result<(), ControlError> {
    let args = |value: Value| value.as_object().cloned();
    let dial = to_request(
        "call_dial",
        args(json!({"target": "1002", "idempotency_key": "k1"})),
        "r1",
    )?;
    assert_eq!(
        dial.operation,
        Operation::CallDial {
            target: "1002".into()
        }
    );
    assert_eq!(dial.idempotency_key.as_deref(), Some("k1"));
    let status = to_request("status_get", None, "r2")?;
    assert_eq!(status.operation, Operation::StatusGet);
    let contacts = to_request("contacts_search", args(json!({"query": "ana"})), "r3")?;
    assert!(matches!(
        contacts.operation,
        Operation::ContactsSearch { .. }
    ));
    let hold = to_request(
        "call_hold",
        args(json!({"call_id": 3, "expected_version": 2, "held": true, "idempotency_key": "k2"})),
        "r4",
    )?;
    assert_eq!(
        hold.operation,
        Operation::CallHold {
            call_id: 3,
            expected_version: 2,
            held: true
        }
    );
    let invalid =
        |name: &str, value: Value| to_request(name, args(value), "r").map_err(|e| e.code).err();
    assert_eq!(invalid("approve", json!({})), Some(ErrorCode::Validation));
    assert_eq!(
        invalid("call_dial", json!({"target": "1002"})),
        Some(ErrorCode::Validation),
        "key required"
    );
    assert_eq!(
        invalid(
            "call_dial",
            json!({"target": "1002", "idempotency_key": "k", "confirm": true})
        ),
        Some(ErrorCode::Validation),
        "no extra fields"
    );
    assert_eq!(
        invalid("status_get", json!({"verbose": true})),
        Some(ErrorCode::Validation)
    );
    assert_eq!(
        invalid(
            "call_dtmf",
            json!({"call_id": 1, "expected_version": 1, "digits": "9X", "idempotency_key": "k"})
        ),
        Some(ErrorCode::Validation)
    );
    Ok(())
}

#[test]
fn errors_are_flagged_and_pending_is_not() {
    let pending = to_result(&Response::pending("r", Status::ApprovalRequired, "op-1-ab"));
    assert_eq!(pending.is_error, Some(false));
    let failed = to_result(&Response::error(
        "r",
        &ControlError::new(ErrorCode::EngineUnavailable, "x"),
    ));
    assert_eq!(failed.is_error, Some(true));
    assert_eq!(
        failed
            .structured_content
            .as_ref()
            .map(|v| v["error"]["code"].clone()),
        Some(json!("engine_unavailable"))
    );
}

#[test]
fn oversized_lines_fail_instead_of_buffering() -> std::io::Result<()> {
    use tokio::io::AsyncReadExt;
    let runtime = tokio::runtime::Builder::new_current_thread().build()?;
    runtime.block_on(async {
        let mut ok = BoundedLines::new(&b"{\"a\":1}\n{\"b\":2}\n"[..]);
        let mut text = String::new();
        ok.read_to_string(&mut text).await?;
        assert_eq!(text.lines().count(), 2);
        let huge = vec![b'x'; MAX_LINE + 1];
        let mut bad = BoundedLines::new(&huge[..]);
        let error = bad.read_to_end(&mut Vec::new()).await.err();
        assert_eq!(
            error.map(|e| e.kind()),
            Some(std::io::ErrorKind::InvalidData)
        );
        Ok(())
    })
}
