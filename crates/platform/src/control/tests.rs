use super::*;
use crate::instance::{Acquired, Request as InstanceRequest, acquire};
use rust_microsip_control::{Operation, Status};
use std::fs::DirBuilder;
use std::io::{BufRead, Write};
use std::os::unix::fs::DirBuilderExt;

struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("rmsip-ctl-{}-{tag}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap_or_else(|e| panic!("{e}"));
        Self(path)
    }
    fn run(&self) -> PathBuf {
        self.0.join("run")
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn owner(dir: &Path) -> Listener {
    match acquire(dir, &InstanceRequest::Activate) {
        Ok(Acquired::Owner(listener)) => listener,
        other => panic!("expected owner, got {other:?}"),
    }
}

fn status(id: &str) -> Request {
    Request {
        schema_version: 1,
        request_id: id.into(),
        idempotency_key: None,
        operation: Operation::StatusGet,
    }
}

/// Answer every request with its operation name until the server is gone.
fn echo_owner(server: ControlServer) -> thread::JoinHandle<usize> {
    thread::spawn(move || {
        let mut served = 0;
        while let Some(incoming) = server.recv_timeout(Duration::from_secs(3)) {
            served += 1;
            let name = incoming.request.operation.name();
            let id = incoming.request.request_id.clone();
            incoming.respond(Response::ok(&id, serde_json::json!({ "op": name })));
        }
        served
    })
}

#[test]
fn two_clients_query_one_owner_and_absent_owner_is_unavailable() -> Result<(), ControlSocketError> {
    let tmp = TempDir::new("two");
    assert!(matches!(
        ControlClient::connect(&tmp.run()),
        Err(ControlSocketError::Unavailable)
    ));
    let listener = owner(&tmp.run());
    let server = ControlServer::start(&listener, OwnerMode::Gui, 7)?;
    let meta = fs::symlink_metadata(tmp.run().join(SOCKET_FILE))?;
    assert_eq!(meta.mode() & 0o777, 0o600);
    let served = echo_owner(server);
    let mut a = ControlClient::connect(&tmp.run())?;
    let mut b = ControlClient::connect(&tmp.run())?;
    assert_eq!(a.welcome().epoch, 7);
    assert_eq!(a.welcome().mode, OwnerMode::Gui);
    for (index, id) in ["a1", "b1", "a2"].into_iter().enumerate() {
        let client = if index == 1 { &mut b } else { &mut a };
        let response = client.call(&status(id))?;
        assert_eq!(response.request_id, id);
        assert_eq!(response.status, Status::Ok);
    }
    drop((a, b));
    drop(listener);
    assert_eq!(served.join().unwrap_or(0), 3);
    Ok(())
}

#[test]
fn invalid_requests_never_reach_the_owner() -> Result<(), ControlSocketError> {
    let tmp = TempDir::new("invalid");
    let listener = owner(&tmp.run());
    let server = ControlServer::start(&listener, OwnerMode::Service, 1)?;
    let mut client = ControlClient::connect(&tmp.run())?;
    // A mutation without an idempotency key is refused by the socket layer.
    let mut dial = status("d1");
    dial.operation = Operation::CallDial {
        target: "1002".into(),
    };
    let response = client.call(&dial)?;
    assert_eq!(response.error.map(|e| e.code), Some(ErrorCode::Validation));
    // A malformed line is answered and the connection stays usable.
    client.writer.write_all(b"{\"op\":\"shell\"}\n")?;
    let mut line = String::new();
    client.reader.read_line(&mut line)?;
    assert!(line.contains("\"validation\""));
    assert!(server.try_recv().is_none(), "nothing was forwarded");
    // Owner silent: the client gets a timeout, never a hang.
    let started = Instant::now();
    let response = client.call(&status("s1"))?;
    assert_eq!(response.error.map(|e| e.code), Some(ErrorCode::Timeout));
    assert!(started.elapsed() < REPLY_DEADLINE + Duration::from_secs(2));
    assert!(
        server.try_recv().is_some(),
        "the request was delivered once"
    );
    Ok(())
}

#[test]
fn bursts_are_rate_limited_per_connection() -> Result<(), ControlSocketError> {
    let tmp = TempDir::new("rate");
    let listener = owner(&tmp.run());
    let served = echo_owner(ControlServer::start(&listener, OwnerMode::Gui, 1)?);
    let mut client = ControlClient::connect(&tmp.run())?;
    let limited = (0..PER_CONNECTION_RATE + 5)
        .filter_map(|i| client.call(&status(&format!("r{i}"))).ok())
        .filter(|r| r.error.as_ref().map(|e| e.code) == Some(ErrorCode::RateLimited))
        .count();
    assert!(limited >= 5, "limited {limited}");
    drop(client);
    drop(listener);
    let _ = served.join();
    Ok(())
}

#[test]
fn foreign_socket_files_and_bad_handshakes_are_refused() -> Result<(), ControlSocketError> {
    let tmp = TempDir::new("foreign");
    let listener = owner(&tmp.run());
    // A regular file where the socket should be is never removed or used.
    fs::write(tmp.run().join(SOCKET_FILE), b"not a socket")?;
    assert!(matches!(
        ControlServer::start(&listener, OwnerMode::Gui, 1),
        Err(ControlSocketError::Insecure(_))
    ));
    assert!(matches!(
        ControlClient::connect(&tmp.run()),
        Err(ControlSocketError::Insecure(_))
    ));
    fs::remove_file(tmp.run().join(SOCKET_FILE))?;
    let _server = ControlServer::start(&listener, OwnerMode::Gui, 1)?;
    // A wrong protocol name in the hello closes the connection.
    let mut raw = UnixStream::connect(tmp.run().join(SOCKET_FILE))?;
    raw.set_read_timeout(Some(Duration::from_secs(3)))?;
    raw.write_all(b"{\"protocol\":\"other\",\"version\":1}\n")?;
    let mut reply = String::new();
    let read = BufReader::new(raw).read_line(&mut reply)?;
    assert_eq!(read, 0, "closed without a welcome");
    Ok(())
}

#[test]
fn late_requests_are_stale_with_a_margin_before_the_client_deadline() {
    let incoming = |age: Duration| {
        let (reply, _answer) = mpsc::sync_channel(1);
        Incoming {
            request: status("s"),
            peer_pid: None,
            reply,
            enqueued: Instant::now() - age,
        }
    };
    assert!(!incoming(Duration::ZERO).is_stale());
    assert!(!incoming(REPLY_DEADLINE - Duration::from_millis(1100)).is_stale());
    assert!(incoming(REPLY_DEADLINE - Duration::from_millis(1000)).is_stale());
    assert!(incoming(REPLY_DEADLINE).is_stale());
}
