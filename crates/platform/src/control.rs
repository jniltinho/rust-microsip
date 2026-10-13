//! Control socket for the CLI and the MCP server (AG-01, AG-03).
//!
//! Only the profile owner — a process holding the single-instance lock, so
//! never two at once — binds `control.sock` (mode `0600`) in the private
//! runtime directory. Every connection is checked with the kernel's peer
//! credentials (`SO_PEERCRED`): a peer with another user id is dropped before
//! any byte is read, and clients check the owner the same way. Requests are
//! validated here and handed to the owner one at a time per connection; the
//! owner's answer, or a `timeout` error after [`REPLY_DEADLINE`], goes back.
//!
//! Limits: at most [`MAX_CONNECTIONS`] connections, [`PER_CONNECTION_RATE`]
//! requests per second per connection and [`GLOBAL_RATE`] in total; excess
//! requests get `rate_limited` or `busy` without reaching the owner.

use crate::Waker;
use crate::instance::Listener;
use rust_microsip_control::{
    ControlError, ErrorCode, Hello, OwnerMode, PROTOCOL, Request, Response, SCHEMA_VERSION,
    SOCKET_FILE, Welcome, read_message, write_message,
};
use std::fs;
use std::io::{self, BufReader};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Simultaneous client connections.
pub const MAX_CONNECTIONS: usize = 16;
/// Requests per second accepted from one connection.
pub const PER_CONNECTION_RATE: u32 = 10;
/// Requests per second accepted from all connections together.
pub const GLOBAL_RATE: u64 = 50;
/// How long a client waits for the owner's answer to one request.
pub const REPLY_DEADLINE: Duration = Duration::from_secs(5);
/// Requests queued for the owner across connections.
const QUEUE: usize = 64;
/// Handshake deadline.
const HELLO_DEADLINE: Duration = Duration::from_secs(2);
/// A silent connection is closed after this long.
const IDLE: Duration = Duration::from_secs(600);

/// One validated request waiting for the owner.
#[derive(Debug)]
pub struct Incoming {
    /// The request.
    pub request: Request,
    /// Process id of the client, for the approval prompt (informational).
    pub peer_pid: Option<u32>,
    reply: SyncSender<Response>,
    enqueued: Instant,
}
impl Incoming {
    /// The client already got `timeout`: a late mutation must be dropped, or
    /// a retry with a new key would create a second, identical request.
    #[must_use]
    pub fn is_stale(&self) -> bool {
        // One second of margin for recording and answering before the
        // client's own deadline.
        self.enqueued.elapsed() + Duration::from_secs(1) >= REPLY_DEADLINE
    }

    /// Send the answer; ignored when the client already timed out.
    pub fn respond(self, response: Response) {
        let _ = self.reply.try_send(response);
    }
}

/// Failure to start the server or to reach an owner.
#[derive(Debug, thiserror::Error)]
pub enum ControlSocketError {
    /// No owner is listening.
    #[error("no rust-microsip instance is running")]
    Unavailable,
    /// The directory, socket or peer is not owned by this user.
    #[error("{0} is not private to the current user")]
    Insecure(PathBuf),
    /// The owner speaks another protocol version.
    #[error("the running instance speaks an incompatible control protocol")]
    Protocol,
    /// The owner has too many control connections.
    #[error("the running instance is busy; try again")]
    Busy,
    /// I/O failure.
    #[error("{0}")]
    Io(#[from] io::Error),
}

/// Owner side. Dropping it stops accepting and removes the socket.
pub struct ControlServer {
    incoming: Receiver<Incoming>,
    waker: Arc<Mutex<Option<Waker>>>,
    stop: Arc<AtomicBool>,
    socket: PathBuf,
}
impl std::fmt::Debug for ControlServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlServer")
            .field("socket", &self.socket)
            .finish_non_exhaustive()
    }
}

struct Shared {
    tx: SyncSender<Incoming>,
    waker: Arc<Mutex<Option<Waker>>>,
    connections: AtomicUsize,
    /// Requests in the current second: `second << 32 | count`.
    global: AtomicU64,
    welcome: Welcome,
    uid: u32,
}

impl ControlServer {
    /// Start serving in the owner's runtime directory. Holding `owner` proves
    /// this process has the single-instance lock.
    ///
    /// # Errors
    /// [`ControlSocketError::Insecure`] for a foreign socket file, otherwise I/O.
    pub fn start(
        owner: &Listener,
        mode: OwnerMode,
        epoch: u64,
    ) -> Result<Self, ControlSocketError> {
        let dir = owner.runtime_dir();
        let uid = fs::symlink_metadata(dir)?.uid();
        let socket = dir.join(SOCKET_FILE);
        match fs::symlink_metadata(&socket) {
            Ok(meta) if meta.file_type().is_socket() && meta.uid() == uid => {
                fs::remove_file(&socket)?;
            }
            Ok(_) => return Err(ControlSocketError::Insecure(socket)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let listener = UnixListener::bind(&socket)?;
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
        let (tx, incoming) = mpsc::sync_channel(QUEUE);
        let waker: Arc<Mutex<Option<Waker>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Shared {
            tx,
            waker: Arc::clone(&waker),
            connections: AtomicUsize::new(0),
            global: AtomicU64::new(0),
            welcome: Welcome {
                protocol: PROTOCOL.to_owned(),
                version: SCHEMA_VERSION,
                mode,
                epoch,
            },
            uid,
        });
        let thread_stop = Arc::clone(&stop);
        thread::Builder::new()
            .name("control-accept".into())
            .spawn(move || accept_loop(&listener, &shared, &thread_stop))?;
        Ok(Self {
            incoming,
            waker,
            stop,
            socket,
        })
    }

    /// Run `waker` after every queued request (replaces any previous waker).
    pub fn set_waker(&self, waker: Waker) {
        *self.waker.lock().unwrap_or_else(PoisonError::into_inner) = Some(waker);
    }

    /// Next request without blocking.
    pub fn try_recv(&self) -> Option<Incoming> {
        self.incoming.try_recv().ok()
    }

    /// Wait up to `timeout` for the next request.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<Incoming> {
        self.incoming.recv_timeout(timeout).ok()
    }
}
impl Drop for ControlServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = UnixStream::connect(&self.socket);
        let _ = fs::remove_file(&self.socket);
    }
}

fn accept_loop(listener: &UnixListener, shared: &Arc<Shared>, stop: &AtomicBool) {
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let Ok(stream) = stream else {
            thread::sleep(Duration::from_millis(100));
            continue;
        };
        // Foreign users are dropped before anything is read.
        let Ok(credentials) = rustix::net::sockopt::socket_peercred(&stream) else {
            continue;
        };
        if credentials.uid.as_raw() != shared.uid {
            continue;
        }
        if shared.connections.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            shared.connections.fetch_sub(1, Ordering::SeqCst);
            let mut stream = stream;
            let busy = ControlError::new(ErrorCode::Busy, "too many control connections");
            let _ = write_message(&mut stream, &Response::error("", &busy));
            continue;
        }
        let pid = u32::try_from(credentials.pid.as_raw_nonzero().get()).ok();
        let shared = Arc::clone(shared);
        let spawned = thread::Builder::new()
            .name("control-conn".into())
            .spawn(move || {
                let _ = serve(stream, &shared, pid);
                shared.connections.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            thread::sleep(Duration::from_millis(100));
        }
    }
}

fn now_second() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Count one request in the global fixed one-second window.
fn global_allows(counter: &AtomicU64) -> bool {
    let second = now_second() & 0xFFFF_FFFF;
    let mut current = counter.load(Ordering::SeqCst);
    loop {
        let next = if current >> 32 == second {
            if current & 0xFFFF_FFFF >= GLOBAL_RATE {
                return false;
            }
            current + 1
        } else {
            (second << 32) | 1
        };
        match counter.compare_exchange(current, next, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return true,
            Err(actual) => current = actual,
        }
    }
}

fn serve(stream: UnixStream, shared: &Shared, pid: Option<u32>) -> io::Result<()> {
    stream.set_read_timeout(Some(HELLO_DEADLINE))?;
    stream.set_write_timeout(Some(REPLY_DEADLINE))?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let hello: Hello = read_message(&mut reader)?;
    if hello != Hello::current() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    write_message(&mut writer, &shared.welcome)?;
    reader.get_ref().set_read_timeout(Some(IDLE))?;
    let (mut window, mut used) = (Instant::now(), 0u32);
    loop {
        let request: Request = match read_message(&mut reader) {
            Ok(request) => request,
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                // Unparsable but bounded line: answer (rate-limited like any
                // request) and keep the connection.
                if window.elapsed() >= Duration::from_secs(1) {
                    (window, used) = (Instant::now(), 0);
                }
                used += 1;
                if used > PER_CONNECTION_RATE {
                    thread::sleep(Duration::from_millis(100));
                }
                let invalid = ControlError::new(ErrorCode::Validation, "malformed request");
                write_message(&mut writer, &Response::error("", &invalid))?;
                continue;
            }
            Err(error) => return Err(error),
        };
        if window.elapsed() >= Duration::from_secs(1) {
            (window, used) = (Instant::now(), 0);
        }
        used += 1;
        let response = if used > PER_CONNECTION_RATE || !global_allows(&shared.global) {
            let limited = ControlError::new(ErrorCode::RateLimited, "too many requests");
            Response::error(&request.request_id, &limited)
        } else if let Err(error) = request.validate() {
            Response::error(&request.request_id, &error)
        } else {
            forward(request, shared, pid)
        };
        write_message(&mut writer, &response)?;
    }
}

fn forward(request: Request, shared: &Shared, pid: Option<u32>) -> Response {
    let id = request.request_id.clone();
    let (reply, answer) = mpsc::sync_channel(1);
    let incoming = Incoming {
        request,
        peer_pid: pid,
        reply,
        enqueued: Instant::now(),
    };
    match shared.tx.try_send(incoming) {
        Ok(()) => {
            if let Some(wake) = shared
                .waker
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
            {
                wake();
            }
        }
        Err(TrySendError::Full(_)) => {
            return Response::error(
                &id,
                &ControlError::new(ErrorCode::Busy, "the owner is busy"),
            );
        }
        Err(TrySendError::Disconnected(_)) => {
            let gone = ControlError::new(ErrorCode::EngineUnavailable, "the owner is stopping");
            return Response::error(&id, &gone);
        }
    }
    answer.recv_timeout(REPLY_DEADLINE).unwrap_or_else(|_| {
        let late = ControlError::new(
            ErrorCode::Timeout,
            "the owner did not answer in time; query the operation before retrying",
        );
        Response::error(&id, &late)
    })
}

/// Client side of the control socket.
#[derive(Debug)]
pub struct ControlClient {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    welcome: Welcome,
}
impl ControlClient {
    /// Connect to the owner of `runtime_dir` without starting one.
    ///
    /// # Errors
    /// [`ControlSocketError::Unavailable`] when nobody listens,
    /// [`ControlSocketError::Insecure`] for a foreign directory, socket or
    /// peer, [`ControlSocketError::Protocol`] for another version.
    pub fn connect(runtime_dir: &Path) -> Result<Self, ControlSocketError> {
        let uid = own_uid()?;
        let not_found = |e: &io::Error| {
            matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            )
        };
        let meta = match fs::symlink_metadata(runtime_dir) {
            Ok(meta) => meta,
            Err(e) if not_found(&e) => return Err(ControlSocketError::Unavailable),
            Err(e) => return Err(e.into()),
        };
        if !meta.file_type().is_dir() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
            return Err(ControlSocketError::Insecure(runtime_dir.to_owned()));
        }
        let socket = runtime_dir.join(SOCKET_FILE);
        match fs::symlink_metadata(&socket) {
            Ok(meta) if meta.file_type().is_socket() && meta.uid() == uid => {}
            Ok(_) => return Err(ControlSocketError::Insecure(socket)),
            Err(e) if not_found(&e) => return Err(ControlSocketError::Unavailable),
            Err(e) => return Err(e.into()),
        }
        let stream = match UnixStream::connect(&socket) {
            Ok(stream) => stream,
            Err(e) if not_found(&e) => return Err(ControlSocketError::Unavailable),
            Err(e) => return Err(e.into()),
        };
        let peer = rustix::net::sockopt::socket_peercred(&stream).map_err(io::Error::from)?;
        if peer.uid.as_raw() != uid {
            return Err(ControlSocketError::Insecure(socket));
        }
        stream.set_read_timeout(Some(REPLY_DEADLINE + Duration::from_secs(1)))?;
        stream.set_write_timeout(Some(REPLY_DEADLINE))?;
        let mut writer = stream.try_clone()?;
        let mut reader = BufReader::new(stream);
        write_message(&mut writer, &Hello::current())?;
        let first: serde_json::Value =
            read_message(&mut reader).map_err(|_| ControlSocketError::Protocol)?;
        if first.get("status").and_then(serde_json::Value::as_str) == Some("error") {
            return Err(ControlSocketError::Busy);
        }
        let welcome: Welcome =
            serde_json::from_value(first).map_err(|_| ControlSocketError::Protocol)?;
        if welcome.protocol != PROTOCOL || welcome.version != SCHEMA_VERSION {
            return Err(ControlSocketError::Protocol);
        }
        Ok(Self {
            reader,
            writer,
            welcome,
        })
    }

    /// The owner's handshake answer.
    #[must_use]
    pub const fn welcome(&self) -> &Welcome {
        &self.welcome
    }

    /// Send one request and wait for its answer (bounded by the socket
    /// timeouts). Never retries.
    ///
    /// # Errors
    /// I/O errors; `TimedOut`/`WouldBlock` when the owner did not answer.
    pub fn call(&mut self, request: &Request) -> io::Result<Response> {
        write_message(&mut self.writer, request)?;
        read_message(&mut self.reader)
    }
}

fn own_uid() -> io::Result<u32> {
    fs::metadata("/proc/self").map(|m| m.uid())
}

#[cfg(test)]
mod tests;
