use super::*;
use std::io::Cursor;

fn request(json: &str) -> Result<Request, serde_json::Error> {
    serde_json::from_str(json)
}

#[test]
fn requests_round_trip_with_the_documented_shape() -> Result<(), serde_json::Error> {
    let parsed = request(
        r#"{"schema_version":1,"request_id":"r1","idempotency_key":"k-1",
            "operation":{"op":"call_dtmf","args":{"call_id":4,"expected_version":7,"digits":"12#"}}}"#,
    )?;
    assert_eq!(
        parsed.operation,
        Operation::CallDtmf {
            call_id: 4,
            expected_version: 7,
            digits: "12#".into()
        }
    );
    assert_eq!(parsed.operation.call_ref().map(|c| c.call_id), Some(4));
    assert!(parsed.validate().is_ok());
    let status = request(
        r#"{"schema_version":1,"request_id":"r2","idempotency_key":null,"operation":{"op":"status_get"}}"#,
    )?;
    assert_eq!(status.operation, Operation::StatusGet);
    assert!(!status.operation.is_mutation() && status.validate().is_ok());
    let back: Request = serde_json::from_str(&serde_json::to_string(&parsed)?)?;
    assert_eq!(back, parsed);
    Ok(())
}

#[test]
fn unknown_fields_and_operations_are_rejected() {
    for bad in [
        r#"{"schema_version":1,"request_id":"r","idempotency_key":null,"operation":{"op":"status_get"},"extra":1}"#,
        r#"{"schema_version":1,"request_id":"r","idempotency_key":"k","operation":{"op":"call_dial","args":{"target":"1002","confirm":true}}}"#,
        r#"{"schema_version":1,"request_id":"r","idempotency_key":"k","operation":{"op":"call_hangup","args":{"call_id":1,"expected_version":1,"force":true}}}"#,
        r#"{"schema_version":1,"request_id":"r","idempotency_key":null,"operation":{"op":"shell","args":{"cmd":"id"}}}"#,
        r#"{"schema_version":1,"request_id":"r","idempotency_key":null,"operation":{"op":"approve","args":{}}}"#,
    ] {
        assert!(request(bad).is_err(), "{bad}");
    }
}

#[test]
fn validation_bounds_inputs_without_echoing_them() {
    let make = |key: Option<&str>, operation: Operation| Request {
        schema_version: 1,
        request_id: "r1".into(),
        idempotency_key: key.map(Into::into),
        operation,
    };
    let dial = |target: &str| Operation::CallDial {
        target: target.into(),
    };
    assert_eq!(
        make(None, dial("1002")).validate().map_err(|e| e.code),
        Err(ErrorCode::Validation),
        "mutations need a key"
    );
    let bad = make(Some("k"), dial("not a number; rm -rf"));
    let error = bad.validate().err();
    assert!(
        error
            .as_ref()
            .is_some_and(|e| !e.message.contains("rm -rf"))
    );
    for (operation, ok) in [
        (dial("1002"), true),
        (
            Operation::CallDtmf {
                call_id: 1,
                expected_version: 1,
                digits: "1".repeat(33),
            },
            false,
        ),
        (
            Operation::CallDtmf {
                call_id: 1,
                expected_version: 1,
                digits: "12E".into(),
            },
            false,
        ),
        (
            Operation::ContactsSearch {
                query: "a".repeat(65),
                cursor: None,
                limit: None,
            },
            false,
        ),
        (
            Operation::HistoryList {
                filter: None,
                cursor: None,
                limit: Some(101),
            },
            false,
        ),
        (
            Operation::HistoryList {
                filter: Some(HistoryFilter::Missed),
                cursor: Some("c:20".into()),
                limit: Some(100),
            },
            true,
        ),
        (
            Operation::OperationGet {
                operation_id: "../x".into(),
            },
            false,
        ),
    ] {
        assert_eq!(
            make(Some("k"), operation.clone()).validate().is_ok(),
            ok,
            "{operation:?}"
        );
    }
    let mut wrong_version = make(None, Operation::StatusGet);
    wrong_version.schema_version = 2;
    assert!(wrong_version.validate().is_err());
    let mut long_id = make(None, Operation::StatusGet);
    long_id.request_id = "x".repeat(65);
    assert!(long_id.validate().is_err());
}

#[test]
fn exit_codes_follow_the_cli_contract() {
    let ok = Response::ok("r", serde_json::json!({}));
    assert_eq!(ok.exit_code(), 0);
    assert_eq!(
        Response::pending("r", Status::Accepted, "op-1").exit_code(),
        0
    );
    assert_eq!(
        Response::pending("r", Status::ApprovalRequired, "op-1").exit_code(),
        3
    );
    for (code, exit) in [
        (ErrorCode::Validation, 2),
        (ErrorCode::GrantRequired, 3),
        (ErrorCode::EngineUnavailable, 4),
        (ErrorCode::Timeout, 5),
        (ErrorCode::Internal, 1),
    ] {
        let response = Response::error("r", &ControlError::new(code, "x"));
        assert_eq!(response.exit_code(), exit, "{code:?}");
        assert_eq!(response.error.map(|e| e.retryable), Some(code.retryable()));
    }
}

#[test]
fn framing_is_bounded_and_line_based() -> io::Result<()> {
    let mut buffer = Vec::new();
    write_message(&mut buffer, &Hello::current())?;
    write_message(&mut buffer, &Response::ok("r", serde_json::json!({"a": 1})))?;
    let mut reader = Cursor::new(buffer);
    assert_eq!(read_message::<Hello>(&mut reader)?, Hello::current());
    assert_eq!(read_message::<Response>(&mut reader)?.request_id, "r");
    assert_eq!(
        read_message::<Hello>(&mut reader)
            .map_err(|e| e.kind())
            .err(),
        Some(io::ErrorKind::UnexpectedEof)
    );
    let mut huge = vec![b'x'; MAX_MESSAGE + 10];
    huge.push(b'\n');
    assert_eq!(
        read_message::<Hello>(&mut Cursor::new(huge))
            .map_err(|e| e.kind())
            .err(),
        Some(io::ErrorKind::InvalidData)
    );
    assert_eq!(
        read_message::<Hello>(&mut Cursor::new(b"{\"protocol\":1}\n".to_vec()))
            .map_err(|e| e.kind())
            .err(),
        Some(io::ErrorKind::InvalidData)
    );
    Ok(())
}
