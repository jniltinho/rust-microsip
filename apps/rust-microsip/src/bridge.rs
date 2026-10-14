//! Work that must not wait for the window's event loop.
//!
//! On Wayland the compositor sends no frame callbacks to a covered or
//! minimized window, and eframe then runs no frame at all (it cannot tell the
//! window is hidden), so anything driven from the UI loop stalls. This thread
//! takes the engine's events first: it shows and closes the incoming-call
//! notification and sends its Answer/Reject straight to the engine, forwards
//! tray clicks, and passes everything on to the window. A recent call chosen
//! in the tray is dialed from here too (when no call is active), so it works
//! while the window is hidden or, on Wayland, minimized. A tray Quit still
//! goes through the window (it may need to confirm hanging up, and the window
//! records history), so it waits until the window draws again; SIGTERM has
//! its own fallback in `main`.

use rust_microsip::ui::External;
use rust_microsip_core::{CallId, CallPhase, Command, DialTarget, Direction, Event};
use rust_microsip_platform::{
    CallAction, Notifier, RecentCall, Tray, TrayAction, TrayStatus, Waker,
};
use rust_microsip_telephony::engine::EngineHandle;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::thread::JoinHandle;
use std::time::Duration;

/// How long the window loop may stay silent after SIGTERM before the
/// process exits without it.
pub const STALL: Duration = Duration::from_secs(3);
/// Bounded engine shutdown (unregister, BYE).
pub const SHUTDOWN: Duration = Duration::from_secs(4);
const IDLE: Duration = Duration::from_millis(250);

/// Handle to the bridge thread; dropping it stops the thread.
pub struct Bridge {
    events: Receiver<Vec<Event>>,
    external: Receiver<External>,
    ticks: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    nudge: SyncSender<()>,
    recent: mpsc::Sender<Vec<RecentCall>>,
    tray_available: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

/// The channel the engine, the notifier and the tray use to wake the bridge.
pub fn nudge_channel() -> (SyncSender<()>, Receiver<()>) {
    mpsc::sync_channel(1)
}

/// A waker that nudges the bridge (never blocks).
pub fn nudger(nudge: &SyncSender<()>) -> Waker {
    let nudge = nudge.clone();
    Arc::new(move || {
        let _ = nudge.try_send(());
    })
}

impl Bridge {
    /// Start the thread. `ui_wake` repaints the window after new input;
    /// `nudge`/`nudged` come from [`nudge_channel`], and the engine must have
    /// been started with [`nudger`] on the same channel. `None` when the
    /// thread cannot be created (nothing would then read engine events).
    pub fn spawn(
        engine: Arc<EngineHandle>,
        notifier: Notifier,
        tray: Tray,
        (nudge, nudged): (SyncSender<()>, Receiver<()>),
        ui_wake: Waker,
        ticks: Arc<AtomicU64>,
    ) -> Option<Self> {
        notifier.set_waker(nudger(&nudge));
        tray.set_waker(nudger(&nudge));
        let (events_tx, events) = mpsc::channel();
        let (external_tx, external) = mpsc::channel();
        let (recent, recent_rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let tray_available = Arc::new(AtomicBool::new(false));
        let worker = Worker {
            engine,
            notifier,
            tray,
            ui_wake,
            stop: Arc::clone(&stop),
            events: events_tx,
            external: external_tx,
            ringing: None,
            active: None,
            recent: recent_rx,
            tray_available: Arc::clone(&tray_available),
        };
        let thread = std::thread::Builder::new()
            .name("rust-microsip-bridge".into())
            .spawn(move || worker.run(&nudged))
            .ok()?;
        Some(Self {
            events,
            external,
            ticks,
            stop,
            nudge,
            recent,
            tray_available,
            thread: Some(thread),
        })
    }

    /// Engine events forwarded since the last call.
    pub fn events(&self) -> Vec<Event> {
        self.events.try_iter().flatten().collect()
    }

    /// Requests for the window (tray clicks) since the last call.
    pub fn external(&self) -> Vec<External> {
        self.external.try_iter().collect()
    }

    /// Replace the recent calls in the tray menu (applied by the thread).
    pub fn set_recent(&self, items: Vec<RecentCall>) {
        if self.recent.send(items).is_ok() {
            let _ = self.nudge.try_send(());
        }
    }

    /// Whether the tray icon is shown, so closing the window can hide it.
    pub fn tray_available(&self) -> bool {
        self.tray_available.load(Ordering::SeqCst)
    }

    /// Record that the window loop ran (it is not stalled); also called when
    /// the window shuts down, so the SIGTERM fallback never races it.
    pub fn tick(&self) {
        self.ticks.fetch_add(1, Ordering::SeqCst);
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.nudge.try_send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Worker {
    engine: Arc<EngineHandle>,
    notifier: Notifier,
    tray: Tray,
    ui_wake: Waker,
    stop: Arc<AtomicBool>,
    events: mpsc::Sender<Vec<Event>>,
    external: mpsc::Sender<External>,
    // ponytail: one ringing call; the engine handles one call at a time.
    ringing: Option<CallId>,
    /// A call that has not ended (any direction or phase).
    active: Option<CallId>,
    recent: Receiver<Vec<RecentCall>>,
    tray_available: Arc<AtomicBool>,
}

impl Worker {
    fn run(mut self, nudged: &Receiver<()>) {
        loop {
            if let Err(RecvTimeoutError::Disconnected) = nudged.recv_timeout(IDLE) {
                return;
            }
            if self.stop.load(Ordering::SeqCst) {
                return;
            }
            let mut wake = false;
            let events = self.engine.try_events();
            if !events.is_empty() {
                self.observe(&events);
                wake |= self.events.send(events).is_ok();
            }
            while let Some(action) = self.notifier.try_recv_action() {
                // Only for the call that is still ringing.
                if self.ringing == Some(action.call) {
                    let command = match action.action {
                        CallAction::Answer => Command::Answer(action.call),
                        CallAction::Reject => Command::Reject(action.call),
                    };
                    let _ = self.engine.send(command);
                }
            }
            if let Some(items) = self.recent.try_iter().last() {
                self.tray.set_recent(items);
            }
            self.tray_available.store(
                self.tray.status() == TrayStatus::Available,
                Ordering::SeqCst,
            );
            while let Some(action) = self.tray.try_recv() {
                let request = match action {
                    TrayAction::Show => External::Activate,
                    TrayAction::Quit => External::QuitRequest,
                    TrayAction::Dial(number) => self.redial(&number),
                };
                wake |= self.external.send(request).is_ok();
            }
            if wake {
                (self.ui_wake)();
            }
        }
    }

    /// Dial a recent call chosen in the tray, unless a call is active; the
    /// window is then shown with the number. A malformed number only shows
    /// the window.
    fn redial(&mut self, number: &str) -> External {
        let Ok(target) = DialTarget::parse(number) else {
            return External::Activate;
        };
        if self.active.is_none()
            && let Err(notice) = self.engine.send(Command::Dial(target.clone()))
        {
            let _ = self.events.send(vec![Event::Notice(notice)]);
        }
        External::Dial(target)
    }

    /// Mirror incoming-call state into the desktop notification and track
    /// whether a call is active.
    fn observe(&mut self, events: &[Event]) {
        for event in events {
            let Event::Call(call) = event else { continue };
            let ended = matches!(call.phase, CallPhase::Ended(_));
            if !ended {
                self.active = Some(call.id);
            } else if self.active == Some(call.id) {
                self.active = None;
            }
            let ringing = call.direction == Direction::Incoming && call.phase == CallPhase::Ringing;
            if ringing && self.ringing != Some(call.id) {
                let _ = self.notifier.incoming_call(call.id, &call.remote);
                self.ringing = Some(call.id);
            } else if !ringing && self.ringing == Some(call.id) {
                let _ = self.notifier.close(call.id);
                self.ringing = None;
            }
        }
    }
}

/// Whether the window loop has not run since `before` was read.
pub fn stalled(before: u64, now: u64) -> bool {
    before == now
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_loop_that_ran_is_not_stalled() {
        assert!(super::stalled(4, 4));
        assert!(!super::stalled(4, 5));
    }
}
