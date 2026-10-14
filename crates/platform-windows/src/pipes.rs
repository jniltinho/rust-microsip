//! Single instance and control channel over named pipes (design W4).
//!
//! The Windows counterpart of the Unix `instance` and `control` modules of
//! `rust-microsip-platform`: same API, wire protocols, queues, limits and
//! timeouts. Each profile has two local pipes,
//! `\\.\pipe\rust-microsip-<user SID>-<runtime dir hash>-instance` and
//! `...-control`:
//!
//! - The owner creates each pipe with `FILE_FLAG_FIRST_PIPE_INSTANCE`, so the
//!   name must not exist yet. For the instance pipe this is the
//!   single-instance lock, released by the kernel when the process dies; it
//!   has exactly one server instance, reused for every client, so the lock is
//!   held for as long as the [`Listener`] lives.
//! - Remote clients are rejected (`PIPE_REJECT_REMOTE_CLIENTS`) and the pipes
//!   carry a protected DACL granting access to the current user SID only.
//! - Every accepted client's process token must carry this user's SID and be
//!   neither restricted nor an AppContainer token; otherwise the connection is
//!   closed without a byte (the `SO_PEERCRED` contract of the Unix code).
//!   Clients check the server process the same way, and open the pipe with
//!   identification-only impersonation, so they never talk to an impostor.
//! - I/O is blocking on dedicated threads, as on Unix. Handles are opened
//!   overlapped only to bound every read, write and accept with a timeout.
//!
//! The runtime directory (`%LOCALAPPDATA%\rust-microsip\run`) names the
//! profile in the pipe names and holds the owner's grant files; its DACL is
//! handled with the other private directories (design W6).

use crate::Waker;
use rust_microsip_control::instance::{Ack, InstanceError, MAX_LINE, Request};
use rust_microsip_control::{
    ControlError, ErrorCode, Hello, MAX_MESSAGE, OwnerMode, PROTOCOL, Request as ControlRequest,
    Response, SCHEMA_VERSION, Status, Welcome, read_message, write_message,
};
use std::ffi::c_void;
use std::fmt::Write as _;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_BROKEN_PIPE, ERROR_FILE_NOT_FOUND, ERROR_IO_PENDING, ERROR_NO_DATA,
    ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, ERROR_PIPE_NOT_CONNECTED, GENERIC_READ, GENERIC_WRITE,
    HANDLE, INVALID_HANDLE_VALUE, LocalFree, WAIT_TIMEOUT, WIN32_ERROR,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_INFORMATION_CLASS,
    TOKEN_QUERY, TokenIsAppContainer, TokenIsRestricted, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, OPEN_EXISTING,
    PIPE_ACCESS_DUPLEX, ReadFile, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, WriteFile,
};
use windows_sys::Win32::System::IO::{
    CancelIoEx, GetOverlappedResult, GetOverlappedResultEx, OVERLAPPED,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    GetNamedPipeServerProcessId, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
    PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

// --- Win32 layer: every unsafe block of this module ------------------------

/// Pipe buffer size in each direction.
const PIPE_BUFFER: u32 = 4096;
/// Longest single wait, so accept loops notice a stop request.
const SLICE: Duration = Duration::from_millis(250);
/// Pause before retrying a busy pipe or after a persistent error.
const RETRY: Duration = Duration::from_millis(50);

fn is(error: &io::Error, code: WIN32_ERROR) -> bool {
    error.raw_os_error() == i32::try_from(code).ok()
}

/// NUL-terminated UTF-16 for Win32.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// Take ownership of a handle a Win32 call just returned (null or
/// `INVALID_HANDLE_VALUE` mean failure, with the error in `GetLastError`).
fn owned(raw: HANDLE) -> io::Result<OwnedHandle> {
    if raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `raw` is a valid handle the caller's Win32 call has just
    // created; nothing else owns it, so `OwnedHandle` closes it exactly once.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}

/// Memory returned by a Win32 call that must be released with `LocalFree`.
struct LocalMemory(*mut c_void);
impl Drop for LocalMemory {
    fn drop(&mut self) {
        // SAFETY: the pointer came from a successful Win32 allocation
        // documented as `LocalFree`-owned, and is freed only here, once.
        let _ = unsafe { LocalFree(self.0) };
    }
}

/// Protected DACL with one entry: generic all for `sid` (design W4).
fn owner_only_descriptor(sid: &str) -> io::Result<LocalMemory> {
    let sddl = wide(&format!("D:P(A;;GA;;;{sid})"));
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: `sddl` is NUL-terminated and outlives the call; `descriptor`
    // is a valid out pointer and the optional size pointer may be null.
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(LocalMemory(descriptor))
}

/// Create one overlapped, byte-mode, local-only server instance of `name`
/// readable and writable by `sid` only; `first` fails if the name exists.
fn create_server(name: &[u16], sid: &str, first: bool, instances: u32) -> io::Result<OwnedHandle> {
    let descriptor = owner_only_descriptor(sid)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(u32::MAX),
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let mut open_mode = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
    if first {
        open_mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
    }
    let pipe_mode = PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS;
    // SAFETY: `name` is NUL-terminated and `attributes` (with the descriptor
    // it points to) outlives the call; failure is checked by `owned`.
    let raw = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            open_mode,
            pipe_mode,
            instances,
            PIPE_BUFFER,
            PIPE_BUFFER,
            0,
            &attributes,
        )
    };
    owned(raw)
}

/// Open the client end of `name`. The server may only identify us, never
/// impersonate us.
fn open_client(name: &[u16]) -> io::Result<OwnedHandle> {
    // SAFETY: `name` is NUL-terminated; null security attributes and template
    // are allowed; failure is checked by `owned`.
    let raw = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            ptr::null_mut(),
        )
    };
    owned(raw)
}

fn create_event() -> io::Result<OwnedHandle> {
    // SAFETY: unnamed manual-reset event with default security; failure
    // (null) is checked by `owned`.
    owned(unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) })
}

/// Return a server instance to the listening state, dropping its client.
fn disconnect(pipe: &OwnedHandle) {
    // SAFETY: `pipe` is an open server instance; failure (not connected)
    // leaves it as it was.
    let _ = unsafe { DisconnectNamedPipe(pipe.as_raw_handle()) };
}

fn client_pid(pipe: &OwnedHandle) -> io::Result<u32> {
    let mut pid = 0;
    // SAFETY: `pipe` is an open server instance and `pid` a valid out pointer.
    if unsafe { GetNamedPipeClientProcessId(pipe.as_raw_handle(), &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(pid)
}

fn server_pid(pipe: &OwnedHandle) -> io::Result<u32> {
    let mut pid = 0;
    // SAFETY: `pipe` is an open client end and `pid` a valid out pointer.
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(pid)
}

/// Fill `out` with one token information class.
fn token_information(
    token: &OwnedHandle,
    class: TOKEN_INFORMATION_CLASS,
    out: &mut [u64],
) -> io::Result<()> {
    let len = u32::try_from(size_of_val(out)).unwrap_or(u32::MAX);
    let mut written = 0;
    // SAFETY: `out` is writable for `len` bytes and 8-byte aligned (enough
    // for every token structure); `written` is a valid out pointer.
    let ok = unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            class,
            out.as_mut_ptr().cast(),
            len,
            &mut written,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Query token of process `pid`.
fn process_token(pid: u32) -> io::Result<OwnedHandle> {
    // SAFETY: plain call; failure (null) is checked by `owned`.
    let process = owned(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) })?;
    let mut raw_token: HANDLE = ptr::null_mut();
    // SAFETY: `process` is open with query rights and `raw_token` is a valid
    // out pointer.
    if unsafe { OpenProcessToken(process.as_raw_handle(), TOKEN_QUERY, &mut raw_token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    owned(raw_token)
}

/// String SID of the token's user.
fn token_sid(token: &OwnedHandle) -> io::Result<String> {
    // TOKEN_USER (16 bytes) plus the largest SID (68 bytes).
    let mut user = [0u64; 16];
    token_information(token, TokenUser, &mut user)?;
    let bytes: Vec<u8> = user.iter().flat_map(|word| word.to_ne_bytes()).collect();
    sid_from_token_user(&bytes, user.as_ptr() as usize)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "malformed token user"))
}

/// User and token kind of process `pid`.
fn process_identity(pid: u32) -> io::Result<Identity> {
    let token = process_token(pid)?;
    let sid = token_sid(&token)?;
    // Both classes answer a DWORD; any nonzero value means yes.
    let mut restricted = [0u64];
    token_information(&token, TokenIsRestricted, &mut restricted)?;
    let mut app_container = [0u64];
    token_information(&token, TokenIsAppContainer, &mut app_container)?;
    Ok(Identity {
        sid,
        restricted: restricted[0] != 0,
        app_container: app_container[0] != 0,
    })
}

/// Run one overlapped operation on `pipe` to completion and return the bytes
/// transferred. It never returns while the operation is pending, so the
/// buffer and the `OVERLAPPED` outlive it: on timeout (`TimedOut`) or `stop`
/// the operation is cancelled and waited for.
fn transfer(
    pipe: &OwnedHandle,
    op: Op<'_>,
    timeout: Option<Duration>,
    stop: Option<&AtomicBool>,
) -> io::Result<u32> {
    let handle = pipe.as_raw_handle();
    let event = create_event()?;
    let mut overlapped = OVERLAPPED {
        hEvent: event.as_raw_handle(),
        ..OVERLAPPED::default()
    };
    // From here on the OVERLAPPED is only used through this pointer.
    let pending: *mut OVERLAPPED = &mut overlapped;
    let connect = matches!(op, Op::Connect);
    let started = match op {
        // SAFETY: `handle` is an open overlapped server instance; `pending`
        // stays valid until the operation completes (see below).
        Op::Connect => unsafe { ConnectNamedPipe(handle, pending) },
        Op::Read(buf) => {
            let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
            // SAFETY: `buf` is writable for `len` bytes and borrowed until
            // this function returns, after completion; `pending` likewise.
            unsafe { ReadFile(handle, buf.as_mut_ptr(), len, ptr::null_mut(), pending) }
        }
        Op::Write(buf) => {
            let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
            // SAFETY: `buf` is readable for `len` bytes and borrowed until
            // this function returns, after completion; `pending` likewise.
            unsafe { WriteFile(handle, buf.as_ptr(), len, ptr::null_mut(), pending) }
        }
    };
    if started == 0 {
        let error = io::Error::last_os_error();
        if connect && is(&error, ERROR_PIPE_CONNECTED) {
            return Ok(0);
        }
        if !is(&error, ERROR_IO_PENDING) {
            // Nothing was queued.
            return Err(error);
        }
    }
    let deadline = timeout.map(|t| Instant::now() + t);
    loop {
        let slice = deadline.map_or(SLICE, |d| {
            d.saturating_duration_since(Instant::now()).min(SLICE)
        });
        let millis = u32::try_from(slice.as_millis()).unwrap_or(u32::MAX);
        let mut done = 0;
        // SAFETY: `pending` describes the operation started above on
        // `handle`; `done` is a valid out pointer.
        if unsafe { GetOverlappedResultEx(handle, pending, &mut done, millis, 0) } != 0 {
            return Ok(done);
        }
        let error = io::Error::last_os_error();
        if !is(&error, WAIT_TIMEOUT) {
            // Completed with an error, or the wait failed: either way make
            // sure nothing is pending before the buffers go away.
            let _ = cancel_and_wait(handle, pending);
            return Err(error);
        }
        let expired = deadline.is_some_and(|d| Instant::now() >= d)
            || stop.is_some_and(|s| s.load(Ordering::SeqCst));
        if expired {
            // Data that arrived before the cancel still counts.
            return cancel_and_wait(handle, pending)
                .map_err(|_| io::Error::from(io::ErrorKind::TimedOut));
        }
    }
}

/// Cancel the operation behind `pending` (harmless when already complete)
/// and block until it is no longer pending.
fn cancel_and_wait(handle: HANDLE, pending: *mut OVERLAPPED) -> io::Result<u32> {
    // SAFETY: `pending` is the live OVERLAPPED of an operation on `handle`.
    let _ = unsafe { CancelIoEx(handle, pending) };
    let mut done = 0;
    // SAFETY: as above; waiting (`bWait` true) returns only once the
    // operation has completed or been cancelled.
    if unsafe { GetOverlappedResult(handle, pending, &mut done, 1) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(done)
}

// --- safe layer --------------------------------------------------------------

enum Op<'a> {
    Connect,
    Read(&'a mut [u8]),
    Write(&'a [u8]),
}

/// One end of a connected pipe; clones share the handle (one reads, one writes).
#[derive(Debug, Clone)]
struct Pipe {
    handle: Arc<OwnedHandle>,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
}
impl Pipe {
    fn new(handle: OwnedHandle) -> Self {
        Self {
            handle: Arc::new(handle),
            read_timeout: None,
            write_timeout: None,
        }
    }
}
impl Read for Pipe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        match transfer(&self.handle, Op::Read(buf), self.read_timeout, None) {
            Ok(read) => Ok(usize::try_from(read).unwrap_or(0)),
            // The other end closed: end of stream, as on a socket.
            Err(e)
                if is(&e, ERROR_BROKEN_PIPE)
                    || is(&e, ERROR_PIPE_NOT_CONNECTED)
                    || is(&e, ERROR_NO_DATA) =>
            {
                Ok(0)
            }
            Err(e) => Err(e),
        }
    }
}
impl Write for Pipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        transfer(&self.handle, Op::Write(buf), self.write_timeout, None)
            .map(|written| usize::try_from(written).unwrap_or(0))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Wait until `pipe` connects; `false` once `stop` is set.
fn accept(pipe: &OwnedHandle, stop: &AtomicBool) -> io::Result<bool> {
    match transfer(pipe, Op::Connect, None, Some(stop)) {
        Ok(_) => Ok(!stop.load(Ordering::SeqCst)),
        Err(_) if stop.load(Ordering::SeqCst) => Ok(false),
        Err(e) => Err(e),
    }
}

/// User SID and token kind of a process.
#[derive(Debug, PartialEq, Eq)]
struct Identity {
    sid: String,
    restricted: bool,
    app_container: bool,
}
impl Identity {
    /// Same user, full (non-restricted, non-AppContainer) token.
    fn is_trusted_by(&self, own_sid: &str) -> bool {
        self.sid == own_sid && !self.restricted && !self.app_container
    }
}

/// The client's process id when it runs as `own_sid` with a full token.
/// Any lookup failure counts as untrusted.
fn trusted_client(pipe: &OwnedHandle, own_sid: &str) -> Option<u32> {
    let pid = client_pid(pipe).ok()?;
    process_identity(pid)
        .is_ok_and(|peer| peer.is_trusted_by(own_sid))
        .then_some(pid)
}

fn own_sid() -> io::Result<String> {
    token_sid(&process_token(std::process::id())?)
}

/// The SID inside a `TOKEN_USER` copied to `bytes` from address `base`:
/// its pointer must point back into the copy.
fn sid_from_token_user(bytes: &[u8], base: usize) -> Option<String> {
    let width = size_of::<usize>();
    let pointer = usize::from_ne_bytes(bytes.get(..width)?.try_into().ok()?);
    let offset = pointer.checked_sub(base)?;
    let count = usize::from(*bytes.get(offset.checked_add(1)?)?);
    sid_string(bytes.get(offset..offset.checked_add(8 + 4 * count)?)?)
}

/// Standard string form (`S-1-5-21-...`) of a binary SID, as
/// `ConvertSidToStringSidW` prints it; `None` when malformed.
fn sid_string(sid: &[u8]) -> Option<String> {
    let (&revision, rest) = sid.split_first()?;
    let (&count, rest) = rest.split_first()?;
    if revision != 1 || rest.len() != 6 + 4 * usize::from(count) {
        return None;
    }
    let (authority, subs) = rest.split_at(6);
    let authority = authority
        .iter()
        .fold(0u64, |acc, byte| (acc << 8) | u64::from(*byte));
    let mut text = if authority >> 32 == 0 {
        format!("S-{revision}-{authority}")
    } else {
        format!("S-{revision}-0x{authority:012X}")
    };
    for sub in subs.as_chunks::<4>().0 {
        let _ = write!(text, "-{}", u32::from_le_bytes(*sub));
    }
    Some(text)
}

/// FNV-1a of the lower-cased path: stable across builds and processes
/// (unlike `DefaultHasher`), and Windows paths are case-insensitive.
fn profile_hash(dir: &Path) -> u64 {
    dir.to_string_lossy()
        .to_lowercase()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
}

/// `\\.\pipe\rust-microsip-<SID>-<profile hash>-<kind>`.
fn pipe_name(sid: &str, runtime_dir: &Path, kind: &str) -> String {
    format!(
        r"\\.\pipe\rust-microsip-{sid}-{:016x}-{kind}",
        profile_hash(runtime_dir)
    )
}

/// Why a client could not get a trusted connection.
enum Refused {
    /// No pipe of that name: nobody owns the profile.
    Absent,
    /// Every instance stayed busy until the deadline.
    Busy,
    /// Access denied, or the server is not this user with a full token.
    Foreign,
    Io(io::Error),
}

/// Connect to `name` and check that the server runs as `own_sid`.
fn connect(name: &str, own_sid: &str, deadline: Instant) -> Result<Pipe, Refused> {
    let wide_name = wide(name);
    let handle = loop {
        match open_client(&wide_name) {
            Ok(handle) => break handle,
            Err(e) if is(&e, ERROR_FILE_NOT_FOUND) => return Err(Refused::Absent),
            Err(e) if is(&e, ERROR_ACCESS_DENIED) => return Err(Refused::Foreign),
            Err(e) if is(&e, ERROR_PIPE_BUSY) => {
                if Instant::now() >= deadline {
                    return Err(Refused::Busy);
                }
                thread::sleep(RETRY);
            }
            Err(e) => return Err(Refused::Io(e)),
        }
    };
    let trusted = server_pid(&handle)
        .and_then(process_identity)
        .is_ok_and(|server| server.is_trusted_by(own_sid));
    if !trusted {
        return Err(Refused::Foreign);
    }
    Ok(Pipe::new(handle))
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::from(io::ErrorKind::TimedOut))
}

/// After the last reply, wait (bounded) for the client to close: closing or
/// disconnecting a server instance first could discard the unread reply.
fn linger(pipe: &mut Pipe, limit: Duration) {
    let deadline = Instant::now() + limit;
    let mut sink = [0u8; 256];
    while let Ok(left) = remaining(deadline) {
        pipe.read_timeout = Some(left);
        if !matches!(pipe.read(&mut sink), Ok(read) if read > 0) {
            return;
        }
    }
}

// --- single instance -------------------------------------------------------

/// Requests buffered by the owner before new ones are answered `busy`.
const QUEUE: usize = 16;
/// Pipe read/write timeout for both sides.
const IO_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a forwarding process waits for a starting or busy owner.
const CONNECT_WINDOW: Duration = Duration::from_secs(3);
const DIR_NAME: &str = "rust-microsip";
const RUN_DIR: &str = "run";

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

/// Default runtime directory: `%LOCALAPPDATA%\rust-microsip\run`.
///
/// There is deliberately no fallback. Without it the application runs in its
/// documented degraded mode (no single-instance protection, with a warning).
///
/// # Errors
/// [`InstanceError::NoRuntimeDir`] when `LOCALAPPDATA` is unset, empty or relative.
pub fn default_runtime_dir() -> Result<PathBuf, InstanceError> {
    match std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        Some(dir) if dir.is_absolute() => Ok(dir.join(DIR_NAME).join(RUN_DIR)),
        _ => Err(InstanceError::NoRuntimeDir),
    }
}

/// Create `runtime_dir` (owner-only DACL, like the Unix `0700` check) and
/// return this user's SID.
fn prepare(runtime_dir: &Path) -> Result<String, InstanceError> {
    crate::files::make_private_dir(runtime_dir)
        .map_err(|_| InstanceError::Insecure(runtime_dir.to_owned()))?;
    own_sid().map_err(io_err(runtime_dir))
}

/// Become the owner of `runtime_dir`, or forward `request` to the existing owner.
///
/// Call this before registering SIP or opening the database. On
/// [`Acquired::Forwarded`] the process must exit. Blocks for at most a few
/// seconds while waiting for a busy or starting owner.
///
/// # Errors
/// [`InstanceError::Insecure`] when the pipe belongs to another user or a
/// restricted process, [`InstanceError::NoResponse`]/[`InstanceError::Protocol`]
/// for a misbehaving owner, and [`InstanceError::Io`] otherwise.
pub fn acquire(runtime_dir: &Path, request: &Request) -> Result<Acquired, InstanceError> {
    let sid = prepare(runtime_dir)?;
    let name = pipe_name(&sid, runtime_dir, "instance");
    let deadline = Instant::now() + CONNECT_WINDOW;
    loop {
        if let Some(listener) = Listener::start(runtime_dir, &sid, &name)? {
            return Ok(Acquired::Owner(listener));
        }
        if let Some(ack) = forward(&name, &sid, request, deadline)? {
            return Ok(Acquired::Forwarded(ack));
        }
        // The owner exited between both attempts: try to take over.
        if Instant::now() >= deadline {
            return Err(InstanceError::NoResponse);
        }
        thread::sleep(RETRY);
    }
}

/// Become the owner of `runtime_dir` without forwarding anything: `None`
/// when another process already owns it (the headless service uses this).
///
/// # Errors
/// As [`acquire`].
pub fn acquire_owner(runtime_dir: &Path) -> Result<Option<Listener>, InstanceError> {
    let sid = prepare(runtime_dir)?;
    Listener::start(runtime_dir, &sid, &pipe_name(&sid, runtime_dir, "instance"))
}

/// Owner side: queued, validated requests from other processes.
///
/// Dropping it stops the accept thread and closes the pipe, which releases
/// the single-instance lock.
pub struct Listener {
    dir: PathBuf,
    sid: String,
    pipe: String,
    requests: Receiver<Request>,
    waker: Arc<Mutex<Option<Waker>>>,
    stop: Arc<AtomicBool>,
    service: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl std::fmt::Debug for Listener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Listener")
            .field("pipe", &self.pipe)
            .finish_non_exhaustive()
    }
}
impl Listener {
    /// `None` when the pipe name already exists (another owner, or a squatter
    /// that [`forward`] will refuse).
    fn start(dir: &Path, sid: &str, name: &str) -> Result<Option<Self>, InstanceError> {
        let path = Path::new(name);
        // One instance only, reused for every client: the name never goes
        // away while we live, so it is the lock.
        let handle = match create_server(&wide(name), sid, true, 1) {
            Ok(handle) => handle,
            Err(e) if is(&e, ERROR_ACCESS_DENIED) || is(&e, ERROR_PIPE_BUSY) => return Ok(None),
            Err(e) => return Err(io_err(path)(e)),
        };
        let (tx, requests) = mpsc::sync_channel(QUEUE);
        let waker: Arc<Mutex<Option<Waker>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let service = Arc::new(AtomicBool::new(false));
        let (thread_waker, thread_stop, thread_service) =
            (Arc::clone(&waker), Arc::clone(&stop), Arc::clone(&service));
        let thread_sid = sid.to_owned();
        let thread = thread::Builder::new()
            .name("instance-ipc".into())
            .spawn(move || {
                accept_loop(
                    Pipe::new(handle),
                    &thread_sid,
                    &tx,
                    &thread_waker,
                    &thread_stop,
                    &thread_service,
                );
            })
            .map_err(io_err(path))?;
        Ok(Some(Self {
            dir: dir.to_owned(),
            sid: sid.to_owned(),
            pipe: name.to_owned(),
            requests,
            waker,
            stop,
            service,
            thread: Some(thread),
        }))
    }

    /// Answer every later launch with [`Ack::Service`] instead of queueing it.
    pub fn set_service_mode(&self) {
        self.service.store(true, Ordering::SeqCst);
    }

    /// The runtime directory this process owns (it holds the grant files).
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
        // Bounded: the accept wait polls `stop` and each client is served
        // within IO_TIMEOUT plus the linger. Joining releases the lock before
        // a later owner in this process (tests) tries to take it.
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn accept_loop(
    mut pipe: Pipe,
    sid: &str,
    tx: &SyncSender<Request>,
    waker: &Mutex<Option<Waker>>,
    stop: &AtomicBool,
    service: &AtomicBool,
) {
    loop {
        match accept(&pipe.handle, stop) {
            Ok(true) => {}
            Ok(false) => return,
            // A client that came and went, or a persistent error: reset the
            // instance and do not spin a core.
            Err(_) => {
                disconnect(&pipe.handle);
                thread::sleep(RETRY);
                continue;
            }
        }
        // ponytail: clients are served one at a time (others retry on
        // ERROR_PIPE_BUSY); each has a total IO_TIMEOUT deadline.
        // Foreign, restricted or AppContainer clients get no byte back.
        if trusted_client(&pipe.handle, sid).is_some() {
            let _ = serve(&mut pipe, tx, waker, service.load(Ordering::SeqCst));
        }
        disconnect(&pipe.handle);
    }
}

fn serve(
    pipe: &mut Pipe,
    tx: &SyncSender<Request>,
    waker: &Mutex<Option<Waker>>,
    service: bool,
) -> io::Result<()> {
    pipe.write_timeout = Some(IO_TIMEOUT);
    let ack = match read_line(pipe)?.as_deref().and_then(Request::decode) {
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
    pipe.write_all(reply)?;
    linger(pipe, IO_TIMEOUT);
    Ok(())
}

/// Read one `\n`-terminated line of at most [`MAX_LINE`] bytes before an
/// absolute deadline (not a per-read timeout); `None` if invalid.
fn read_line(pipe: &mut Pipe) -> io::Result<Option<String>> {
    let deadline = Instant::now() + IO_TIMEOUT;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 128];
    loop {
        pipe.read_timeout = Some(remaining(deadline)?);
        let read = pipe.read(&mut chunk)?;
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

/// Forward `request` to the owner of `name`; `None` when nobody owns it.
fn forward(
    name: &str,
    sid: &str,
    request: &Request,
    deadline: Instant,
) -> Result<Option<Ack>, InstanceError> {
    let path = Path::new(name);
    let mut pipe = match connect(name, sid, deadline) {
        Ok(pipe) => pipe,
        Err(Refused::Absent) => return Ok(None),
        Err(Refused::Busy) => return Err(InstanceError::NoResponse),
        Err(Refused::Foreign) => return Err(InstanceError::Insecure(path.to_owned())),
        Err(Refused::Io(e)) => return Err(io_err(path)(e)),
    };
    pipe.write_timeout = Some(IO_TIMEOUT);
    pipe.write_all(request.encode().as_bytes())
        .map_err(io_err(path))?;
    let line = read_line(&mut pipe).map_err(|e| match e.kind() {
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => InstanceError::NoResponse,
        _ => io_err(path)(e),
    })?;
    match line.as_deref() {
        Some("v1 ok") => Ok(Some(Ack::Ok)),
        Some("v1 busy") => Ok(Some(Ack::Busy)),
        Some("v1 invalid") => Ok(Some(Ack::Invalid)),
        Some("v1 service") => Ok(Some(Ack::Service)),
        _ => Err(InstanceError::Protocol),
    }
}

// --- control channel -------------------------------------------------------

/// Simultaneous client connections.
pub const MAX_CONNECTIONS: usize = 16;
/// Requests per second accepted from one connection.
pub const PER_CONNECTION_RATE: u32 = 10;
/// Requests per second accepted from all connections together.
pub const GLOBAL_RATE: u64 = 50;
/// How long a client waits for the owner's answer to one request.
pub const REPLY_DEADLINE: Duration = Duration::from_secs(5);
/// Requests queued for the owner across connections.
const CONTROL_QUEUE: usize = 64;
/// Handshake deadline.
const HELLO_DEADLINE: Duration = Duration::from_secs(2);
/// A silent connection is closed after this long.
const IDLE: Duration = Duration::from_secs(600);

/// One validated request waiting for the owner.
#[derive(Debug)]
pub struct Incoming {
    /// The request.
    pub request: ControlRequest,
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
    /// The pipe or its server is not owned by this user.
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

/// Owner side. Dropping it stops accepting; the pipe name disappears once
/// the open connections end.
pub struct ControlServer {
    incoming: Receiver<Incoming>,
    waker: Arc<Mutex<Option<Waker>>>,
    stop: Arc<AtomicBool>,
    pipe: String,
}
impl std::fmt::Debug for ControlServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlServer")
            .field("pipe", &self.pipe)
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
}

impl ControlServer {
    /// Start serving the owner's profile. Holding `owner` proves this process
    /// has the single-instance lock.
    ///
    /// # Errors
    /// [`ControlSocketError::Insecure`] when the pipe name already exists
    /// (nobody else may hold it while we own the profile), otherwise I/O.
    pub fn start(
        owner: &Listener,
        mode: OwnerMode,
        epoch: u64,
    ) -> Result<Self, ControlSocketError> {
        let name = pipe_name(&owner.sid, owner.runtime_dir(), "control");
        let wide_name = wide(&name);
        let first = match create_server(&wide_name, &owner.sid, true, PIPE_UNLIMITED_INSTANCES) {
            Ok(handle) => handle,
            Err(e) if is(&e, ERROR_ACCESS_DENIED) || is(&e, ERROR_PIPE_BUSY) => {
                return Err(ControlSocketError::Insecure(PathBuf::from(name)));
            }
            Err(e) => return Err(e.into()),
        };
        let (tx, incoming) = mpsc::sync_channel(CONTROL_QUEUE);
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
        });
        let (thread_stop, sid) = (Arc::clone(&stop), owner.sid.clone());
        thread::Builder::new()
            .name("control-accept".into())
            .spawn(move || control_accept_loop(first, &wide_name, &sid, &shared, &thread_stop))?;
        Ok(Self {
            incoming,
            waker,
            stop,
            pipe: name,
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
        // The accept thread notices within one SLICE and closes its instance.
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn control_accept_loop(
    mut listening: OwnedHandle,
    name: &[u16],
    sid: &str,
    shared: &Arc<Shared>,
    stop: &AtomicBool,
) {
    loop {
        match accept(&listening, stop) {
            Ok(true) => {}
            Ok(false) => return,
            Err(_) => {
                disconnect(&listening);
                thread::sleep(RETRY);
                continue;
            }
        }
        // Keep an instance listening (and the name ours) before handing this
        // one over; without a spare, drop the client and reuse the instance.
        let Ok(next) = create_server(name, sid, false, PIPE_UNLIMITED_INSTANCES) else {
            disconnect(&listening);
            thread::sleep(RETRY);
            continue;
        };
        let connected = std::mem::replace(&mut listening, next);
        // Foreign users are dropped before anything is read.
        let Some(pid) = trusted_client(&connected, sid) else {
            continue;
        };
        let mut pipe = Pipe::new(connected);
        if shared.connections.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            shared.connections.fetch_sub(1, Ordering::SeqCst);
            pipe.write_timeout = Some(HELLO_DEADLINE);
            let busy = ControlError::new(ErrorCode::Busy, "too many control connections");
            if write_message(&mut pipe, &Response::error("", &busy)).is_ok() {
                linger(&mut pipe, HELLO_DEADLINE);
            }
            continue;
        }
        let shared = Arc::clone(shared);
        let spawned = thread::Builder::new()
            .name("control-conn".into())
            .spawn(move || {
                let _ = serve_control(pipe, &shared, Some(pid));
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

fn serve_control(pipe: Pipe, shared: &Shared, pid: Option<u32>) -> io::Result<()> {
    let mut writer = pipe.clone();
    writer.write_timeout = Some(REPLY_DEADLINE);
    let mut reader = BufReader::new(pipe);
    reader.get_mut().read_timeout = Some(HELLO_DEADLINE);
    let hello: Hello = read_message(&mut reader)?;
    if hello != Hello::current() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    write_message(&mut writer, &shared.welcome)?;
    reader.get_mut().read_timeout = Some(IDLE);
    let (mut window, mut used) = (Instant::now(), 0u32);
    loop {
        let request: ControlRequest = match read_message(&mut reader) {
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
            forward_control(request, shared, pid)
        };
        write_message(&mut writer, &response)?;
    }
}

fn forward_control(request: ControlRequest, shared: &Shared, pid: Option<u32>) -> Response {
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

/// Client side of the control channel.
#[derive(Debug)]
pub struct ControlClient {
    reader: BufReader<Pipe>,
    writer: Pipe,
    welcome: Welcome,
}
impl ControlClient {
    /// Connect to the owner of `runtime_dir` without starting one.
    ///
    /// # Errors
    /// [`ControlSocketError::Unavailable`] when nobody listens,
    /// [`ControlSocketError::Insecure`] for a pipe served by another user or
    /// a restricted process, [`ControlSocketError::Busy`] when the owner has
    /// too many connections, [`ControlSocketError::Protocol`] for another
    /// version.
    pub fn connect(runtime_dir: &Path) -> Result<Self, ControlSocketError> {
        let sid = own_sid()?;
        let name = pipe_name(&sid, runtime_dir, "control");
        let pipe = match connect(&name, &sid, Instant::now() + CONNECT_WINDOW) {
            Ok(pipe) => pipe,
            Err(Refused::Absent) => return Err(ControlSocketError::Unavailable),
            Err(Refused::Busy) => return Err(ControlSocketError::Busy),
            Err(Refused::Foreign) => return Err(ControlSocketError::Insecure(PathBuf::from(name))),
            Err(Refused::Io(e)) => return Err(e.into()),
        };
        let mut writer = pipe.clone();
        writer.write_timeout = Some(REPLY_DEADLINE);
        let mut reader = BufReader::new(pipe);
        reader.get_mut().read_timeout = Some(REPLY_DEADLINE + Duration::from_secs(1));
        write_message(&mut writer, &Hello::current())?;
        let mut first = Vec::new();
        reader
            .by_ref()
            .take(MAX_MESSAGE as u64 + 1)
            .read_until(b'\n', &mut first)
            .map_err(|_| ControlSocketError::Protocol)?;
        if let Ok(welcome) = read_message::<Welcome>(&mut first.as_slice()) {
            if welcome.protocol != PROTOCOL || welcome.version != SCHEMA_VERSION {
                return Err(ControlSocketError::Protocol);
            }
            return Ok(Self {
                reader,
                writer,
                welcome,
            });
        }
        match read_message::<Response>(&mut first.as_slice()) {
            Ok(response) if response.status == Status::Error => Err(ControlSocketError::Busy),
            _ => Err(ControlSocketError::Protocol),
        }
    }

    /// The owner's handshake answer.
    #[must_use]
    pub const fn welcome(&self) -> &Welcome {
        &self.welcome
    }

    /// Send one request and wait for its answer (bounded by the pipe
    /// timeouts). Never retries.
    ///
    /// # Errors
    /// I/O errors; `TimedOut` when the owner did not answer.
    pub fn call(&mut self, request: &ControlRequest) -> io::Result<Response> {
        write_message(&mut self.writer, request)?;
        read_message(&mut self.reader)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_microsip_control::Operation;
    use rust_microsip_core::DialTarget;

    // --- pure helpers ------------------------------------------------------

    #[test]
    fn sid_strings_match_the_windows_format() {
        // S-1-5-21-1-2-3-1001: revision 1, 5 sub-authorities, NT authority.
        let mut sid = vec![1, 5, 0, 0, 0, 0, 0, 5];
        for sub in [21u32, 1, 2, 3, 1001] {
            sid.extend_from_slice(&sub.to_le_bytes());
        }
        assert_eq!(sid_string(&sid).as_deref(), Some("S-1-5-21-1-2-3-1001"));
        // A wide authority is printed in hex.
        let wide_authority = [1, 0, 1, 0, 0, 0, 0, 0];
        assert_eq!(
            sid_string(&wide_authority).as_deref(),
            Some("S-1-0x010000000000")
        );
        // Truncated, padded or wrong revision: refused.
        assert_eq!(sid_string(&sid[..sid.len() - 1]), None);
        sid.push(0);
        assert_eq!(sid_string(&sid), None);
        assert_eq!(sid_string(&[2, 0, 0, 0, 0, 0, 0, 5]), None);
        assert_eq!(sid_string(&[]), None);
    }

    #[test]
    fn token_user_sid_must_point_into_the_copy() {
        let base = 0x1000usize;
        let mut bytes = (base + 16).to_ne_bytes().to_vec();
        bytes.resize(16, 0);
        bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 5]);
        bytes.extend_from_slice(&18u32.to_le_bytes());
        assert_eq!(
            sid_from_token_user(&bytes, base).as_deref(),
            Some("S-1-5-18")
        );
        // Pointer before the buffer or past its end.
        assert_eq!(sid_from_token_user(&bytes, base + 32), None);
        assert_eq!(sid_from_token_user(&bytes, base - 64), None);
        assert_eq!(sid_from_token_user(&bytes[..4], base), None);
    }

    #[test]
    fn pipe_names_are_per_user_profile_and_case_insensitive() {
        assert_eq!(profile_hash(Path::new("")), 0xcbf2_9ce4_8422_2325);
        assert_eq!(profile_hash(Path::new("a")), 0xaf63_dc4c_8601_ec8c);
        let dir = Path::new(r"C:\Users\Alice\AppData\Local\rust-microsip\run");
        let name = pipe_name("S-1-5-21-1-2-3-1001", dir, "instance");
        assert!(name.starts_with(r"\\.\pipe\rust-microsip-S-1-5-21-1-2-3-1001-"));
        assert!(name.ends_with("-instance"));
        assert_eq!(
            name,
            pipe_name(
                "S-1-5-21-1-2-3-1001",
                Path::new(r"c:\users\alice\appdata\local\RUST-MICROSIP\run"),
                "instance"
            )
        );
        assert_ne!(name, pipe_name("S-1-5-21-1-2-3-1002", dir, "instance"));
        assert_ne!(name, pipe_name("S-1-5-21-1-2-3-1001", dir, "control"));
        assert_ne!(
            name,
            pipe_name("S-1-5-21-1-2-3-1001", &dir.join("other"), "instance")
        );
    }

    #[test]
    fn only_the_same_user_with_a_full_token_is_trusted() {
        let peer = |sid: &str, restricted, app_container| Identity {
            sid: sid.into(),
            restricted,
            app_container,
        };
        assert!(peer("S-1-5-21-1", false, false).is_trusted_by("S-1-5-21-1"));
        assert!(!peer("S-1-5-21-2", false, false).is_trusted_by("S-1-5-21-1"));
        assert!(!peer("S-1-5-21-1", true, false).is_trusted_by("S-1-5-21-1"));
        assert!(!peer("S-1-5-21-1", false, true).is_trusted_by("S-1-5-21-1"));
    }

    #[test]
    fn runtime_dir_requires_absolute_localappdata() {
        // Read-only check of the current environment; never mutates it.
        match std::env::var_os("LOCALAPPDATA") {
            Some(d) if Path::new(&d).is_absolute() => assert_eq!(
                default_runtime_dir().ok(),
                Some(Path::new(&d).join(DIR_NAME).join(RUN_DIR))
            ),
            _ => assert!(matches!(
                default_runtime_dir(),
                Err(InstanceError::NoRuntimeDir)
            )),
        }
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

    // --- in-process round trips (real pipes) --------------------------------

    /// Unique runtime directory under the temp dir, removed on drop.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let n = NEXT.fetch_add(1, Ordering::SeqCst);
            let path =
                std::env::temp_dir().join(format!("rmsip-win-{}-{tag}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            Self(path)
        }
        fn run(&self) -> PathBuf {
            self.0.join("run")
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn sid() -> String {
        own_sid().unwrap_or_else(|e| panic!("{e}"))
    }
    fn owner(dir: &Path) -> Listener {
        match acquire(dir, &Request::Activate) {
            Ok(Acquired::Owner(listener)) => listener,
            other => panic!("expected owner, got {other:?}"),
        }
    }
    fn dial(s: &str) -> Request {
        Request::Dial(DialTarget::parse(s).unwrap_or_else(|e| panic!("{e}")))
    }
    fn status(id: &str) -> ControlRequest {
        ControlRequest {
            schema_version: 1,
            request_id: id.into(),
            idempotency_key: None,
            operation: Operation::StatusGet,
            capability: None,
        }
    }
    /// Raw client on a pipe of `dir`: send `bytes`, return the first line.
    fn raw(dir: &Path, kind: &str, bytes: &[u8]) -> String {
        let name = pipe_name(&sid(), dir, kind);
        let Ok(mut pipe) = connect(&name, &sid(), Instant::now() + CONNECT_WINDOW) else {
            panic!("cannot connect to {name}");
        };
        pipe.read_timeout = Some(Duration::from_secs(5));
        pipe.write_timeout = Some(Duration::from_secs(5));
        let _ = pipe.write_all(bytes);
        let mut line = String::new();
        let _ = BufReader::new(pipe).read_line(&mut line);
        line
    }
    /// Answer every request until the server is gone.
    fn echo_owner(server: ControlServer) -> JoinHandle<usize> {
        thread::spawn(move || {
            let mut served = 0;
            while let Some(incoming) = server.recv_timeout(Duration::from_secs(3)) {
                served += 1;
                let id = incoming.request.request_id.clone();
                incoming.respond(Response::ok(&id, Default::default()));
            }
            served
        })
    }

    #[test]
    fn second_instance_forwards_and_owner_receives() {
        let tmp = TempDir::new("fwd");
        let listener = owner(&tmp.run());
        assert!(tmp.run().is_dir(), "the runtime dir is created");
        let woke = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&woke);
        listener.set_waker(Arc::new(move || flag.store(true, Ordering::SeqCst)));
        for request in [Request::Activate, dial("+5511999990000"), dial("*97")] {
            match acquire(&tmp.run(), &request) {
                Ok(Acquired::Forwarded(Ack::Ok)) => {}
                other => panic!("expected forward, got {other:?}"),
            }
            assert_eq!(listener.recv_timeout(Duration::from_secs(5)), Some(request));
        }
        assert!(woke.load(Ordering::SeqCst));
        assert_eq!(listener.try_recv(), None);
        assert!(matches!(acquire_owner(&tmp.run()), Ok(None)), "still owned");
        // Dropping the owner releases the name for the next one.
        drop(listener);
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
    fn malformed_and_oversized_requests_are_rejected() {
        let tmp = TempDir::new("bad");
        let listener = owner(&tmp.run());
        let long = format!("v1 dial {}\n", "1".repeat(MAX_LINE));
        for bad in [
            &b"v2 activate\n"[..],
            b"v1 dial \n",
            b"v1 dial 10 02\n",
            b"v1 dial  1002\n",
            b"v1 dial 1002\r\n",
            b"v1 dial $(id)\n",
            b"v1 dial \xff\n",
            b"v1 hangup\n",
            b"v1 activate\nv1 activate\n",
            long.as_bytes(),
        ] {
            assert_eq!(
                raw(&tmp.run(), "instance", bad),
                "v1 invalid\n",
                "{:?}",
                String::from_utf8_lossy(bad)
            );
        }
        // Pipes have no half-close: an unterminated line times out unanswered.
        assert_eq!(raw(&tmp.run(), "instance", b"v1 dial 1002"), "");
        assert_eq!(listener.try_recv(), None);
        // The listener keeps serving after rejections.
        assert_eq!(raw(&tmp.run(), "instance", b"v1 dial 1002\n"), "v1 ok\n");
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
            assert_eq!(raw(&tmp.run(), "instance", b"v1 activate\n"), "v1 ok\n");
        }
        assert!(matches!(
            acquire(&tmp.run(), &Request::Activate),
            Ok(Acquired::Forwarded(Ack::Busy))
        ));
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
        Ok(())
    }

    #[test]
    fn two_clients_query_one_owner_and_absent_owner_is_unavailable()
    -> Result<(), ControlSocketError> {
        let tmp = TempDir::new("two");
        assert!(matches!(
            ControlClient::connect(&tmp.run()),
            Err(ControlSocketError::Unavailable)
        ));
        let listener = owner(&tmp.run());
        let server = ControlServer::start(&listener, OwnerMode::Gui, 7)?;
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
        // A mutation without an idempotency key is refused by the pipe layer.
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
        let delivered = server.try_recv();
        assert_eq!(
            delivered.and_then(|incoming| incoming.peer_pid),
            Some(std::process::id()),
            "delivered once, with the client's pid"
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
    fn squatted_names_and_bad_handshakes_are_refused() -> Result<(), ControlSocketError> {
        let tmp = TempDir::new("squat");
        let listener = owner(&tmp.run());
        // Someone already holds the control pipe name: never serve beside it.
        let name = wide(&pipe_name(&sid(), &tmp.run(), "control"));
        let squatter = create_server(&name, &sid(), true, PIPE_UNLIMITED_INSTANCES)?;
        assert!(matches!(
            ControlServer::start(&listener, OwnerMode::Gui, 1),
            Err(ControlSocketError::Insecure(_))
        ));
        drop(squatter);
        let _server = ControlServer::start(&listener, OwnerMode::Gui, 1)?;
        // A wrong protocol name in the hello closes the connection.
        let reply = raw(
            &tmp.run(),
            "control",
            b"{\"protocol\":\"other\",\"version\":1}\n",
        );
        assert_eq!(reply, "", "closed without a welcome");
        Ok(())
    }
}
