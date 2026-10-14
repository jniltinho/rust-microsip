//! Notification-area icon with Show/Quit (design W5).
// ponytail: stand-in until W-B task 2.9.

use crate::Waker;
// --- tray ------------------------------------------------------------------

/// Whether the tray icon is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayStatus {
    /// Still connecting.
    Connecting,
    /// Shown.
    Available,
    /// Not shown.
    Unavailable,
}

/// A click on the tray icon or its menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    /// Bring the window to the front.
    Show,
    /// Quit like the window's close button.
    Quit,
}

/// Tray icon (the notification area icon arrives in milestone W-B).
#[derive(Debug)]
pub struct Tray;

impl Tray {
    /// Show the icon.
    pub fn session(_width: u32, _height: u32, _rgba: &[u8]) -> Self {
        Self
    }
    /// Current availability.
    pub fn status(&self) -> TrayStatus {
        TrayStatus::Unavailable
    }
    /// Run `waker` after each click.
    pub fn set_waker(&self, _waker: Waker) {}
    /// Next click.
    pub fn try_recv(&self) -> Option<TrayAction> {
        None
    }
}
