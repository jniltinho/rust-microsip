//! Production telephony engine: one account, one call at a time.
//!
//! The engine runs a Tokio runtime on a dedicated thread. The UI talks to it
//! only through [`EngineHandle`]: bounded, non-blocking [`Command`] submission
//! and [`Event`] polling. Dialogs, SDP, SRTP keys and RTP sockets never leave
//! the engine. Audio crosses the boundary only through [`MediaIo`].

mod media;
mod sdp;
mod sip;

use media::{MediaControl, MediaParams, SharedMedia};
use rsipstack::{
    dialog::{
        DialogId,
        dialog::{Dialog, DialogState},
        invitation::InviteOption,
        invite_dialog::InviteDialog,
    },
    sip::{Header, HeadersExt, Method, Request, Response, StatusCode, ToTypedHeader},
    transaction::transaction::Transaction,
};
use rust_microsip_core::{
    AccountSettings, CallId, CallPhase, CallSnapshot, Command, DialTarget, Direction, EndReason,
    Event, MediaIo, MediaSecurity, Notice, RegistrationFailure, RegistrationState, TransferState,
};
use rust_microsip_media::Codec;
use sdp::{Flow, LocalDescription, LocalKey, RemoteDescription, SdpError};
use secrecy::SecretString;
use std::{
    collections::VecDeque,
    net::SocketAddr,
    sync::{Arc, Mutex},
    thread::JoinHandle,
    time::Duration,
};
use tokio::{net::UdpSocket, sync::mpsc, task::JoinHandle as Task};
use zeroize::Zeroizing;

const COMMAND_CAPACITY: usize = 64;
const EVENT_CAPACITY: usize = 256;
const BACKLOG_LIMIT: usize = 4096;
const OFFER_CODECS: [Codec; 2] = [Codec::Pcmu, Codec::Pcma];
const MAX_REGISTRATION_ATTEMPTS: u32 = 8;

/// Messages from engine-owned tasks back to the actor.
enum Internal {
    Incoming(Box<Transaction>),
    Dialog(Box<DialogState>),
    Registered(
        Result<
            (
                Box<rsipstack::dialog::registration::Registration>,
                u32,
                bool,
            ),
            RegistrationFailure,
        >,
        u64,
    ),
    /// Generation and refresh sequence: only the latest timer acts.
    RefreshRegistration(u64, u64),
    Outgoing(CallId, Result<(InviteDialog, Option<Response>), ()>),
    Reinvite(CallId, Flow, Option<Response>),
    Referred(CallId, Option<u16>),
    MediaFailed(CallId, media::MediaFailure),
    /// A CANCEL that never completed; end the call locally.
    CancelDeadline(CallId),
    TransferDeadline(CallId, u64),
}

fn classify_connect(error: &rsipstack::Error) -> RegistrationFailure {
    if let rsipstack::Error::IoError(io) = error
        && io
            .get_ref()
            .and_then(|e| e.downcast_ref::<rustls::Error>())
            .is_some()
    {
        return RegistrationFailure::Certificate;
    }
    RegistrationFailure::Network
}

/// Which certificates authenticate a TLS registrar.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum TrustAnchors {
    /// The operating system CA bundle.
    #[default]
    System,
    /// Only this PEM bundle (replaces the system roots).
    Custom(Vec<u8>),
    /// A configured bundle could not be read safely: TLS accounts fail with
    /// a certificate error instead of falling back to the system roots.
    Unusable,
}

/// Non-blocking front end owned by the UI thread.
pub struct EngineHandle {
    commands: mpsc::Sender<Command>,
    events: Mutex<mpsc::Receiver<Event>>,
    custom_ca: Arc<Mutex<TrustAnchors>>,
    thread: Option<JoinHandle<()>>,
}

impl EngineHandle {
    /// Start the engine thread. `wake` is called (from the engine thread)
    /// whenever new events are ready, e.g. to request a UI repaint.
    ///
    /// # Errors
    /// Returns an I/O error if the runtime or thread cannot be created.
    pub fn spawn(
        media: Box<dyn MediaIo>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> std::io::Result<Self> {
        let (commands, command_rx) = mpsc::channel(COMMAND_CAPACITY);
        let (event_tx, events) = mpsc::channel(EVENT_CAPACITY);
        let custom_ca = Arc::new(Mutex::new(TrustAnchors::System));
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("rust-microsip-sip")
            .build()?;
        let ca = custom_ca.clone();
        let thread = std::thread::Builder::new()
            .name("rust-microsip-engine".into())
            .spawn(move || {
                runtime.block_on(async move {
                    let (internal_tx, internal_rx) = mpsc::channel(256);
                    let mut actor = Actor {
                        sink: Sink {
                            tx: event_tx,
                            backlog: VecDeque::new(),
                            wake,
                        },
                        media: Arc::new(Mutex::new(media)),
                        custom_ca: ca,
                        transfer_seq: 0,
                        internal: internal_tx,
                        account: None,
                        registration: RegistrationState::Disabled,
                        generation: 0,
                        call: None,
                        next_call: 1,
                        dnd: false,
                        pending_byes: Vec::new(),
                        refresh_seq: 0,
                    };
                    actor.run(command_rx, internal_rx).await;
                });
                runtime.shutdown_timeout(Duration::from_secs(2));
            })?;
        Ok(Self {
            commands,
            events: Mutex::new(events),
            custom_ca,
            thread: Some(thread),
        })
    }

    /// Choose the TLS trust anchors used by subsequent registrations.
    pub fn set_trust(&self, trust: TrustAnchors) {
        if let Ok(mut current) = self.custom_ca.lock() {
            *current = trust;
        }
    }

    /// Queue a command without blocking.
    ///
    /// # Errors
    /// Returns [`Notice::Overloaded`] when the bounded queue is full or the
    /// engine has stopped.
    pub fn send(&self, command: Command) -> Result<(), Notice> {
        self.commands
            .try_send(command)
            .map_err(|_| Notice::Overloaded)
    }

    /// Drain all currently available events without blocking.
    pub fn try_events(&self) -> Vec<Event> {
        let mut out = Vec::new();
        if let Ok(mut events) = self.events.lock() {
            while let Ok(event) = events.try_recv() {
                out.push(event);
            }
        }
        out
    }

    /// Request shutdown and wait up to `timeout` for the engine thread.
    /// Returns `true` if the engine stopped in time.
    pub fn shutdown(mut self, timeout: Duration) -> bool {
        let _ = self.commands.try_send(Command::Shutdown);
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            if self.thread.as_ref().is_none_or(JoinHandle::is_finished) {
                if let Some(thread) = self.thread.take() {
                    let _ = thread.join();
                }
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }
}

/// Ordered event output with a bounded overflow backlog. Notices are shed
/// first under pressure; call and registration snapshots are kept.
struct Sink {
    tx: mpsc::Sender<Event>,
    backlog: VecDeque<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
}
impl Sink {
    fn flush(&mut self) {
        while let Some(event) = self.backlog.pop_front() {
            if let Err(error) = self.tx.try_send(event) {
                self.backlog.push_front(error.into_inner());
                break;
            }
        }
    }
    fn emit(&mut self, event: Event) {
        self.flush();
        if self.backlog.is_empty() {
            match self.tx.try_send(event) {
                Ok(()) => {}
                Err(error) => self.backlog.push_back(error.into_inner()),
            }
        } else {
            self.backlog.push_back(event);
        }
        // Shed in order: notices, superseded registration states, non-final
        // call snapshots. Final snapshots and `Stopped` are never shed.
        while self.backlog.len() > BACKLOG_LIMIT {
            let last_registration = self
                .backlog
                .iter()
                .rposition(|e| matches!(e, Event::Registration(_)));
            let index = self
                .backlog
                .iter()
                .position(|e| matches!(e, Event::Notice(_)))
                .or_else(|| {
                    self.backlog.iter().enumerate().position(|(i, e)| {
                        matches!(e, Event::Registration(_)) && Some(i) != last_registration
                    })
                })
                .or_else(|| {
                    self.backlog
                        .iter()
                        .position(|e| matches!(e, Event::Call(c) if !c.phase.is_final()))
                });
            match index {
                Some(index) => {
                    self.backlog.remove(index);
                }
                None => break,
            }
        }
        (self.wake)();
    }
}

struct Account {
    stack: sip::Stack,
    registration: Option<rsipstack::dialog::registration::Registration>,
    attempts: u32,
    /// A REGISTER is outstanding; never run two concurrently.
    in_flight: bool,
    /// The previous refresh was the fast follow-up of a slow REGISTER.
    fast_refreshed: bool,
}

struct Transfer {
    state: TransferState,
}

struct Call {
    id: CallId,
    direction: Direction,
    remote: String,
    phase: CallPhase,
    muted: bool,
    secure: bool,
    transfer: Option<Transfer>,
    version: u64,
    dialog: Option<InviteDialog>,
    dialog_id: Option<DialogId>,
    cancel_requested: bool,
    socket: Arc<UdpSocket>,
    local_key: Option<LocalKey>,
    crypto_tag: u32,
    sdp_version: u64,
    codec: Codec,
    event_pt: Option<u8>,
    remote_media: Option<SocketAddr>,
    remote_key_line: Zeroizing<String>,
    local_flow: Flow,
    remote_flow: Flow,
    offer: Option<RemoteDescription>,
    /// The codec (and event PT) were agreed; describe only those from now on.
    negotiated: bool,
    reinvite_pending: bool,
    media: Option<(mpsc::Sender<MediaControl>, Task<media::MediaStats>)>,
    _handle: Option<Task<()>>,
}
impl Call {
    fn snapshot(&mut self) -> CallSnapshot {
        self.version += 1;
        CallSnapshot {
            id: self.id,
            direction: self.direction,
            remote: self.remote.clone(),
            phase: self.phase,
            muted: self.muted,
            secure: self.secure,
            transfer: self.transfer.as_ref().map(|t| t.state),
            version: self.version,
        }
    }
    fn describe(&self, flow: Flow) -> Zeroizing<Vec<u8>> {
        let address = self
            .socket
            .local_addr()
            .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 0)));
        let codecs = [self.codec];
        // An answer (or later offer) carries only what was negotiated: the
        // chosen codec, and telephone-event only if the peer offered it.
        LocalDescription {
            address,
            version: self.sdp_version,
            codecs: if self.negotiated {
                &codecs
            } else {
                &OFFER_CODECS
            },
            event_pt: if self.negotiated {
                self.event_pt
            } else {
                Some(sdp::EVENT_PT)
            },
            key: self.local_key.as_ref().map(|k| (k, self.crypto_tag)),
            flow,
        }
        .render()
    }
    fn control(&self, control: MediaControl) {
        if let Some((tx, _)) = &self.media {
            let _ = tx.try_send(control);
        }
    }
}

fn user_part(request: &Request) -> String {
    request
        .from_header()
        .ok()
        .and_then(|h| h.typed().ok())
        .and_then(|from| from.uri.user().map(str::to_owned))
        .map(|user| {
            user.chars()
                .filter(|c| !c.is_control() && !is_format_char(*c))
                .take(64)
                .collect()
        })
        .unwrap_or_else(|| "unknown".into())
}

/// Invisible formatting characters (bidi overrides, zero-width) that could
/// spoof a displayed caller identity.
fn is_format_char(c: char) -> bool {
    matches!(c, '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{FEFF}')
}

fn end_reason(status: Option<u16>) -> EndReason {
    match status {
        Some(486 | 600) => EndReason::Busy,
        Some(487) => EndReason::Canceled,
        Some(408 | 480) => EndReason::NoAnswer,
        Some(488 | 606) => EndReason::NotAcceptable,
        _ => EndReason::Failed,
    }
}

fn sdp_headers() -> Option<Vec<Header>> {
    Some(vec![Header::ContentType("application/sdp".into())])
}

fn sipfrag(body: &[u8]) -> Option<u16> {
    let line = std::str::from_utf8(body.get(..body.len().min(256))?)
        .ok()?
        .lines()
        .next()?;
    let mut parts = line.splitn(3, ' ');
    (parts.next()? == "SIP/2.0")
        .then(|| parts.next()?.parse().ok())
        .flatten()
        .filter(|s| (100..700).contains(s))
}

struct Actor {
    sink: Sink,
    media: SharedMedia,
    custom_ca: Arc<Mutex<TrustAnchors>>,
    internal: mpsc::Sender<Internal>,
    transfer_seq: u64,
    account: Option<Account>,
    registration: RegistrationState,
    generation: u64,
    call: Option<Call>,
    next_call: u64,
    dnd: bool,
    /// Dialogs whose BYE waits for proof of connectivity (bounded).
    pending_byes: Vec<InviteDialog>,
    /// Sequence of the only live refresh timer.
    refresh_seq: u64,
}

impl Actor {
    async fn run(
        &mut self,
        mut commands: mpsc::Receiver<Command>,
        mut internal: mpsc::Receiver<Internal>,
    ) {
        let mut flush = tokio::time::interval(Duration::from_millis(100));
        loop {
            tokio::select! {
                command = commands.recv() => match command {
                    None | Some(Command::Shutdown) => {
                        self.shutdown().await;
                        self.sink.emit(Event::Stopped);
                        self.sink.flush();
                        return;
                    }
                    Some(command) => self.command(command).await,
                },
                Some(message) = internal.recv() => self.internal(message).await,
                _ = flush.tick() => {
                    if !self.sink.backlog.is_empty() {
                        self.sink.flush();
                        (self.sink.wake)();
                    }
                }
            }
        }
    }

    fn set_registration(&mut self, state: RegistrationState) {
        if self.registration != state {
            self.registration = state;
            self.sink.emit(Event::Registration(state));
        }
    }

    fn publish(&mut self) {
        if let Some(call) = self.call.as_mut() {
            let snapshot = call.snapshot();
            let ended = snapshot.phase.is_final();
            self.sink.emit(Event::Call(snapshot));
            if ended
                && let Some(call) = self.call.take()
                && let Some((tx, task)) = call.media
            {
                drop(tx);
                task.abort();
            }
        }
    }

    fn end(&mut self, reason: EndReason) {
        if let Some(call) = self.call.as_mut() {
            call.phase = CallPhase::Ended(reason);
        }
        self.publish();
    }

    fn notice(&mut self, notice: Notice) {
        self.sink.emit(Event::Notice(notice));
    }

    fn call_mut(&mut self, id: CallId) -> Option<&mut Call> {
        self.call.as_mut().filter(|c| c.id == id)
    }

    async fn command(&mut self, command: Command) {
        match command {
            Command::Register(settings, password) => self.register(*settings, password).await,
            Command::Unregister => {
                if self.call.is_some() {
                    self.notice(Notice::InvalidState);
                } else {
                    self.unregister().await;
                }
            }
            Command::Dial(target) => self.dial(target).await,
            Command::Answer(id) => self.answer(id),
            Command::Reject(id) => {
                if let Some(call) = self.call_mut(id) {
                    if call.direction == Direction::Incoming && call.phase == CallPhase::Ringing {
                        if let Some(dialog) = &call.dialog {
                            let _ = dialog.reject(Some(StatusCode::BusyHere), None);
                        }
                        self.end(EndReason::Rejected);
                    }
                } else {
                    self.notice(Notice::UnknownCall);
                }
            }
            Command::Hangup(id) => self.hangup(id),
            Command::Dtmf(id, key) => match self.call_mut(id) {
                Some(call) if call.phase == CallPhase::Established => {
                    call.control(MediaControl::Dtmf(key))
                }
                Some(_) => self.notice(Notice::InvalidState),
                None => self.notice(Notice::UnknownCall),
            },
            Command::Hold(id) => self.hold(id, true),
            Command::Resume(id) => self.hold(id, false),
            Command::Transfer(id, target) => self.transfer(id, target),
            Command::SetMute(id, muted) => match self.call_mut(id) {
                Some(call) if !call.phase.is_final() => {
                    call.muted = muted;
                    call.control(MediaControl::Mute(muted));
                    self.publish();
                }
                Some(_) => self.notice(Notice::InvalidState),
                None => self.notice(Notice::UnknownCall),
            },
            Command::SetDnd(on) => self.dnd = on,
            Command::Shutdown => {}
        }
    }

    async fn register(&mut self, settings: AccountSettings, password: SecretString) {
        if self.call.is_some() {
            self.notice(Notice::InvalidState);
            return;
        }
        if settings.validate().is_err() {
            self.set_registration(RegistrationState::Failed(RegistrationFailure::Rejected(0)));
            return;
        }
        self.unregister().await;
        self.generation += 1;
        self.set_registration(RegistrationState::Registering);
        let trust = self
            .custom_ca
            .lock()
            .map(|t| t.clone())
            .unwrap_or(TrustAnchors::Unusable);
        let ca = match trust {
            TrustAnchors::System => None,
            TrustAnchors::Custom(pem) => Some(pem),
            TrustAnchors::Unusable if settings.transport == rust_microsip_core::Transport::Tls => {
                self.set_registration(RegistrationState::Failed(RegistrationFailure::Certificate));
                return;
            }
            TrustAnchors::Unusable => None,
        };
        match sip::Stack::start(settings, &password, ca, self.internal.clone()).await {
            Ok(stack) => {
                self.account = Some(Account {
                    stack,
                    registration: None,
                    attempts: 0,
                    in_flight: false,
                    fast_refreshed: false,
                });
                self.send_register(self.generation);
            }
            Err(failure) => self.set_registration(RegistrationState::Failed(failure)),
        }
    }

    fn send_register(&mut self, generation: u64) {
        let Some(account) = self.account.as_mut() else {
            return;
        };
        if account.in_flight {
            return;
        }
        let pair = match account.registration.take() {
            Some(existing) => match account.stack.registration() {
                Ok((_, registrar)) => Ok((existing, registrar)),
                Err(failure) => {
                    account.registration = Some(existing);
                    Err(failure)
                }
            },
            None => account.stack.registration(),
        };
        let (mut registration, registrar) = match pair {
            Ok(pair) => pair,
            Err(failure) => {
                // Never let the refresh chain die silently.
                self.set_registration(RegistrationState::Failed(failure));
                self.schedule(Duration::from_secs(30), generation);
                return;
            }
        };
        let expires = account.stack.settings.expires.clamp(60, 3600);
        account.in_flight = true;
        let internal = self.internal.clone();
        tokio::spawn(async move {
            let started = tokio::time::Instant::now();
            let result = tokio::time::timeout(
                Duration::from_secs(32),
                registration.register(registrar, Some(expires)),
            )
            .await;
            let outcome = match result {
                Err(_) => Err(RegistrationFailure::Timeout),
                Ok(Err(error)) => Err(classify_connect(&error)),
                Ok(Ok(response)) => match u16::from(response.status_code.clone()) {
                    200..=299 => {
                        let granted = response
                            .expires_header()
                            .and_then(|h| h.value().trim().parse::<u32>().ok())
                            .filter(|e| *e > 0)
                            .unwrap_or(expires);
                        // A slow answer came through retransmissions across an
                        // outage: the binding may have lapsed meanwhile.
                        let slow = started.elapsed() > Duration::from_secs(5);
                        Ok((Box::new(registration), granted, slow))
                    }
                    401 | 403 | 407 => Err(RegistrationFailure::Unauthorized),
                    408 => Err(RegistrationFailure::Timeout),
                    code => Err(RegistrationFailure::Rejected(code)),
                },
            };
            let _ = internal
                .send(Internal::Registered(outcome, generation))
                .await;
        });
    }

    async fn unregister(&mut self) {
        // Deferred BYEs must leave before the stack goes away (bounded).
        if !self.pending_byes.is_empty() {
            let mut byes = tokio::task::JoinSet::new();
            for dialog in self.pending_byes.drain(..) {
                byes.spawn(async move {
                    let _ = dialog.hangup().await;
                });
            }
            let _ = tokio::time::timeout(Duration::from_secs(1), async {
                while byes.join_next().await.is_some() {}
            })
            .await;
        }
        if let Some(mut account) = self.account.take() {
            if let (Some(mut registration), Ok((_, registrar))) =
                (account.registration.take(), account.stack.registration())
            {
                // Budget: hangup 1 s + BYEs 1 s + this 1.5 s stays below the
                // application's 4 s shutdown wait.
                let _ = tokio::time::timeout(
                    Duration::from_millis(1500),
                    registration.register(registrar, Some(0)),
                )
                .await;
            }
            account.stack.endpoint.shutdown();
            account.stack.stop.cancel();
        }
        self.generation += 1;
        self.set_registration(RegistrationState::Disabled);
    }

    /// (Re)arm the single refresh timer; earlier timers become stale.
    fn schedule(&mut self, delay: Duration, generation: u64) {
        self.refresh_seq += 1;
        let seq = self.refresh_seq;
        let internal = self.internal.clone();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = internal
                .send(Internal::RefreshRegistration(generation, seq))
                .await;
        });
    }

    async fn dial(&mut self, target: DialTarget) {
        if self.call.is_some() {
            self.notice(Notice::Busy);
            return;
        }
        let (Some(account), RegistrationState::Registered) = (&self.account, self.registration)
        else {
            self.notice(Notice::NotRegistered);
            return;
        };
        let stack = &account.stack;
        let secure = stack.settings.media == MediaSecurity::Srtp;
        let Ok(socket) = UdpSocket::bind(SocketAddr::new(stack.local_ip, 0)).await else {
            self.notice(Notice::InvalidState);
            return;
        };
        let local_key = if secure {
            match LocalKey::generate() {
                Ok(key) => Some(key),
                Err(_) => {
                    self.notice(Notice::InvalidState);
                    return;
                }
            }
        } else {
            None
        };
        let id = CallId(self.next_call);
        self.next_call += 1;
        let mut call = Call {
            id,
            direction: Direction::Outgoing,
            remote: target.as_str().to_owned(),
            phase: CallPhase::Dialing,
            muted: false,
            secure,
            transfer: None,
            version: 0,
            dialog: None,
            dialog_id: None,
            cancel_requested: false,
            socket: Arc::new(socket),
            local_key,
            crypto_tag: 1,
            sdp_version: 1,
            codec: Codec::Pcmu,
            event_pt: None,
            remote_media: None,
            remote_key_line: Zeroizing::new(String::new()),
            local_flow: Flow::SendRecv,
            remote_flow: Flow::SendRecv,
            offer: None,
            negotiated: false,
            reinvite_pending: false,
            media: None,
            _handle: None,
        };
        let offer = call.describe(Flow::SendRecv);
        let (Some(caller), Some(callee)) = (
            stack.uri(&stack.settings.username),
            stack.uri(target.as_str()),
        ) else {
            self.notice(Notice::InvalidState);
            return;
        };
        let option = InviteOption {
            caller_display_name: (!stack.settings.display_name.is_empty())
                .then(|| stack.settings.display_name.clone()),
            caller,
            callee,
            contact: stack.contact.clone(),
            destination: Some(stack.destination.clone()),
            credential: Some(stack.credential.clone()),
            content_type: Some("application/sdp".into()),
            offer: Some(offer.to_vec()),
            ..Default::default()
        };
        let layer = stack.layer.clone();
        let states = stack.states.clone();
        let internal = self.internal.clone();
        call._handle = Some(tokio::spawn(async move {
            let result = layer.do_invite(option, states).await.map_err(|_| ());
            let _ = internal.send(Internal::Outgoing(id, result)).await;
        }));
        self.call = Some(call);
        self.publish();
    }

    fn start_media(&mut self, remote: RemoteDescription) {
        let media = self.media.clone();
        let Some(call) = self.call.as_mut() else {
            return;
        };
        let remote_key = remote.crypto.map(|(_, key)| key);
        let local_key = call.local_key.as_ref().and_then(|k| k.master().ok());
        let (tx, rx) = mpsc::channel(16);
        let send = call.local_flow.sends() && remote.flow.receives();
        let receive = call.local_flow.receives() && remote.flow.sends();
        let params = MediaParams {
            socket: call.socket.clone(),
            remote: remote.rtp,
            codec: call.codec,
            event_pt: call.event_pt,
            local_key,
            remote_key,
            send,
            receive,
        };
        let internal = self.internal.clone();
        let id = call.id;
        let task = tokio::spawn(async move {
            let stats = media::run(params, media, rx).await;
            if let Some(failure) = stats.failure {
                let _ = internal.send(Internal::MediaFailed(id, failure)).await;
            }
            stats
        });
        if call.muted {
            let _ = tx.try_send(MediaControl::Mute(true));
        }
        call.remote_media = Some(remote.rtp);
        call.remote_flow = remote.flow;
        call.media = Some((tx, task));
    }

    fn answer(&mut self, id: CallId) {
        let Some(call) = self.call_mut(id) else {
            self.notice(Notice::UnknownCall);
            return;
        };
        if call.direction != Direction::Incoming || call.phase != CallPhase::Ringing {
            self.notice(Notice::InvalidState);
            return;
        }
        let Some(offer) = call.offer.take() else {
            return;
        };
        let answer = call.describe(offer.flow.answer());
        let accepted = call
            .dialog
            .as_ref()
            .is_some_and(|d| d.accept(sdp_headers(), Some(answer.to_vec())).is_ok());
        if !accepted {
            if let Some(dialog) = &call.dialog {
                let _ = dialog.reject(Some(StatusCode::ServerInternalError), None);
            }
            self.end(EndReason::Failed);
            return;
        }
        call.local_flow = Flow::SendRecv;
        call.phase = CallPhase::Established;
        call.remote_key_line = Zeroizing::new(offer.key_line.to_string());
        self.start_media(offer);
        self.publish();
    }

    fn hangup(&mut self, id: CallId) {
        let Some(call) = self.call_mut(id) else {
            self.notice(Notice::UnknownCall);
            return;
        };
        match (call.direction, call.phase) {
            (_, CallPhase::Ended(_) | CallPhase::Terminating) => {}
            (Direction::Incoming, CallPhase::Ringing) => {
                if let Some(dialog) = &call.dialog {
                    let _ = dialog.reject(Some(StatusCode::BusyHere), None);
                }
                self.end(EndReason::Rejected);
            }
            (Direction::Outgoing, CallPhase::Dialing | CallPhase::Ringing) => {
                call.cancel_requested = true;
                call.phase = CallPhase::Terminating;
                if let Some(dialog) = call.dialog.clone() {
                    tokio::spawn(async move {
                        let _ = dialog.cancel().await;
                    });
                }
                // The final response normally ends the call; never wait forever.
                let internal = self.internal.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(32)).await;
                    let _ = internal.send(Internal::CancelDeadline(id)).await;
                });
                self.publish();
            }
            _ => {
                call.phase = CallPhase::Terminating;
                if let Some(dialog) = call.dialog.clone() {
                    tokio::spawn(async move {
                        let _ = tokio::time::timeout(Duration::from_secs(5), dialog.hangup()).await;
                    });
                }
                self.end(EndReason::Normal);
            }
        }
    }

    fn hold(&mut self, id: CallId, hold: bool) {
        let internal = self.internal.clone();
        let Some(call) = self.call_mut(id) else {
            self.notice(Notice::UnknownCall);
            return;
        };
        let expected = if hold {
            CallPhase::Established
        } else {
            CallPhase::Held
        };
        if call.phase != expected || call.reinvite_pending {
            self.notice(Notice::InvalidState);
            return;
        }
        call.reinvite_pending = true;
        let flow = if hold { Flow::SendOnly } else { Flow::SendRecv };
        call.sdp_version += 1;
        let body = call.describe(flow);
        let Some(dialog) = call.dialog.clone() else {
            return;
        };
        tokio::spawn(async move {
            let response = dialog
                .reinvite(sdp_headers(), Some(body.to_vec()))
                .await
                .ok()
                .flatten();
            let _ = internal.send(Internal::Reinvite(id, flow, response)).await;
        });
    }

    fn transfer(&mut self, id: CallId, target: DialTarget) {
        let internal = self.internal.clone();
        let domain = self
            .account
            .as_ref()
            .and_then(|a| a.stack.uri(target.as_str()));
        let Some(call) = self.call_mut(id) else {
            self.notice(Notice::UnknownCall);
            return;
        };
        if !matches!(call.phase, CallPhase::Established | CallPhase::Held)
            || call
                .transfer
                .as_ref()
                .is_some_and(|t| t.state == TransferState::Pending)
        {
            self.notice(Notice::InvalidState);
            return;
        }
        let (Some(dialog), Some(uri)) = (call.dialog.clone(), domain) else {
            return;
        };
        call.transfer = Some(Transfer {
            state: TransferState::Pending,
        });
        let refer_to = format!("<{uri}>");
        self.transfer_seq += 1;
        let seq = self.transfer_seq;
        tokio::spawn(async move {
            let status = dialog
                .refer(refer_to, None, None)
                .await
                .ok()
                .flatten()
                .map(|r| u16::from(r.status_code));
            let _ = internal.send(Internal::Referred(id, status)).await;
            // No final NOTIFY within this time: report the transfer as failed.
            tokio::time::sleep(Duration::from_secs(30)).await;
            let _ = internal.send(Internal::TransferDeadline(id, seq)).await;
        });
        self.publish();
    }

    async fn shutdown(&mut self) {
        if let Some(call) = self.call.as_ref() {
            let id = call.id;
            if let Some(dialog) = call.dialog.clone() {
                if call.direction == Direction::Incoming && call.phase == CallPhase::Ringing {
                    let _ = dialog.reject(Some(StatusCode::BusyHere), None);
                } else {
                    let _ = tokio::time::timeout(Duration::from_secs(1), dialog.hangup()).await;
                }
            }
            if self.call.as_ref().is_some_and(|c| c.id == id) {
                self.end(EndReason::Normal);
            }
        }
        self.unregister().await;
    }

    async fn internal(&mut self, message: Internal) {
        match message {
            Internal::Incoming(tx) => self.incoming(*tx).await,
            Internal::Dialog(state) => self.dialog_state(*state).await,
            Internal::Registered(result, generation) => self.registered(result, generation),
            Internal::RefreshRegistration(generation, seq) => {
                if generation == self.generation
                    && seq == self.refresh_seq
                    && self.account.is_some()
                {
                    if self.registration != RegistrationState::Registered {
                        self.set_registration(RegistrationState::Registering);
                    }
                    self.send_register(generation);
                }
            }
            Internal::Outgoing(id, result) => self.outgoing(id, result),
            Internal::Reinvite(id, flow, response) => self.reinvited(id, flow, response),
            Internal::MediaFailed(id, failure) => {
                if let Some(call) = self.call_mut(id)
                    && !call.phase.is_final()
                {
                    let dialog = call.dialog.clone();
                    self.end(EndReason::Failed);
                    match (dialog, failure) {
                        // Silence usually means the network is gone, and a BYE
                        // sent now would be lost (the SDK marks the dialog
                        // ended either way). Probe the registrar first and
                        // send the BYE once it answers.
                        (Some(dialog), media::MediaFailure::Inactivity) => {
                            if self.pending_byes.len() >= 8 {
                                self.pending_byes.remove(0);
                            }
                            self.pending_byes.push(dialog);
                            let generation = self.generation;
                            self.send_register(generation);
                        }
                        (Some(dialog), media::MediaFailure::Crypto) => {
                            tokio::spawn(async move {
                                let _ =
                                    tokio::time::timeout(Duration::from_secs(5), dialog.hangup())
                                        .await;
                            });
                        }
                        (None, _) => {}
                    }
                }
            }
            Internal::CancelDeadline(id) => {
                if self
                    .call_mut(id)
                    .is_some_and(|c| c.phase == CallPhase::Terminating)
                {
                    self.end(EndReason::Canceled);
                }
            }
            Internal::TransferDeadline(id, seq) => {
                if seq == self.transfer_seq
                    && let Some(call) = self.call_mut(id)
                    && call
                        .transfer
                        .as_ref()
                        .is_some_and(|t| t.state == TransferState::Pending)
                {
                    call.transfer = Some(Transfer {
                        state: TransferState::Failed(408),
                    });
                    self.publish();
                }
            }
            Internal::Referred(id, status) => {
                if let Some(call) = self.call_mut(id) {
                    if !status.is_some_and(|s| (200..300).contains(&s)) {
                        call.transfer = Some(Transfer {
                            state: TransferState::Failed(status.unwrap_or(408)),
                        });
                    }
                    self.publish();
                }
            }
        }
    }

    fn registered(
        &mut self,
        result: Result<
            (
                Box<rsipstack::dialog::registration::Registration>,
                u32,
                bool,
            ),
            RegistrationFailure,
        >,
        generation: u64,
    ) {
        if generation != self.generation {
            return;
        }
        let Some(account) = self.account.as_mut() else {
            return;
        };
        account.in_flight = false;
        match result {
            Ok((registration, granted, slow)) => {
                // The registrar answered: deferred BYEs can get through now.
                for dialog in self.pending_byes.drain(..) {
                    tokio::spawn(async move {
                        let _ = tokio::time::timeout(Duration::from_secs(5), dialog.hangup()).await;
                    });
                }
                account.registration = Some(*registration);
                account.attempts = 0;
                // Only one fast retry per slow answer: a consistently slow
                // path must not turn into a REGISTER every few seconds.
                let fast = slow && !account.fast_refreshed;
                account.fast_refreshed = fast;
                let refresh = if fast {
                    // Re-register promptly to replace a binding that may have
                    // expired while the network was down.
                    Duration::from_secs(2)
                } else {
                    Duration::from_secs(u64::from(granted.max(60)) * 4 / 5)
                };
                self.set_registration(RegistrationState::Registered);
                self.schedule(refresh, generation);
            }
            Err(failure) => {
                account.attempts += 1;
                let attempts = account.attempts;
                self.set_registration(RegistrationState::Failed(failure));
                // Credentials errors and exhausted attempts wait for the user.
                if failure != RegistrationFailure::Unauthorized
                    && attempts < MAX_REGISTRATION_ATTEMPTS
                {
                    let delay =
                        Duration::from_secs(5u64 << attempts.min(6)).min(Duration::from_secs(300));
                    self.schedule(delay, generation);
                }
            }
        }
    }

    fn outgoing(&mut self, id: CallId, result: Result<(InviteDialog, Option<Response>), ()>) {
        let policy = self.policy();
        let Some(call) = self.call_mut(id) else {
            return;
        };
        let (dialog, response) = match result {
            Ok(pair) => pair,
            Err(()) => {
                let reason = if call.cancel_requested {
                    EndReason::Canceled
                } else {
                    EndReason::Failed
                };
                self.end(reason);
                return;
            }
        };
        let status = response.as_ref().map(|r| u16::from(r.status_code.clone()));
        if !status.is_some_and(|s| (200..300).contains(&s)) {
            let reason = if call.cancel_requested {
                EndReason::Canceled
            } else {
                end_reason(status)
            };
            self.end(reason);
            return;
        }
        call.dialog_id = Some(dialog.id());
        call.dialog = Some(dialog.clone());
        if call.cancel_requested {
            tokio::spawn(async move {
                let _ = dialog.hangup().await;
            });
            self.end(EndReason::Canceled);
            return;
        }
        let body = response.map(|r| r.body).unwrap_or_default();
        match sdp::parse(&body, policy).and_then(|remote| {
            let codec = remote.codec()?;
            if !OFFER_CODECS.contains(&codec) {
                return Err(SdpError::NoCodec);
            }
            Ok((remote, codec))
        }) {
            Ok((remote, codec)) => {
                call.codec = codec;
                call.event_pt = remote.event_pt;
                call.negotiated = true;
                call.remote_key_line = Zeroizing::new(remote.key_line.to_string());
                call.phase = CallPhase::Established;
                self.start_media(remote);
                self.publish();
            }
            Err(_) => {
                tokio::spawn(async move {
                    let _ = dialog.hangup().await;
                });
                self.end(EndReason::NotAcceptable);
            }
        }
    }

    fn reinvited(&mut self, id: CallId, flow: Flow, response: Option<Response>) {
        let policy = self.policy();
        let Some(call) = self.call_mut(id) else {
            return;
        };
        call.reinvite_pending = false;
        let ok = response
            .as_ref()
            .is_some_and(|r| r.status_code == StatusCode::OK);
        let parsed = response
            .filter(|_| ok)
            .and_then(|r| sdp::parse(&r.body, policy).ok());
        match parsed {
            Some(remote) if !remote.codecs.contains(&call.codec) => {
                // The peer switched codec in its answer; keep the call as it was.
                self.notice(Notice::InvalidState);
            }
            Some(remote) if *remote.key_line == *call.remote_key_line => {
                call.local_flow = flow;
                call.remote_flow = remote.flow;
                call.phase = if flow == Flow::SendRecv {
                    CallPhase::Established
                } else {
                    CallPhase::Held
                };
                let held = flow != Flow::SendRecv;
                call.control(MediaControl::Remote(remote.rtp, None));
                call.control(MediaControl::Flow {
                    send: !held && remote.flow.receives(),
                    receive: !held && remote.flow.sends(),
                });
                self.publish();
            }
            Some(remote) => {
                // Peer changed its key in an answer: open the new epoch.
                call.local_flow = flow;
                call.remote_key_line = Zeroizing::new(remote.key_line.to_string());
                let key = remote.crypto.map(|(_, k)| k);
                call.control(MediaControl::Remote(remote.rtp, key));
                let held = flow != Flow::SendRecv;
                call.remote_flow = remote.flow;
                call.control(MediaControl::Flow {
                    send: !held && remote.flow.receives(),
                    receive: !held && remote.flow.sends(),
                });
                call.phase = if held {
                    CallPhase::Held
                } else {
                    CallPhase::Established
                };
                self.publish();
            }
            None => self.notice(Notice::InvalidState),
        }
    }

    fn policy(&self) -> MediaSecurity {
        self.account
            .as_ref()
            .map_or(MediaSecurity::PlainRtp, |a| a.stack.settings.media)
    }

    async fn incoming(&mut self, mut tx: Transaction) {
        let Some(account) = &self.account else {
            let _ = tx.reply(StatusCode::TemporarilyUnavailable).await;
            return;
        };
        if self.call.is_some() || self.dnd {
            let _ = tx.reply(StatusCode::BusyHere).await;
            return;
        }
        let policy = account.stack.settings.media;
        let offer = match sdp::parse(&tx.original.body, policy) {
            Ok(offer) if offer.codec().is_ok() => offer,
            _ => {
                let _ = tx.reply(StatusCode::NotAcceptableHere).await;
                return;
            }
        };
        let Ok(socket) = UdpSocket::bind(SocketAddr::new(account.stack.local_ip, 0)).await else {
            let _ = tx.reply(StatusCode::ServerInternalError).await;
            return;
        };
        let local_key = if offer.crypto.is_some() {
            match LocalKey::generate() {
                Ok(key) => Some(key),
                Err(_) => {
                    let _ = tx.reply(StatusCode::ServerInternalError).await;
                    return;
                }
            }
        } else {
            None
        };
        let dialog = match account.stack.layer.get_or_create_server_invite(
            &tx,
            account.stack.states.clone(),
            Some(account.stack.credential.clone()),
            Some(account.stack.contact.clone()),
        ) {
            Ok(dialog) => dialog,
            Err(_) => {
                let _ = tx.reply(StatusCode::ServerInternalError).await;
                return;
            }
        };
        let _ = dialog.ringing(None, None);
        let remote = user_part(&tx.original);
        let mut handler = dialog.clone();
        let handle = tokio::spawn(async move {
            let _ = handler.handle(&mut tx).await;
        });
        let id = CallId(self.next_call);
        self.next_call += 1;
        let codec = offer.codec().unwrap_or(Codec::Pcmu);
        self.call = Some(Call {
            id,
            direction: Direction::Incoming,
            remote,
            phase: CallPhase::Ringing,
            muted: false,
            secure: offer.crypto.is_some(),
            transfer: None,
            version: 0,
            dialog_id: Some(dialog.id()),
            dialog: Some(dialog),
            cancel_requested: false,
            socket: Arc::new(socket),
            local_key,
            crypto_tag: offer.crypto.as_ref().map_or(1, |c| c.0),
            sdp_version: 1,
            codec,
            event_pt: offer.event_pt,
            remote_media: None,
            remote_key_line: Zeroizing::new(String::new()),
            local_flow: Flow::SendRecv,
            remote_flow: offer.flow,
            offer: Some(offer),
            negotiated: true,
            reinvite_pending: false,
            media: None,
            _handle: Some(handle),
        });
        self.publish();
    }

    async fn dialog_state(&mut self, state: DialogState) {
        let Some(id) = sip::dialog_id(&state).cloned() else {
            return;
        };
        let policy = self.policy();
        let Some(call) = self.call.as_mut() else {
            // Late events for finished calls: answer requests, ignore the rest.
            reply_stale(state).await;
            return;
        };
        // Before the dialog is known only its creation events may claim the
        // call; afterwards Call-ID and our tag must both match.
        let ours = match &call.dialog_id {
            None => matches!(state, DialogState::Calling(_) | DialogState::Trying(_)),
            Some(known) => known.call_id == id.call_id && known.local_tag == id.local_tag,
        };
        if !ours {
            reply_stale(state).await;
            return;
        }
        match state {
            DialogState::Calling(id) | DialogState::Trying(id) => {
                if call.dialog_id.is_none() {
                    call.dialog_id = Some(id.clone());
                }
                if call.dialog.is_none()
                    && let Some(account) = &self.account
                    && let Some(Dialog::Invite(dialog)) = account.stack.layer.get_dialog(&id)
                {
                    call.dialog = Some(dialog.clone());
                    if call.cancel_requested {
                        tokio::spawn(async move {
                            let _ = dialog.cancel().await;
                        });
                    }
                }
            }
            DialogState::Early(..) => {
                if call.direction == Direction::Outgoing && call.phase == CallPhase::Dialing {
                    call.phase = CallPhase::Ringing;
                    self.publish();
                }
            }
            DialogState::Updated(_, request, handle) => {
                if request.method != Method::Invite && request.method != Method::Update {
                    let _ = handle.reply(StatusCode::OK).await;
                    return;
                }
                if request.body.is_empty() {
                    // Session refresh or delayed offer: keep the session as is.
                    if request.method == Method::Invite {
                        let body = call.describe(call.local_flow);
                        let _ = handle
                            .respond(StatusCode::OK, sdp_headers(), Some(body.to_vec()))
                            .await;
                    } else {
                        let _ = handle.reply(StatusCode::OK).await;
                    }
                    return;
                }
                match sdp::parse(&request.body, policy) {
                    Ok(mut offer) if offer.codecs.contains(&call.codec) => {
                        let rekeyed = *offer.key_line != *call.remote_key_line;
                        if let Some((tag, _)) = &offer.crypto {
                            call.crypto_tag = *tag;
                        }
                        let key = offer.crypto.take().filter(|_| rekeyed).map(|(_, k)| k);
                        call.sdp_version += 1;
                        // While locally held we only ever send (or nothing).
                        let answer_flow = match (call.local_flow, offer.flow.receives()) {
                            (Flow::SendOnly, true) => Flow::SendOnly,
                            (Flow::SendOnly, false) => Flow::Inactive,
                            _ => offer.flow.answer(),
                        };
                        let body = call.describe(answer_flow);
                        if handle
                            .respond(StatusCode::OK, sdp_headers(), Some(body.to_vec()))
                            .await
                            .is_ok()
                        {
                            call.remote_key_line = Zeroizing::new(offer.key_line.to_string());
                            call.control(MediaControl::Remote(offer.rtp, key));
                            let local_held = call.local_flow != Flow::SendRecv;
                            call.control(MediaControl::Flow {
                                send: !local_held && offer.flow.receives(),
                                receive: !local_held && offer.flow.sends(),
                            });
                            call.remote_flow = offer.flow;
                        }
                    }
                    _ => {
                        let _ = handle.reply(StatusCode::NotAcceptableHere).await;
                    }
                }
            }
            DialogState::Notify(_, request, handle) => {
                let pending = call
                    .transfer
                    .as_ref()
                    .is_some_and(|t| t.state == TransferState::Pending);
                let event_ok = request
                    .headers
                    .iter()
                    .any(|h| matches!(h, Header::Event(v) if v.value().trim().to_ascii_lowercase().starts_with("refer")));
                let status = sipfrag(&request.body);
                let _ = handle
                    .reply(if pending && event_ok && status.is_some() {
                        StatusCode::OK
                    } else {
                        StatusCode::CallTransactionDoesNotExist
                    })
                    .await;
                let terminal = request.headers.iter().any(|h| {
                    matches!(h, Header::SubscriptionState(v) if v.value().trim().to_ascii_lowercase().starts_with("terminated"))
                });
                if !(pending && event_ok) {
                    return;
                }
                match status {
                    Some(200..=299) => {
                        call.transfer = Some(Transfer {
                            state: TransferState::Succeeded,
                        });
                        if let Some(dialog) = call.dialog.clone() {
                            tokio::spawn(async move {
                                let _ =
                                    tokio::time::timeout(Duration::from_secs(5), dialog.hangup())
                                        .await;
                            });
                        }
                        self.end(EndReason::Transferred);
                    }
                    Some(code) if terminal && code >= 300 => {
                        call.transfer = Some(Transfer {
                            state: TransferState::Failed(code),
                        });
                        self.publish();
                    }
                    _ => {}
                }
            }
            DialogState::Refer(_, _, handle) => {
                let _ = handle.reply(StatusCode::Decline).await;
            }
            DialogState::Info(_, _, handle)
            | DialogState::Options(_, _, handle)
            | DialogState::Message(_, _, handle) => {
                let _ = handle.reply(StatusCode::OK).await;
            }
            DialogState::Terminated(..) => {
                let reason = match (call.direction, call.phase) {
                    (Direction::Incoming, CallPhase::Ringing) => Some(EndReason::Canceled),
                    (_, CallPhase::Established | CallPhase::Held) => Some(EndReason::Normal),
                    _ => None,
                };
                if let Some(reason) = reason {
                    self.end(reason);
                }
            }
            _ => {}
        }
    }
}

async fn reply_stale(state: DialogState) {
    match state {
        DialogState::Updated(_, _, handle)
        | DialogState::Notify(_, _, handle)
        | DialogState::Refer(_, _, handle)
        | DialogState::Info(_, _, handle)
        | DialogState::Message(_, _, handle)
        | DialogState::Options(_, _, handle)
        | DialogState::Publish(_, _, handle) => {
            let _ = handle.reply(StatusCode::CallTransactionDoesNotExist).await;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn call(
        negotiated: bool,
        codec: Codec,
        event_pt: Option<u8>,
    ) -> Result<Call, std::io::Error> {
        Ok(Call {
            id: CallId(1),
            direction: Direction::Incoming,
            remote: String::new(),
            phase: CallPhase::Ringing,
            muted: false,
            secure: false,
            transfer: None,
            version: 0,
            dialog: None,
            dialog_id: None,
            cancel_requested: false,
            socket: Arc::new(UdpSocket::bind("127.0.0.1:0").await?),
            local_key: None,
            crypto_tag: 1,
            sdp_version: 1,
            codec,
            event_pt,
            remote_media: None,
            remote_key_line: Zeroizing::new(String::new()),
            local_flow: Flow::SendRecv,
            remote_flow: Flow::SendRecv,
            offer: None,
            negotiated,
            reinvite_pending: false,
            media: None,
            _handle: None,
        })
    }

    #[tokio::test]
    async fn answers_carry_only_negotiated_codec_and_offered_events()
    -> Result<(), Box<dyn std::error::Error>> {
        // A PCMA-first offer without telephone-event.
        let answer = call(true, Codec::Pcma, None)
            .await?
            .describe(Flow::SendRecv);
        let parsed = sdp::parse(&answer, MediaSecurity::PlainRtp).map_err(|_| "parse")?;
        assert_eq!(parsed.codecs, vec![Codec::Pcma]);
        assert_eq!(parsed.event_pt, None);
        let answer = call(true, Codec::Pcmu, Some(96))
            .await?
            .describe(Flow::RecvOnly);
        let parsed = sdp::parse(&answer, MediaSecurity::PlainRtp).map_err(|_| "parse")?;
        assert_eq!(
            (parsed.codecs, parsed.event_pt, parsed.flow),
            (vec![Codec::Pcmu], Some(96), Flow::RecvOnly)
        );
        // The initial offer lists every supported codec and our event type.
        let offer = call(false, Codec::Pcmu, None)
            .await?
            .describe(Flow::SendRecv);
        let parsed = sdp::parse(&offer, MediaSecurity::PlainRtp).map_err(|_| "parse")?;
        assert_eq!(
            (parsed.codecs, parsed.event_pt),
            (OFFER_CODECS.to_vec(), Some(sdp::EVENT_PT))
        );
        Ok(())
    }

    #[test]
    fn caller_identity_drops_invisible_formatting() {
        assert!(is_format_char('\u{202E}') && is_format_char('\u{200B}') && !is_format_char('a'));
    }

    #[test]
    fn maps_final_statuses_and_sipfrag() {
        assert_eq!(end_reason(Some(486)), EndReason::Busy);
        assert_eq!(end_reason(Some(487)), EndReason::Canceled);
        assert_eq!(end_reason(Some(480)), EndReason::NoAnswer);
        assert_eq!(end_reason(Some(488)), EndReason::NotAcceptable);
        assert_eq!(end_reason(None), EndReason::Failed);
        assert_eq!(sipfrag(b"SIP/2.0 200 OK\r\n"), Some(200));
        assert_eq!(sipfrag(b"SIP/2.0 2000 OK\r\n"), None);
        assert_eq!(sipfrag(b"HTTP/1.1 200 OK\r\n"), None);
    }

    #[test]
    fn sink_keeps_snapshots_and_sheds_notices_under_pressure() {
        let (tx, mut rx) = mpsc::channel(1);
        let mut sink = Sink {
            tx,
            backlog: VecDeque::new(),
            wake: Arc::new(|| {}),
        };
        sink.emit(Event::Registration(RegistrationState::Registered));
        for _ in 0..BACKLOG_LIMIT {
            sink.emit(Event::Notice(Notice::Overloaded));
        }
        sink.emit(Event::Stopped);
        assert!(sink.backlog.len() <= BACKLOG_LIMIT);
        assert!(sink.backlog.back().is_some_and(|e| *e == Event::Stopped));
        assert_eq!(
            rx.try_recv().ok(),
            Some(Event::Registration(RegistrationState::Registered))
        );
    }

    #[test]
    fn engine_starts_reports_not_registered_and_stops() -> Result<(), Box<dyn std::error::Error>> {
        let handle = EngineHandle::spawn(Box::new(rust_microsip_core::NullMedia), Arc::new(|| {}))?;
        handle
            .send(Command::Dial(DialTarget::parse("1002")?))
            .map_err(|_| "send")?;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut seen = Vec::new();
        while std::time::Instant::now() < deadline
            && !seen.contains(&Event::Notice(Notice::NotRegistered))
        {
            seen.extend(handle.try_events());
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(seen.contains(&Event::Notice(Notice::NotRegistered)));
        assert!(handle.shutdown(Duration::from_secs(5)));
        Ok(())
    }
}
