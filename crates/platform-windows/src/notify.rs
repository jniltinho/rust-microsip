//! Incoming-call toasts with Answer/Reject (design W5).
//!
//! WinRT toasts through `ToastNotificationManager` on a background thread;
//! callers only enqueue into a bounded queue. Each toast's buttons carry an
//! opaque random token (never the number) that maps back to the call in
//! memory; activation is handled in-process through the `Activated` event.
//! Toasts are best-effort: when the notifier cannot be created or is not
//! enabled (no registered AppUserModelID, notifications disabled, Server SKU)
//! the notifier reports [`NotifierStatus::Unavailable`] and the window banner
//! remains the way to answer. A single toast that fails to load or show is
//! skipped without disabling the others.

use crate::Waker;
use rust_microsip_control::caller::sanitize_caller;
use rust_microsip_core::CallId;
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::{
    NotificationSetting, ToastActivatedEventArgs, ToastNotification, ToastNotificationManager,
    ToastNotifier,
};
use windows::core::{GUID, HSTRING, IInspectable, Interface, Ref};

/// AppUserModelID the toasts are shown under.
const AUMID: &str = "rust-microsip";
const QUEUE: usize = 8;

/// Whether desktop notifications can currently be shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifierStatus {
    /// Still creating the toast notifier.
    Connecting,
    /// Toasts work; `actions` tells whether buttons are supported.
    Available {
        /// Answer/reject buttons are shown (always on Windows).
        actions: bool,
    },
    /// Toasts cannot be shown; use the window only.
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

/// Token -> call, shared with the `Activated` handlers.
type Tokens = Shared<HashMap<String, CallId>>;

/// Fire-and-forget incoming-call notifier. Dropping it hides open toasts.
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

impl Default for Notifier {
    fn default() -> Self {
        Self::session()
    }
}

impl Notifier {
    /// Toasts for the current user, under the `rust-microsip` AppUserModelID.
    pub fn session() -> Self {
        let (jobs, rx) = mpsc::sync_channel(QUEUE);
        let (action_tx, actions) = mpsc::sync_channel(QUEUE);
        let status = Arc::new(Mutex::new(NotifierStatus::Connecting));
        let waker: Shared<Option<Waker>> = Arc::default();
        let (st, wk) = (Arc::clone(&status), Arc::clone(&waker));
        let spawned = thread::Builder::new()
            .name("notifier".into())
            .spawn(move || worker(&rx, &st, action_tx, wk));
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

    /// Same as [`Notifier::session`]; Windows has no service address.
    pub fn with_address(_address: &str) -> Self {
        Self::session()
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
        self.send(Job::Show(call, toast_body(caller)))
    }

    /// Hide the toast for `call` (answered, rejected or ended).
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
    jobs: &Receiver<Job>,
    status: &Mutex<NotifierStatus>,
    action_tx: SyncSender<NotificationAction>,
    waker: Shared<Option<Waker>>,
) {
    let Some(notifier) = open() else {
        // Dropping `jobs` on return makes later sends report Unavailable.
        *lock(status) = NotifierStatus::Unavailable;
        return;
    };
    *lock(status) = NotifierStatus::Available { actions: true };
    let tokens: Tokens = Arc::default();
    let mut shown: HashMap<CallId, (String, ToastNotification)> = HashMap::new();
    for job in jobs {
        match job {
            Job::Show(call, body) => {
                hide(&notifier, &tokens, shown.remove(&call));
                // A toast that fails is skipped; the window banner still
                // offers the call and later toasts are unaffected.
                if let Ok(entry) = show(&notifier, &body, call, &tokens, &action_tx, &waker) {
                    shown.insert(call, entry);
                }
            }
            Job::Close(call) => hide(&notifier, &tokens, shown.remove(&call)),
        }
    }
    for (_, entry) in shown.drain() {
        hide(&notifier, &tokens, Some(entry));
    }
}

/// The notifier for [`AUMID`], if toasts are enabled for it.
fn open() -> Option<ToastNotifier> {
    let notifier =
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID)).ok()?;
    // Fails or reports a disabled setting when the AUMID is not registered,
    // the user turned notifications off, or the SKU has no toast platform.
    (notifier.Setting().ok()? == NotificationSetting::Enabled).then_some(notifier)
}

fn show(
    notifier: &ToastNotifier,
    body: &str,
    call: CallId,
    tokens: &Tokens,
    action_tx: &SyncSender<NotificationAction>,
    waker: &Shared<Option<Waker>>,
) -> windows::core::Result<(String, ToastNotification)> {
    let token = format!("{:032x}", GUID::new()?.to_u128());
    let xml = XmlDocument::new()?;
    xml.LoadXml(&HSTRING::from(toast_xml(body, &token)))?;
    let toast = ToastNotification::CreateToastNotification(&xml)?;
    let (tokens_h, tx, wk) = (Arc::clone(tokens), action_tx.clone(), Arc::clone(waker));
    toast.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(
        move |_, args: Ref<IInspectable>| {
            if let Some(args) = args.as_ref()
                && let Ok(args) = args.cast::<ToastActivatedEventArgs>()
                && let Ok(arguments) = args.Arguments()
                && let Some(action) = take_action(&arguments.to_string_lossy(), &tokens_h)
                && tx.try_send(action).is_ok()
            {
                // Clone out of the lock so the waker runs unlocked, and keep
                // a panicking waker from unwinding into WinRT.
                let wake = lock(&wk).clone();
                if let Some(wake) = wake {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| wake()));
                }
            }
            Ok(())
        },
    ))?;
    lock(tokens).insert(token.clone(), call);
    if let Err(e) = notifier.Show(&toast) {
        lock(tokens).remove(&token);
        return Err(e);
    }
    Ok((token, toast))
}

fn hide(notifier: &ToastNotifier, tokens: &Tokens, entry: Option<(String, ToastNotification)>) {
    if let Some((token, toast)) = entry {
        lock(tokens).remove(&token);
        let _ = notifier.Hide(&toast);
    }
}

/// The toast body for `caller`: only characters valid in XML 1.0, then
/// `sanitize_caller` (controls and bidi marks removed, markup escaped), so
/// `LoadXml` never sees an invalid character or markup from the caller.
fn toast_body(caller: &str) -> String {
    let valid: String = caller.chars().filter(|&c| is_xml_char(c)).collect();
    sanitize_caller(&valid)
}

/// `Char` of XML 1.0: #x9 | #xA | #xD | [#x20-#xD7FF] | [#xE000-#xFFFD] |
/// [#x10000-#x10FFFF] (surrogates cannot occur in a `char`).
fn is_xml_char(c: char) -> bool {
    matches!(c, '\u{9}' | '\u{A}' | '\u{D}' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

/// Toast XML; `body` comes from `toast_body` (valid XML characters, escaped)
/// and `token` is hex, so neither can break out of its text or attribute.
fn toast_xml(body: &str, token: &str) -> String {
    format!(
        "<toast scenario=\"incomingCall\">\
<visual><binding template=\"ToastGeneric\"><text>Incoming call</text><text>{body}</text></binding></visual>\
<actions>\
<action content=\"Answer\" arguments=\"answer:{token}\" activationType=\"foreground\"/>\
<action content=\"Reject\" arguments=\"reject:{token}\" activationType=\"foreground\"/>\
</actions></toast>"
    )
}

/// The action for button `arguments`, consuming the token so a call is acted
/// on at most once; unknown or stale tokens and body clicks yield `None`.
fn take_action(arguments: &str, tokens: &Tokens) -> Option<NotificationAction> {
    let (kind, token) = arguments.split_once(':')?;
    let action = match kind {
        "answer" => CallAction::Answer,
        "reject" => CallAction::Reject,
        _ => return None,
    };
    let call = lock(tokens).remove(token)?;
    Some(NotificationAction { call, action })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_carries_the_escaped_caller_and_token_only() {
        let body = sanitize_caller("Alice <1002> & \"Bob\"");
        let xml = toast_xml(&body, "00ff");
        assert!(xml.contains("<text>Incoming call</text>"));
        assert!(xml.contains("<text>Alice &lt;1002&gt; &amp; \"Bob\"</text>"));
        assert!(xml.contains("arguments=\"answer:00ff\" activationType=\"foreground\""));
        assert!(xml.contains("arguments=\"reject:00ff\" activationType=\"foreground\""));
        assert!(!xml.contains("<1002>"));
    }

    #[test]
    fn hostile_callers_yield_valid_escaped_text() {
        let hostile = "A\u{FFFE}B\u{FFFF}C\u{0}\u{1}\u{8}\u{B}\u{C}\u{1F}\u{7F}D&amp;</text></binding></visual><actions><action content=\"x\" arguments=\"answer:00\"/>\
<![CDATA[x]]><!-- c --><?pi?>\u{1F600}";
        let body = toast_body(hostile);
        assert!(body.chars().all(is_xml_char), "{body:?}");
        assert!(!body.contains('<') && !body.contains('>'), "{body:?}");
        assert!(body.starts_with("ABCD&amp;amp;&lt;/text&gt;"), "{body:?}");
        let xml = toast_xml(&body, "00ff");
        assert_eq!(xml.matches("<action ").count(), 2);
        assert_eq!(xml.matches("<text>").count(), 2);
        // The real parser accepts it (no toast platform or AUMID needed).
        let parsed = XmlDocument::new()
            .and_then(|doc| doc.LoadXml(&HSTRING::from(xml.as_str())).map(|()| doc));
        assert!(parsed.is_ok(), "{parsed:?}");
        // Only invalid characters: the generic placeholder, never empty.
        assert_eq!(toast_body("\u{FFFE}\u{FFFF}\u{1}"), "Unknown caller");
        for c in [
            '\u{9}',
            '\u{A}',
            '\u{D}',
            ' ',
            '\u{D7FF}',
            '\u{E000}',
            '\u{FFFD}',
            '\u{10000}',
            '\u{10FFFF}',
        ] {
            assert!(is_xml_char(c), "{c:?}");
        }
        for c in ['\u{0}', '\u{8}', '\u{B}', '\u{1F}', '\u{FFFE}', '\u{FFFF}'] {
            assert!(!is_xml_char(c), "{c:?}");
        }
    }

    #[test]
    fn session_settles_without_blocking() {
        let notifier = Notifier::session();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while notifier.status() == NotifierStatus::Connecting
            && std::time::Instant::now() < deadline
        {
            thread::sleep(std::time::Duration::from_millis(20));
        }
        // Unavailable without a registered AUMID (CI runners, Wine).
        eprintln!("notifier status: {:?}", notifier.status());
        if notifier.status() == NotifierStatus::Unavailable {
            assert_eq!(
                notifier.incoming_call(CallId(1), "Alice"),
                Delivery::Unavailable
            );
            assert_eq!(notifier.close(CallId(1)), Delivery::Unavailable);
        }
        assert_ne!(notifier.status(), NotifierStatus::Connecting);
        assert_eq!(notifier.try_recv_action(), None);
    }

    #[test]
    fn tokens_map_to_calls_once() {
        let tokens: Tokens = Arc::default();
        let call = CallId(7);
        lock(&tokens).insert("abc".into(), call);
        assert_eq!(take_action("bogus:abc", &tokens), None);
        assert_eq!(take_action("", &tokens), None);
        assert_eq!(
            take_action("reject:abc", &tokens),
            Some(NotificationAction {
                call,
                action: CallAction::Reject
            })
        );
        assert_eq!(take_action("answer:abc", &tokens), None);
        assert_eq!(take_action("answer:other", &tokens), None);
    }
}
