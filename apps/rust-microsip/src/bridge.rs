//! Work that must not wait for the window's event loop.
//!
//! On Wayland the compositor sends no frame callbacks to a covered or
//! minimized window, and eframe then runs no frame at all (it cannot tell the
//! window is hidden), so anything driven from the UI loop stalls. This thread
//! takes the engine's events first: it shows and closes the incoming-call
//! notification and sends its Answer/Reject straight to the engine, forwards
//! tray clicks, and passes everything on to the window. When the window loop
//! is stalled, a tray Quit without a call shuts the engine down and exits.

use rust_microsip::ui::External;
use rust_microsip_core::{CallId, CallPhase, Command, Direction, Event};
use rust_microsip_platform::{CallAction, Notifier, Tray, TrayAction, Waker};
use rust_microsip_telephony::engine::EngineHandle;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How long the window loop may stay silent before it counts as stalled.
pub const STALL: Duration = Duration::from_secs(3);
/// Bounded engine shutdown (unregister, BYE) before exiting without the window.
pub const SHUTDOWN: Duration = Duration::from_secs(4);
const IDLE: Duration = Duration::from_millis(250);

/// Handle to the bridge thread; dropping it stops the thread.
pub struct Bridge {
    events: Receiver<Vec<Event>>,
    external: Receiver<External>,
    ticks: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    nudge: SyncSender<()>,
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
    /// been started with [`nudger`] on the same channel.
    pub fn spawn(
        engine: Arc<EngineHandle>,
        notifier: Notifier,
        tray: Tray,
        (nudge, nudged): (SyncSender<()>, Receiver<()>),
        ui_wake: Waker,
        ticks: Arc<AtomicU64>,
    ) -> Self {
        notifier.set_waker(nudger(&nudge));
        tray.set_waker(nudger(&nudge));
        let (events_tx, events) = mpsc::channel();
        let (external_tx, external) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker = Worker {
            engine,
            notifier,
            tray,
            ui_wake,
            ticks: Arc::clone(&ticks),
            stop: Arc::clone(&stop),
            events: events_tx,
            external: external_tx,
            ringing: None,
            in_call: false,
            quit_requested: None,
        };
        let thread = std::thread::Builder::new()
            .name("rust-microsip-bridge".into())
            .spawn(move || worker.run(&nudged))
            .ok();
        Self {
            events,
            external,
            ticks,
            stop,
            nudge,
            thread,
        }
    }

    /// Engine events forwarded since the last call.
    pub fn events(&self) -> Vec<Event> {
        self.events.try_iter().flatten().collect()
    }

    /// Requests for the window (tray clicks) since the last call.
    pub fn external(&self) -> Vec<External> {
        self.external.try_iter().collect()
    }

    /// Record that the window loop ran (it is not stalled).
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
    ticks: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    events: mpsc::Sender<Vec<Event>>,
    external: mpsc::Sender<External>,
    ringing: Option<CallId>,
    in_call: bool,
    quit_requested: Option<(Instant, u64)>,
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
            while let Some(action) = self.tray.try_recv() {
                let request = match action {
                    TrayAction::Show => External::Activate,
                    TrayAction::Quit => {
                        self.quit_requested = Some((Instant::now(), self.ticks()));
                        External::QuitRequest
                    }
                };
                wake |= self.external.send(request).is_ok();
            }
            if wake {
                (self.ui_wake)();
            }
            if let Some((at, tick)) = self.quit_requested
                && at.elapsed() >= STALL
            {
                self.quit_requested = None;
                // The window never ran: quit without it, but never drop a call
                // (that needs the window's confirmation).
                if stalled(tick, self.ticks()) && !self.in_call {
                    let _ = self.engine.shutdown(SHUTDOWN);
                    std::process::exit(0);
                }
            }
        }
    }

    fn ticks(&self) -> u64 {
        self.ticks.load(Ordering::SeqCst)
    }

    /// Mirror incoming-call state into the desktop notification.
    fn observe(&mut self, events: &[Event]) {
        for event in events {
            let Event::Call(call) = event else { continue };
            self.in_call = !matches!(call.phase, CallPhase::Ended(_));
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
