//! Incoming-call desktop notifications over `org.freedesktop.Notifications` (LIN-01).
//!
//! All D-Bus work runs on a background thread with zbus's own `async-io`
//! executor (never Tokio). Callers only enqueue into a bounded queue, so they
//! never block; when there is no session bus or notification service the
//! notifier reports [`NotifierStatus::Unavailable`] and the window remains the
//! only way to answer.

use crate::Waker;
use crate::sanitize_caller;
use rust_microsip_core::CallId;
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;
use zbus::message::Type as MessageType;
use zbus::zvariant::Value;
use zbus::{Connection, MatchRule, MessageStream};

const DEST: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const QUEUE: usize = 8;
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// Notification lifetime in milliseconds; it is also closed explicitly.
const EXPIRE_MS: i32 = 60_000;

/// Whether desktop notifications can currently be shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifierStatus {
    /// Still connecting to the session bus.
    Connecting,
    /// Connected; `actions` tells whether answer/reject buttons are supported.
    Available {
        /// The server advertises the `actions` capability.
        actions: bool,
    },
    /// No session bus or notification service; use the window only.
    Unavailable,
}

/// Outcome of a non-blocking notification request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Queued for the background thread.
    Queued,
    /// The queue is full; the request was dropped.
    QueueFull,
    /// Notifications are unavailable; nothing was sent.
    Unavailable,
}

/// Button pressed on an incoming-call notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallAction {
    /// The user chose "Answer".
    Answer,
    /// The user chose "Reject".
    Reject,
}

/// Action invoked by the user on the notification for `call`.
///
/// It is a request only; the UI must check the call is still ringing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotificationAction {
    /// Call the notification was shown for.
    pub call: CallId,
    /// Chosen action.
    pub action: CallAction,
}

enum Job {
    Show(CallId, String),
    Close(CallId),
}

type Shared<T> = Arc<Mutex<T>>;
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Fire-and-forget incoming-call notifier. Dropping it closes open notifications.
pub struct Notifier {
    jobs: SyncSender<Job>,
    status: Shared<NotifierStatus>,
    actions: Receiver<NotificationAction>,
    waker: Shared<Option<Waker>>,
}
impl std::fmt::Debug for Notifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Notifier")
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}
impl Notifier {
    /// Connect to the user's session bus (`DBUS_SESSION_BUS_ADDRESS`) in the background.
    pub fn session() -> Self {
        Self::spawn(None)
    }

    /// Connect to an explicit D-Bus address (for tests or sandboxed buses).
    pub fn with_address(address: &str) -> Self {
        Self::spawn(Some(address.to_owned()))
    }

    fn spawn(address: Option<String>) -> Self {
        let (jobs, rx) = mpsc::sync_channel(QUEUE);
        let (action_tx, actions) = mpsc::sync_channel(QUEUE);
        let status = Arc::new(Mutex::new(NotifierStatus::Connecting));
        let waker: Shared<Option<Waker>> = Arc::default();
        let (st, wk) = (Arc::clone(&status), Arc::clone(&waker));
        let spawned = thread::Builder::new()
            .name("notifier".into())
            .spawn(move || worker(address, &rx, &st, &action_tx, &wk));
        if spawned.is_err() {
            *lock(&status) = NotifierStatus::Unavailable;
        }
        Self {
            jobs,
            status,
            actions,
            waker,
        }
    }

    /// Current availability.
    pub fn status(&self) -> NotifierStatus {
        *lock(&self.status)
    }

    /// Run `waker` after each queued [`NotificationAction`] (replaces any previous one).
    pub fn set_waker(&self, waker: Waker) {
        *lock(&self.waker) = Some(waker);
    }

    /// Show "Incoming call" for `call` with a sanitized `caller` body.
    pub fn incoming_call(&self, call: CallId, caller: &str) -> Delivery {
        self.send(Job::Show(call, sanitize_caller(caller)))
    }

    /// Close the notification for `call` (answered, rejected or ended).
    pub fn close(&self, call: CallId) -> Delivery {
        self.send(Job::Close(call))
    }

    /// Next action pressed by the user, without blocking.
    pub fn try_recv_action(&self) -> Option<NotificationAction> {
        self.actions.try_recv().ok()
    }

    fn send(&self, job: Job) -> Delivery {
        if self.status() == NotifierStatus::Unavailable {
            return Delivery::Unavailable;
        }
        match self.jobs.try_send(job) {
            Ok(()) => Delivery::Queued,
            Err(TrySendError::Full(_)) => Delivery::QueueFull,
            Err(TrySendError::Disconnected(_)) => Delivery::Unavailable,
        }
    }
}

fn worker(
    address: Option<String>,
    jobs: &Receiver<Job>,
    status: &Mutex<NotifierStatus>,
    action_tx: &SyncSender<NotificationAction>,
    waker: &Shared<Option<Waker>>,
) {
    let Ok((conn, actions)) = zbus::block_on(connect(address)) else {
        // Dropping `jobs` on return makes later sends report Unavailable.
        *lock(status) = NotifierStatus::Unavailable;
        return;
    };
    let ids: Shared<HashMap<CallId, u32>> = Arc::default();
    let actions = actions && {
        let (conn, ids, tx, waker) = (
            conn.clone(),
            Arc::clone(&ids),
            action_tx.clone(),
            Arc::clone(waker),
        );
        thread::Builder::new()
            .name("notifier-actions".into())
            .spawn(move || zbus::block_on(listen(&conn, &ids, &tx, &waker)))
            .is_ok()
    };
    *lock(status) = NotifierStatus::Available { actions };
    for job in jobs {
        match job {
            Job::Show(call, body) => {
                if let Ok(id) = zbus::block_on(notify(&conn, &body, actions)) {
                    lock(&ids).insert(call, id);
                }
            }
            Job::Close(call) => {
                let id = lock(&ids).remove(&call);
                if let Some(id) = id {
                    let _ = zbus::block_on(close(&conn, id));
                }
            }
        }
    }
    let open: Vec<u32> = lock(&ids).drain().map(|(_, id)| id).collect();
    for id in open {
        let _ = zbus::block_on(close(&conn, id));
    }
}

/// Connect and probe the service; returns whether it supports actions.
async fn connect(address: Option<String>) -> zbus::Result<(Connection, bool)> {
    let builder = match address {
        Some(address) => zbus::connection::Builder::address(address.as_str())?,
        None => zbus::connection::Builder::session()?,
    };
    let conn = builder.method_timeout(CALL_TIMEOUT).build().await?;
    let caps: Vec<String> = conn
        .call_method(Some(DEST), PATH, Some(DEST), "GetCapabilities", &())
        .await?
        .body()
        .deserialize()?;
    Ok((conn, caps.iter().any(|c| c == "actions")))
}

async fn notify(conn: &Connection, body: &str, actions: bool) -> zbus::Result<u32> {
    let buttons: &[&str] = if actions {
        &["answer", "Answer", "reject", "Reject"]
    } else {
        &[]
    };
    let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
    hints.insert("urgency", Value::U8(2));
    hints.insert("category", Value::from("call.incoming"));
    let args = (
        "rust-microsip",
        0u32,
        "rust-microsip",
        "Incoming call",
        body,
        buttons,
        hints,
        EXPIRE_MS,
    );
    conn.call_method(Some(DEST), PATH, Some(DEST), "Notify", &args)
        .await?
        .body()
        .deserialize()
}

async fn close(conn: &Connection, id: u32) -> zbus::Result<()> {
    conn.call_method(Some(DEST), PATH, Some(DEST), "CloseNotification", &(id,))
        .await
        .map(drop)
}

/// Forward `ActionInvoked` signals sent by the notification service's current owner.
async fn listen(
    conn: &Connection,
    ids: &Mutex<HashMap<CallId, u32>>,
    tx: &SyncSender<NotificationAction>,
    waker: &Mutex<Option<Waker>>,
) -> zbus::Result<()> {
    // Only accept signals from the unique name that owns the service.
    let owner: String = conn
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetNameOwner",
            &(DEST,),
        )
        .await?
        .body()
        .deserialize()?;
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .sender(owner.as_str())?
        .path(PATH)?
        .interface(DEST)?
        .member("ActionInvoked")?
        .build();
    let mut stream = MessageStream::for_match_rule(rule, conn, Some(QUEUE)).await?;
    while let Some(message) = next(&mut stream).await {
        let Ok((id, key)) = message?.body().deserialize::<(u32, String)>() else {
            continue;
        };
        let action = match key.as_str() {
            "answer" => CallAction::Answer,
            "reject" => CallAction::Reject,
            _ => continue,
        };
        let call = lock(ids).iter().find(|(_, v)| **v == id).map(|(c, _)| *c);
        if let Some(call) = call
            && tx.try_send(NotificationAction { call, action }).is_ok()
            && let Some(wake) = lock(waker).as_ref()
        {
            wake();
        }
    }
    Ok(())
}

/// `StreamExt::next` without a futures dependency.
// ponytail: uses zbus's hidden futures_core re-export (zbus pinned =5.19.0);
// switch to a direct futures-core dependency if the pin moves.
async fn next(stream: &mut MessageStream) -> Option<zbus::Result<zbus::Message>> {
    use zbus::export::futures_core::Stream;
    std::future::poll_fn(|cx| std::pin::Pin::new(&mut *stream).poll_next(cx)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn wait_status(notifier: &Notifier) -> NotifierStatus {
        let deadline = Instant::now() + Duration::from_secs(10);
        while notifier.status() == NotifierStatus::Connecting && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        notifier.status()
    }

    #[test]
    fn missing_bus_degrades_to_unavailable() {
        let path = std::env::temp_dir().join(format!("rmsip-no-bus-{}", std::process::id()));
        for address in [
            format!("unix:path={}", path.display()),
            "not-an-address".to_owned(),
        ] {
            let notifier = Notifier::with_address(&address);
            assert_eq!(
                wait_status(&notifier),
                NotifierStatus::Unavailable,
                "{address}"
            );
            assert_eq!(
                notifier.incoming_call(CallId(1), "Alice"),
                Delivery::Unavailable
            );
            assert_eq!(notifier.close(CallId(1)), Delivery::Unavailable);
            assert_eq!(notifier.try_recv_action(), None);
        }
    }
}
