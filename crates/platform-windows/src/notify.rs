//! Incoming-call toasts with Answer/Reject (design W5).
// ponytail: stand-in until W-B task 2.8.

use crate::Waker;
use rust_microsip_core::CallId;
// --- notifications ---------------------------------------------------------

/// Whether desktop notifications can currently be shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifierStatus {
    /// Still connecting.
    Connecting,
    /// Notifications work; `actions` tells whether buttons are supported.
    Available {
        /// The server supports answer/reject buttons.
        actions: bool,
    },
    /// No notification service; use the window only.
    Unavailable,
}

/// Outcome of a non-blocking notification request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Queued.
    Queued,
    /// The queue is full; dropped.
    QueueFull,
    /// Notifications are unavailable; nothing was sent.
    Unavailable,
}

/// Button pressed on an incoming-call notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallAction {
    /// "Answer".
    Answer,
    /// "Reject".
    Reject,
}

/// Action invoked on the notification for `call`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotificationAction {
    /// Call the notification was shown for.
    pub call: CallId,
    /// Chosen action.
    pub action: CallAction,
}

/// Incoming-call notifier (toasts arrive in milestone W-B).
#[derive(Debug, Default)]
pub struct Notifier;

impl Notifier {
    /// The user's notification service.
    pub fn session() -> Self {
        Self
    }
    /// An explicit service address (tests).
    pub fn with_address(_address: &str) -> Self {
        Self
    }
    /// Current availability.
    pub fn status(&self) -> NotifierStatus {
        NotifierStatus::Unavailable
    }
    /// Run `waker` after each action.
    pub fn set_waker(&self, _waker: Waker) {}
    /// Show "Incoming call".
    pub fn incoming_call(&self, _call: CallId, _caller: &str) -> Delivery {
        Delivery::Unavailable
    }
    /// Close the notification for `call`.
    pub fn close(&self, _call: CallId) -> Delivery {
        Delivery::Unavailable
    }
    /// Next action pressed by the user.
    pub fn try_recv_action(&self) -> Option<NotificationAction> {
        None
    }
}
