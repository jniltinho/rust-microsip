//! Per-user single instance and protected local IPC (LIN-03).
//!
//! The runtime directory (default `$XDG_RUNTIME_DIR/rust-microsip`) must be a
//! real directory owned by the current user with mode `0700`; it is created
//! that way when missing and refused otherwise. Inside it an exclusive
//! advisory lock (`instance.lock`) elects the owner, which is released by the
//! kernel when the process dies, so a stale socket can never block startup and
//! two racing processes cannot both become owner. The owner listens on
//! `instance.sock` (mode `0600`); any other process forwards one request,
//! waits for an acknowledgement and must then exit without registering SIP or
//! opening the database.
//!
//! Peer authentication relies on the private directory. Kernel peer
//! credentials (`SO_PEERCRED`) are not checked here because stable `std` has
//! no API for them; that check is deferred to the CLI/MCP IPC v1 work.
//!
//! Wire protocol, one line each way (at most [`MAX_LINE`] bytes):
//! `v1 activate` | `v1 dial <target>` answered by `v1 ok` | `v1 busy` | `v1 invalid`,
//! or `v1 service` when the headless service owns the profile.

use crate::Waker;
use rust_microsip_core::DialTarget;
use std::fs::{self, DirBuilder, File, OpenOptions, TryLockError};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

/// Maximum length of one protocol line, including the newline.
pub const MAX_LINE: usize = 512;
/// Requests buffered by the owner before new ones are answered `busy`.
const QUEUE: usize = 16;
/// Socket read/write timeout for both sides.
const IO_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a forwarding process waits for the owner to start listening.
const CONNECT_WINDOW: Duration = Duration::from_secs(3);
const DIR_NAME: &str = "rust-microsip";
const LOCK_FILE: &str = "instance.lock";
const SOCKET_FILE: &str = "instance.sock";

/// A validated request forwarded by another process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Raise and focus the existing window.
    Activate,
    /// Show a dial confirmation for this target; never dial automatically.
    Dial(DialTarget),
}
impl Request {
    fn encode(&self) -> String {
        match self {
            Self::Activate => "v1 activate\n".to_owned(),
            Self::Dial(target) => format!("v1 dial {}\n", target.as_str()),
        }
    }
    fn decode(line: &str) -> Option<Self> {
        match line.strip_prefix("v1 ")? {
            "activate" => Some(Self::Activate),
            rest => {
                let target = rest.strip_prefix("dial ")?;
                // DialTarget::parse trims; require the exact canonical form.
                let parsed = DialTarget::parse(target).ok()?;
                (parsed.as_str() == target).then_some(Self::Dial(parsed))
            }
        }
    }
}

/// Owner reply to a forwarded request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ack {
    /// The request was queued for the owner.
    Ok,
    /// The owner queue was full; the request was dropped.
    Busy,
    /// The owner rejected the request as malformed.
    Invalid,
    /// The headless service owns the profile: no window opens and links are
    /// not dialed (`engine_owned_by_service`, `uri_confirmation_unavailable`).
    Service,
}

/// Single-instance or IPC failure. Paths are included; request data never is.
#[derive(Debug, thiserror::Error)]
pub enum InstanceError {
    /// `XDG_RUNTIME_DIR` is unset or not absolute; there is no private fallback.
    #[error("XDG_RUNTIME_DIR is not set to an absolute path")]
    NoRuntimeDir,
    /// The runtime directory or socket is not a private object owned by this user.
    #[error("{0} is not private to the current user (expected owner-only access)")]
    Insecure(PathBuf),
    /// The owner process did not answer within the bounded timeout.
    #[error("the running instance did not respond")]
    NoResponse,
    /// The owner answered with an unexpected line.
    #[error("the running instance sent an invalid reply")]
    Protocol,
    /// An I/O operation failed.
    #[error("{path}: {source}")]
    Io {
        /// Object being accessed.
        path: PathBuf,
        /// Underlying error.
        source: io::Error,
    },
}
fn io_err(path: &Path) -> impl FnOnce(io::Error) -> InstanceError + '_ {
    move |source| InstanceError::Io {
        path: path.to_owned(),
        source,
    }
}

/// Result of [`acquire`].
#[derive(Debug)]
pub enum Acquired {
    /// This process owns the profile and must serve forwarded requests.
    Owner(Listener),
    /// Another process owns the profile and acknowledged the request; exit now.
    Forwarded(Ack),
}

/// Default runtime directory: `$XDG_RUNTIME_DIR/rust-microsip`.
///
/// There is deliberately no fallback to a persistent directory. Without a
/// per-session runtime directory the application runs in a documented
/// degraded mode (no single-instance protection, with a warning); any other
/// error must stop a second instance from starting.
///
/// # Errors
/// [`InstanceError::NoRuntimeDir`] when the variable is unset, empty or relative.
pub fn default_runtime_dir() -> Result<PathBuf, InstanceError> {
    match std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) {
        Some(dir) if dir.is_absolute() => Ok(dir.join(DIR_NAME)),
        _ => Err(InstanceError::NoRuntimeDir),
    }
}

/// Become the owner of `runtime_dir`, or forward `request` to the existing owner.
///
/// Call this before registering SIP or opening the database. On
/// [`Acquired::Forwarded`] the process must exit. Blocks for at most a few
/// seconds while waiting for a starting owner.
///
/// # Errors
/// [`InstanceError::Insecure`] when the directory or socket is not private,
/// [`InstanceError::NoResponse`]/[`InstanceError::Protocol`] for a
/// misbehaving owner, and [`InstanceError::Io`] otherwise.
pub fn acquire(runtime_dir: &Path, request: &Request) -> Result<Acquired, InstanceError> {
    let uid = current_uid()?;
    ensure_private_dir(runtime_dir, uid)?;
    let lock_path = runtime_dir.join(LOCK_FILE);
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&lock_path)
        .map_err(io_err(&lock_path))?;
    let socket = runtime_dir.join(SOCKET_FILE);
    match lock.try_lock() {
        Ok(()) => Listener::start(lock, runtime_dir, socket, uid).map(Acquired::Owner),
        Err(TryLockError::WouldBlock) => forward(&socket, uid, request).map(Acquired::Forwarded),
        Err(TryLockError::Error(e)) => Err(io_err(&lock_path)(e)),
    }
}

/// Become the owner of `runtime_dir` without forwarding anything: `None`
/// when another process already owns it (the headless service uses this).
///
/// # Errors
/// As [`acquire`].
pub fn acquire_owner(runtime_dir: &Path) -> Result<Option<Listener>, InstanceError> {
    let uid = current_uid()?;
    ensure_private_dir(runtime_dir, uid)?;
    let lock_path = runtime_dir.join(LOCK_FILE);
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&lock_path)
        .map_err(io_err(&lock_path))?;
    match lock.try_lock() {
        Ok(()) => Listener::start(lock, runtime_dir, runtime_dir.join(SOCKET_FILE), uid).map(Some),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(io_err(&lock_path)(e)),
    }
}

/// Owner side: queued, validated requests from other processes.
///
/// Dropping it stops the accept thread, removes the socket and releases the lock.
pub struct Listener {
    dir: PathBuf,
    requests: Receiver<Request>,
    waker: Arc<Mutex<Option<Waker>>>,
    stop: Arc<AtomicBool>,
    service: Arc<AtomicBool>,
    socket: PathBuf,
    _lock: File,
}
impl std::fmt::Debug for Listener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Listener")
            .field("socket", &self.socket)
            .finish_non_exhaustive()
    }
}
impl Listener {
    fn start(lock: File, dir: &Path, socket: PathBuf, uid: u32) -> Result<Self, InstanceError> {
        // We hold the lock, so any existing socket is stale; remove it only if ours.
        match fs::symlink_metadata(&socket) {
            Ok(meta) if meta.file_type().is_socket() && meta.uid() == uid => {
                fs::remove_file(&socket).map_err(io_err(&socket))?;
            }
            Ok(_) => return Err(InstanceError::Insecure(socket)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_err(&socket)(e)),
        }
        let listener = UnixListener::bind(&socket).map_err(io_err(&socket))?;
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).map_err(io_err(&socket))?;
        let (tx, requests) = mpsc::sync_channel(QUEUE);
        let waker: Arc<Mutex<Option<Waker>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let service = Arc::new(AtomicBool::new(false));
        let (thread_waker, thread_stop, thread_service) =
            (Arc::clone(&waker), Arc::clone(&stop), Arc::clone(&service));
        thread::Builder::new()
            .name("instance-ipc".into())
            .spawn(move || {
                accept_loop(&listener, &tx, &thread_waker, &thread_stop, &thread_service)
            })
            .map_err(io_err(&socket))?;
        Ok(Self {
            dir: dir.to_owned(),
            requests,
            waker,
            stop,
            service,
            socket,
            _lock: lock,
        })
    }

    /// Answer every later launch with [`Ack::Service`] instead of queueing it.
    pub fn set_service_mode(&self) {
        self.service.store(true, Ordering::SeqCst);
    }

    /// The private runtime directory this process owns.
    #[must_use]
    pub fn runtime_dir(&self) -> &Path {
        &self.dir
    }

    /// Run `waker` after every queued request (replaces any previous waker).
    pub fn set_waker(&self, waker: Waker) {
        *self.waker.lock().unwrap_or_else(PoisonError::into_inner) = Some(waker);
    }

    /// Next queued request without blocking.
    pub fn try_recv(&self) -> Option<Request> {
        self.requests.try_recv().ok()
    }

    /// Wait up to `timeout` for the next request.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<Request> {
        self.requests.recv_timeout(timeout).ok()
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the blocking accept; failure only means the thread is already gone.
        let _ = UnixStream::connect(&self.socket);
        let _ = fs::remove_file(&self.socket);
    }
}

fn accept_loop(
    listener: &UnixListener,
    tx: &SyncSender<Request>,
    waker: &Mutex<Option<Waker>>,
    stop: &AtomicBool,
    service: &AtomicBool,
) {
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        // ponytail: clients are served one at a time; each has a total
        // IO_TIMEOUT deadline, so a slow client cannot hold the thread longer.
        match stream {
            Ok(stream) => {
                let _ = serve(stream, tx, waker, service.load(Ordering::SeqCst));
            }
            // Persistent accept errors (EMFILE, ...) must not spin a core.
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
        }
    }
}

fn serve(
    mut stream: UnixStream,
    tx: &SyncSender<Request>,
    waker: &Mutex<Option<Waker>>,
    service: bool,
) -> io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let ack = match read_line(&stream)?.as_deref().and_then(Request::decode) {
        None => Ack::Invalid,
        Some(_) if service => Ack::Service,
        Some(request) => match tx.try_send(request) {
            Ok(()) => {
                if let Some(wake) = waker
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .as_ref()
                {
                    wake();
                }
                Ack::Ok
            }
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => Ack::Busy,
        },
    };
    let reply: &[u8] = match ack {
        Ack::Ok => b"v1 ok\n",
        Ack::Busy => b"v1 busy\n",
        Ack::Invalid => b"v1 invalid\n",
        Ack::Service => b"v1 service\n",
    };
    stream.write_all(reply)
}

/// Read one `\n`-terminated UTF-8 line of at most [`MAX_LINE`] bytes; `None` if invalid.
/// Read one `\n`-terminated line of at most [`MAX_LINE`] bytes before an
/// absolute deadline (not a per-read timeout).
fn read_line(mut stream: &UnixStream) -> io::Result<Option<String>> {
    let deadline = Instant::now() + IO_TIMEOUT;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 128];
    loop {
        let left = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| io::Error::from(io::ErrorKind::TimedOut))?;
        stream.set_read_timeout(Some(left))?;
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..read]);
        if let Some(end) = buf.iter().position(|b| *b == b'\n') {
            // Exactly one line per connection; trailing bytes are malformed.
            if end + 1 != buf.len() {
                return Ok(None);
            }
            buf.pop();
            return Ok(String::from_utf8(buf).ok());
        }
        if buf.len() >= MAX_LINE {
            return Ok(None);
        }
    }
}

fn forward(socket: &Path, uid: u32, request: &Request) -> Result<Ack, InstanceError> {
    let deadline = Instant::now() + CONNECT_WINDOW;
    let mut stream = loop {
        // The owner may still be binding: wait for its socket within the window.
        match fs::symlink_metadata(socket) {
            Ok(meta) if !(meta.file_type().is_socket() && meta.uid() == uid) => {
                return Err(InstanceError::Insecure(socket.to_owned()));
            }
            Ok(_) => {
                if let Ok(stream) = UnixStream::connect(socket) {
                    break stream;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_err(socket)(e)),
        }
        if Instant::now() >= deadline {
            return Err(InstanceError::NoResponse);
        }
        thread::sleep(Duration::from_millis(50));
    };
    let timeouts = stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(IO_TIMEOUT)));
    timeouts.map_err(io_err(socket))?;
    stream
        .write_all(request.encode().as_bytes())
        .map_err(io_err(socket))?;
    let line = read_line(&stream).map_err(|e| match e.kind() {
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => InstanceError::NoResponse,
        _ => io_err(socket)(e),
    })?;
    match line.as_deref() {
        Some("v1 ok") => Ok(Ack::Ok),
        Some("v1 busy") => Ok(Ack::Busy),
        Some("v1 invalid") => Ok(Ack::Invalid),
        Some("v1 service") => Ok(Ack::Service),
        _ => Err(InstanceError::Protocol),
    }
}

/// Effective user id of this process (owner of `/proc/self`, Linux only).
fn current_uid() -> Result<u32, InstanceError> {
    let proc = Path::new("/proc/self");
    fs::metadata(proc).map(|m| m.uid()).map_err(io_err(proc))
}

/// Create `dir` with mode 0700 if missing; refuse symlinks, foreign owners and
/// any group/other permission bits. The parent must already exist.
fn ensure_private_dir(dir: &Path, uid: u32) -> Result<(), InstanceError> {
    match DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(io_err(dir)(e)),
    }
    let meta = fs::symlink_metadata(dir).map_err(io_err(dir))?;
    if !meta.file_type().is_dir() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
        return Err(InstanceError::Insecure(dir.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    /// Unique temporary directory removed on drop.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = NEXT.fetch_add(1, Ordering::SeqCst);
            let path =
                std::env::temp_dir().join(format!("rmsip-plat-{}-{tag}-{n}", std::process::id()));
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
        match acquire(dir, &Request::Activate) {
            Ok(Acquired::Owner(l)) => l,
            other => panic!("expected owner, got {other:?}"),
        }
    }
    fn dial(s: &str) -> Request {
        Request::Dial(DialTarget::parse(s).unwrap_or_else(|e| panic!("{e}")))
    }
    fn raw(dir: &Path, bytes: &[u8]) -> String {
        let mut s = UnixStream::connect(dir.join(SOCKET_FILE)).unwrap_or_else(|e| panic!("{e}"));
        s.set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap_or_else(|e| panic!("{e}"));
        let _ = s.write_all(bytes);
        let _ = s.shutdown(std::net::Shutdown::Write);
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    }

    #[test]
    fn second_instance_forwards_and_owner_receives() {
        let tmp = TempDir::new("fwd");
        let listener = owner(&tmp.run());
        let woke = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&woke);
        listener.set_waker(Arc::new(move || flag.store(true, Ordering::SeqCst)));

        let meta = fs::symlink_metadata(tmp.run()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(meta.mode() & 0o777, 0o700);
        let sock =
            fs::symlink_metadata(tmp.run().join(SOCKET_FILE)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(sock.mode() & 0o777, 0o600);

        for request in [Request::Activate, dial("+5511999990000"), dial("*97")] {
            match acquire(&tmp.run(), &request) {
                Ok(Acquired::Forwarded(Ack::Ok)) => {}
                other => panic!("expected forward, got {other:?}"),
            }
            assert_eq!(listener.recv_timeout(Duration::from_secs(5)), Some(request));
        }
        assert!(woke.load(Ordering::SeqCst));
        assert_eq!(listener.try_recv(), None);
    }

    #[test]
    fn stale_socket_is_replaced_after_owner_exit() {
        let tmp = TempDir::new("stale");
        fs::create_dir(tmp.run()).unwrap_or_else(|e| panic!("{e}"));
        fs::set_permissions(tmp.run(), fs::Permissions::from_mode(0o700))
            .unwrap_or_else(|e| panic!("{e}"));
        // A leftover socket from a crashed owner: bound, never listened on, lock free.
        let stale =
            UnixListener::bind(tmp.run().join(SOCKET_FILE)).unwrap_or_else(|e| panic!("{e}"));
        drop(stale);
        assert!(tmp.run().join(SOCKET_FILE).exists());
        let listener = owner(&tmp.run());
        assert!(matches!(
            acquire(&tmp.run(), &Request::Activate),
            Ok(Acquired::Forwarded(Ack::Ok))
        ));
        assert_eq!(
            listener.recv_timeout(Duration::from_secs(5)),
            Some(Request::Activate)
        );
        drop(listener);
        assert!(!tmp.run().join(SOCKET_FILE).exists());
        let _again = owner(&tmp.run());
    }

    #[test]
    fn concurrent_starts_elect_exactly_one_owner() {
        let tmp = TempDir::new("race");
        let dir = tmp.run();
        let results: Vec<_> = (0..8)
            .map(|_| {
                let dir = dir.clone();
                thread::spawn(move || acquire(&dir, &Request::Activate))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|_| panic!("thread panicked")))
            .collect();
        let owners = results
            .iter()
            .filter(|r| matches!(r, Ok(Acquired::Owner(_))))
            .count();
        let forwarded = results
            .iter()
            .filter(|r| matches!(r, Ok(Acquired::Forwarded(_))))
            .count();
        assert_eq!((owners, forwarded), (1, 7), "{results:?}");
    }

    #[test]
    fn insecure_directories_are_refused() {
        let tmp = TempDir::new("perm");
        fs::create_dir(tmp.run()).unwrap_or_else(|e| panic!("{e}"));
        fs::set_permissions(tmp.run(), fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(matches!(
            acquire(&tmp.run(), &Request::Activate),
            Err(InstanceError::Insecure(_))
        ));

        let real = tmp.0.join("real");
        DirBuilder::new()
            .mode(0o700)
            .create(&real)
            .unwrap_or_else(|e| panic!("{e}"));
        let link = tmp.0.join("link");
        symlink(&real, &link).unwrap_or_else(|e| panic!("{e}"));
        assert!(matches!(
            acquire(&link, &Request::Activate),
            Err(InstanceError::Insecure(_))
        ));

        // A non-socket at the socket path is never removed or trusted.
        fs::write(real.join(SOCKET_FILE), b"x").unwrap_or_else(|e| panic!("{e}"));
        assert!(matches!(
            acquire(&real, &Request::Activate),
            Err(InstanceError::Insecure(_))
        ));
        assert!(real.join(SOCKET_FILE).is_file());
    }

    #[test]
    fn malformed_and_oversized_requests_are_rejected() {
        let tmp = TempDir::new("bad");
        let listener = owner(&tmp.run());
        let long = format!("v1 dial {}\n", "1".repeat(MAX_LINE));
        for bad in [
            &b"v1 dial 1002"[..],
            b"v2 activate\n",
            b"v1 dial \n",
            b"v1 dial 10 02\n",
            b"v1 dial  1002\n",
            b"v1 dial 1002\r\n",
            b"v1 dial $(id)\n",
            b"v1 dial \xff\n",
            b"v1 hangup\n",
            long.as_bytes(),
        ] {
            assert_eq!(
                raw(&tmp.run(), bad),
                "v1 invalid\n",
                "{:?}",
                String::from_utf8_lossy(bad)
            );
        }
        assert_eq!(listener.try_recv(), None);
        // The listener keeps serving after rejections.
        assert_eq!(raw(&tmp.run(), b"v1 dial 1002\n"), "v1 ok\n");
        assert_eq!(
            listener.recv_timeout(Duration::from_secs(5)),
            Some(dial("1002"))
        );
    }

    #[test]
    fn full_queue_answers_busy() {
        let tmp = TempDir::new("busy");
        let _listener = owner(&tmp.run());
        for _ in 0..QUEUE {
            assert_eq!(raw(&tmp.run(), b"v1 activate\n"), "v1 ok\n");
        }
        assert!(matches!(
            acquire(&tmp.run(), &Request::Activate),
            Ok(Acquired::Forwarded(Ack::Busy))
        ));
    }

    #[test]
    fn runtime_dir_requires_absolute_xdg() {
        // Read-only check of the current environment; never mutates it.
        match std::env::var_os("XDG_RUNTIME_DIR") {
            Some(d) if Path::new(&d).is_absolute() => {
                assert_eq!(
                    default_runtime_dir().ok(),
                    Some(Path::new(&d).join(DIR_NAME))
                );
            }
            _ => assert!(matches!(
                default_runtime_dir(),
                Err(InstanceError::NoRuntimeDir)
            )),
        }
    }

    #[test]
    fn a_service_owner_turns_later_launches_away() -> Result<(), InstanceError> {
        let tmp = TempDir::new("service");
        let listener = owner(&tmp.run());
        listener.set_service_mode();
        let target = DialTarget::parse("1002").map_err(|_| InstanceError::Protocol)?;
        for request in [Request::Activate, Request::Dial(target)] {
            assert!(matches!(
                acquire(&tmp.run(), &request)?,
                Acquired::Forwarded(Ack::Service)
            ));
        }
        assert!(listener.try_recv().is_none(), "nothing reaches the service");
        assert!(acquire_owner(&tmp.run())?.is_none(), "still owned");
        Ok(())
    }
}
