//! Owner session of the control socket (AG-04, AG-05): epoch, payload MACs,
//! operation ids, approvals waiting for a person and the DTMF pacing queue.
//!
//! The HMAC key is random per owner start and never leaves memory, so the
//! ledger can detect a reused idempotency key with a different payload in the
//! same epoch without storing digits or numbers. After a restart the key is
//! gone: old keys are answered `stale_epoch` and never re-executed.

use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use rust_microsip_control::{Operation, OwnerMode};
use rust_microsip_core::{CallId, DtmfKey};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How long a person has to approve a request in the window.
pub const APPROVAL_WINDOW: Duration = Duration::from_secs(60);
/// Requests waiting for approval at once; more are answered `busy`.
pub const MAX_PENDING: usize = 8;
/// How long "Deny all and pause" refuses new control mutations.
pub const PAUSE_AFTER_DENY_ALL: Duration = Duration::from_secs(300);
/// Gap between paced DTMF digits (a 120 ms event plus a pause).
pub const DTMF_GAP: Duration = Duration::from_millis(200);

/// A mutation waiting for a person's decision.
#[derive(Clone, Debug)]
pub struct PendingApproval {
    /// Ledger id.
    pub operation_id: String,
    /// What will happen, for the person deciding (may show the destination).
    pub description: String,
    /// Who asked.
    pub requester: String,
    /// Decision deadline.
    pub deadline: Instant,
    pub(crate) operation: Operation,
}

/// Digits of one approved DTMF operation, held only in memory.
#[derive(Debug)]
pub(crate) struct DtmfJob {
    pub operation_id: String,
    pub call: CallId,
    pub keys: VecDeque<DtmfKey>,
    pub sent: usize,
    pub next: Instant,
}

/// Per-start state of the profile owner.
pub struct ControlOwner {
    /// Window or headless service.
    pub mode: OwnerMode,
    /// Ledger epoch of this start.
    pub epoch: i64,
    /// The ledger is on disk; mutations are refused otherwise.
    pub durable: bool,
    key: hmac::Key,
    rng: SystemRandom,
    pub(crate) pending: Vec<PendingApproval>,
    pub(crate) dtmf: VecDeque<DtmfJob>,
    /// Set by "Deny all and pause": new mutations answer `busy` until then.
    pub(crate) paused_until: Option<Instant>,
}
impl std::fmt::Debug for ControlOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlOwner")
            .field("mode", &self.mode)
            .field("epoch", &self.epoch)
            .field("durable", &self.durable)
            .field("pending", &self.pending.len())
            .finish_non_exhaustive()
    }
}

impl ControlOwner {
    /// New session with a fresh random MAC key.
    ///
    /// # Errors
    /// When the system random generator fails.
    pub fn new(mode: OwnerMode, epoch: i64, durable: bool) -> Result<Self, &'static str> {
        let rng = SystemRandom::new();
        let key =
            hmac::Key::generate(hmac::HMAC_SHA256, &rng).map_err(|_| "no system randomness")?;
        Ok(Self {
            mode,
            epoch,
            durable,
            key,
            rng,
            pending: Vec::new(),
            dtmf: VecDeque::new(),
            paused_until: None,
        })
    }

    /// HMAC-SHA-256 of the canonical JSON of `operation`.
    #[must_use]
    pub fn mac(&self, operation: &Operation) -> Vec<u8> {
        let canonical = serde_json::to_vec(operation).unwrap_or_default();
        hmac::sign(&self.key, &canonical).as_ref().to_vec()
    }

    /// A new unguessable operation id.
    ///
    /// # Errors
    /// When the system random generator fails.
    pub fn operation_id(&self) -> Result<String, &'static str> {
        let mut bytes = [0u8; 12];
        self.rng
            .fill(&mut bytes)
            .map_err(|_| "no system randomness")?;
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        Ok(format!("op-{}-{hex}", self.epoch))
    }

    /// Requests waiting for a decision, oldest first.
    #[must_use]
    pub fn pending(&self) -> &[PendingApproval] {
        &self.pending
    }
}

/// Destination shown in audit rows: only the last two characters.
#[must_use]
pub fn masked(target: &str) -> String {
    let tail: String = target
        .chars()
        .rev()
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("***{tail}")
}

/// Human description of an operation for the approval prompt. The person
/// deciding sees the full destination; digits are counted, never shown.
#[must_use]
pub fn describe(operation: &Operation) -> String {
    match operation {
        Operation::CallDial { target } => format!("call {target}"),
        Operation::CallAnswer(_) => "answer the ringing call (starts the microphone)".into(),
        Operation::CallReject(_) => "reject the ringing call".into(),
        Operation::CallHangup(_) => "hang up the call".into(),
        Operation::CallDtmf { digits, .. } => {
            format!("send {} DTMF digit(s) in the call", digits.chars().count())
        }
        Operation::CallMute { muted, .. } => if *muted {
            "mute the microphone"
        } else {
            "unmute the microphone"
        }
        .into(),
        Operation::CallHold { held, .. } => if *held {
            "put the call on hold"
        } else {
            "resume the call"
        }
        .into(),
        Operation::CallTransfer { target, .. } => format!("transfer the call to {target}"),
        Operation::DndSet { enabled } => if *enabled {
            "turn do-not-disturb on"
        } else {
            "turn do-not-disturb off"
        }
        .into(),
        other => other.name().into(),
    }
}

/// Audit detail without full numbers or digits.
#[must_use]
pub fn audit_detail(operation: &Operation) -> String {
    match operation {
        Operation::CallDial { target } | Operation::CallTransfer { target, .. } => {
            format!("destination {}", masked(target))
        }
        Operation::CallDtmf { digits, .. } => format!("{} digit(s)", digits.chars().count()),
        Operation::CallMute { muted, .. } => format!("muted={muted}"),
        Operation::CallHold { held, .. } => format!("held={held}"),
        Operation::DndSet { enabled } => format!("enabled={enabled}"),
        _ => "-".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macs_bind_payloads_and_ids_are_unique() -> Result<(), &'static str> {
        let owner = ControlOwner::new(OwnerMode::Gui, 3, true)?;
        let a = Operation::CallDial {
            target: "1002".into(),
        };
        let b = Operation::CallDial {
            target: "1003".into(),
        };
        assert_eq!(owner.mac(&a), owner.mac(&a));
        assert_ne!(owner.mac(&a), owner.mac(&b));
        assert_eq!(owner.mac(&a).len(), 32);
        let other = ControlOwner::new(OwnerMode::Gui, 3, true)?;
        assert_ne!(owner.mac(&a), other.mac(&a), "the key is per start");
        let (x, y) = (owner.operation_id()?, owner.operation_id()?);
        assert!(x.starts_with("op-3-") && x != y && rust_microsip_control::token(&x, 64));
        Ok(())
    }

    #[test]
    fn audit_text_never_holds_digits_or_full_numbers() {
        let dtmf = Operation::CallDtmf {
            call_id: 1,
            expected_version: 1,
            digits: "4321#".into(),
        };
        assert_eq!(audit_detail(&dtmf), "5 digit(s)");
        assert!(!describe(&dtmf).contains("4321"));
        let dial = Operation::CallDial {
            target: "5511998877".into(),
        };
        assert_eq!(audit_detail(&dial), "destination ***77");
        assert_eq!(describe(&dial), "call 5511998877");
    }
}
