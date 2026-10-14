//! Termination requests: SIGTERM/SIGINT on Unix, Ctrl+C/Ctrl+Break on
//! Windows (session end arrives with add-windows-port task 2.11).

/// Stream of termination requests; each yields the exit code a process
/// killed by that signal would have (143 TERM, 130 INT).
pub struct Signals {
    #[cfg(unix)]
    term: tokio::signal::unix::Signal,
    #[cfg(unix)]
    int: tokio::signal::unix::Signal,
}

impl Signals {
    /// Install the handlers; `None` if the platform refuses them. Call
    /// inside a Tokio runtime with I/O enabled.
    pub fn new() -> Option<Self> {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            Some(Self {
                term: signal(SignalKind::terminate()).ok()?,
                int: signal(SignalKind::interrupt()).ok()?,
            })
        }
        #[cfg(not(unix))]
        Some(Self {})
    }

    /// Wait for the next request.
    pub async fn next(&mut self) -> u8 {
        #[cfg(unix)]
        {
            tokio::select! {
                _ = self.term.recv() => 143,
                _ = self.int.recv() => 130,
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
            130
        }
    }
}
