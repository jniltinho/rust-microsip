//! Control requests from the CLI/MCP, handled by the window's reducer so the
//! owner has exactly one source of call state (AG-01, AG-04, AG-05).
//!
//! Reads: `status_get` (no names or numbers) and `operation_get` (state and
//! sanitized outcome) need no grant; contacts, history, devices and the
//! configuration need a read grant, not offered yet, so they answer
//! `grant_required`. Mutations are recorded in the durable ledger before any
//! effect and wait for a person to approve them in the window; the headless
//! service answers `grant_required` instead.

use super::{Backend, UiState};
use crate::owner::{
    APPROVAL_WINDOW, DTMF_GAP, DtmfJob, MAX_PENDING, PendingApproval, audit_detail, describe,
};
use rust_microsip_control::{
    CallRef, ControlError, ErrorCode, Operation, OwnerMode, Request, Response, Status,
};
use rust_microsip_core::{
    CallId, CallPhase, Command, DialTarget, Direction, DtmfKey, RegistrationState,
};
use rust_microsip_platform::Incoming;
use rust_microsip_storage::{OperationRecord, OperationState};
use serde_json::{Value, json};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A DTMF queue not advanced for this long is abandoned.
const STALLED_DTMF: Duration = Duration::from_secs(1);

fn error(code: ErrorCode, message: &str) -> ControlError {
    ControlError::new(code, message)
}

fn phase_name(phase: CallPhase) -> String {
    format!("{phase:?}").to_ascii_lowercase()
}

fn state_name(state: OperationState) -> &'static str {
    match state {
        OperationState::AwaitingApproval => "awaiting_approval",
        OperationState::Executing => "executing",
        OperationState::Succeeded => "succeeded",
        OperationState::Failed => "failed",
        OperationState::Denied => "denied",
        OperationState::Expired => "expired",
        OperationState::OutcomeUnknown => "outcome_unknown",
    }
}

impl UiState {
    /// DTMF digits are still being paced out.
    #[must_use]
    pub fn control_busy(&self) -> bool {
        self.control.as_ref().is_some_and(|o| !o.dtmf.is_empty())
    }

    /// Requests waiting for the person's decision in the window.
    #[must_use]
    pub fn pending_approvals(&self) -> &[PendingApproval] {
        self.control.as_ref().map_or(&[], |owner| owner.pending())
    }

    /// Answer one control request.
    pub fn control_request(&mut self, incoming: Incoming, now: Instant) {
        // A mutation the client already gave up on is dropped unrecorded, so
        // a retry with a new key cannot create a second identical prompt.
        if incoming.request.operation.is_mutation() && incoming.is_stale() {
            return;
        }
        let response = self.control_response(&incoming.request, incoming.peer_pid, now);
        incoming.respond(response);
    }

    fn control_response(&mut self, request: &Request, pid: Option<u32>, now: Instant) -> Response {
        let id = &request.request_id;
        let result = match &request.operation {
            Operation::StatusGet => Ok(Response::ok(id, self.status_json())),
            Operation::OperationGet { operation_id } => self
                .operation_json(operation_id)
                .map(|data| Response::ok(id, data)),
            Operation::DevicesList
            | Operation::ContactsSearch { .. }
            | Operation::HistoryList { .. }
            | Operation::ConfigShow => self
                .granted_read(request)
                .map(|data| Response::ok(id, data)),
            _ => self.control_mutation(request, pid, now),
        };
        result.unwrap_or_else(|e| Response::error(id, &e))
    }

    fn status_json(&self) -> Value {
        let registration = match self.registration {
            RegistrationState::Disabled => "disabled",
            RegistrationState::Registering => "registering",
            RegistrationState::Registered => "registered",
            RegistrationState::Failed(_) => "failed",
        };
        let call = self.call.as_ref().map(|call| {
            let s = &call.snapshot;
            json!({
                "call_id": s.id.0,
                "version": s.version,
                "direction": if s.direction == Direction::Incoming { "incoming" } else { "outgoing" },
                "phase": phase_name(s.phase),
                "secure": s.secure,
                "muted": s.muted,
                "held": s.phase == CallPhase::Held,
            })
        });
        let (mode, epoch) = self
            .control
            .as_ref()
            .map_or((None, None), |o| (Some(o.mode), Some(o.epoch)));
        json!({
            "mode": mode,
            "epoch": epoch,
            "registration": registration,
            "dnd": self.dnd,
            "call": call,
        })
    }

    fn operation_json(&self, operation_id: &str) -> Result<Value, ControlError> {
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| error(ErrorCode::StorageUnavailable, "the ledger is unavailable"))?;
        let record = store
            .control_get(operation_id)
            .map_err(|_| error(ErrorCode::StorageUnavailable, "the ledger is unavailable"))?
            .ok_or_else(|| {
                error(
                    ErrorCode::NotFound,
                    "unknown operation, or older than the 24-hour retention",
                )
            })?;
        let current = self.control.as_ref().map(|o| o.epoch);
        Ok(json!({
            "operation_id": record.operation_id,
            "operation": record.operation,
            "state": state_name(record.state),
            "final": record.state.is_final(),
            "current_epoch": current == Some(record.epoch),
            "outcome": record.outcome.as_deref().and_then(|o| serde_json::from_str::<Value>(o).ok()),
        }))
    }

    /// The call a session operation names must still be the current call at
    /// the version the client saw.
    fn check_call(&self, call: CallRef) -> Result<CallId, ControlError> {
        match &self.call {
            Some(active)
                if active.snapshot.id.0 == call.call_id
                    && active.snapshot.version == call.expected_version =>
            {
                Ok(active.snapshot.id)
            }
            _ => Err(error(
                ErrorCode::NotFound,
                "the call ended or changed; read status_get again",
            )),
        }
    }

    fn control_mutation(
        &mut self,
        request: &Request,
        pid: Option<u32>,
        now: Instant,
    ) -> Result<Response, ControlError> {
        let id = &request.request_id;
        let operation = &request.operation;
        let Some(key) = request.idempotency_key.as_deref() else {
            return Err(error(ErrorCode::Validation, "idempotency_key is required"));
        };
        let owner = self
            .control
            .as_ref()
            .ok_or_else(|| error(ErrorCode::Internal, "control is not active"))?;
        let (epoch, mode, durable) = (owner.epoch, owner.mode, owner.durable);
        let mac = owner.mac(operation);
        let unavailable = || {
            error(
                ErrorCode::StorageUnavailable,
                "the ledger is unavailable; nothing was done",
            )
        };
        let store = self
            .store
            .as_mut()
            .filter(|_| durable)
            .ok_or_else(unavailable)?;
        let existing = store.control_find_key(key).map_err(|_| unavailable())?;
        if existing.is_none()
            && store
                .control_key_used(epoch, key)
                .map_err(|_| unavailable())?
        {
            // Used in this session, but older than the retention window.
            return Err(error(
                ErrorCode::Conflict,
                "this idempotency key was already used in this session",
            ));
        }
        if let Some(existing) = existing {
            if existing.epoch != epoch {
                let mut response = Response::error(
                    id,
                    &error(
                        ErrorCode::StaleEpoch,
                        "this key belongs to an earlier owner session; query the operation or use a new key",
                    ),
                );
                response.operation_id = Some(existing.operation_id);
                return Ok(response);
            }
            if existing.payload_mac != mac {
                return Err(error(
                    ErrorCode::Conflict,
                    "this idempotency key was used for a different request",
                ));
            }
            // A retransmission: report, never repeat.
            let waiting = self.control.as_ref().is_some_and(|o| {
                o.pending
                    .iter()
                    .any(|p| p.operation_id == existing.operation_id)
            });
            let status = match existing.state {
                OperationState::AwaitingApproval if !waiting => {
                    return Err(error(
                        ErrorCode::StorageUnavailable,
                        "this request can no longer be approved; use a new key",
                    ));
                }
                OperationState::AwaitingApproval => Status::ApprovalRequired,
                OperationState::Executing => Status::Accepted,
                _ => {
                    let data = self.operation_json(&existing.operation_id)?;
                    let mut response = Response::ok(id, data);
                    response.operation_id = Some(existing.operation_id);
                    return Ok(response);
                }
            };
            return Ok(Response::pending(id, status, &existing.operation_id));
        }
        if let Some(call) = operation.call_ref() {
            self.check_call(call)?;
        }
        // A grant that covers this exact request runs it without a prompt.
        if let Some(grant) = self.request_grant(request)? {
            match self.grant_allows(&grant, operation) {
                Ok(()) => {
                    let operation_id =
                        self.record(key, operation, mac, epoch, OperationState::Executing)?;
                    if let Some(store) = self.store.as_mut() {
                        let detail = format!("{} by grant {}", audit_detail(operation), grant.id);
                        let _ = store.control_audit(operation.name(), "granted", &detail);
                    }
                    if let Some(owner) = self.control.as_mut() {
                        owner
                            .granted
                            .push_back((operation_id.clone(), operation.clone()));
                    }
                    return Ok(Response::pending(id, Status::Accepted, &operation_id));
                }
                Err(denial) if mode == OwnerMode::Service => return Err(denial),
                // In the window, anything the grant does not cover asks a person.
                Err(_) => {}
            }
        }
        if mode == OwnerMode::Service {
            return Err(error(
                ErrorCode::GrantRequired,
                "the service runs only what its grant allows",
            ));
        }
        let owner = self
            .control
            .as_ref()
            .ok_or_else(|| error(ErrorCode::Internal, "control is not active"))?;
        if owner.paused_until.is_some_and(|until| now < until) {
            return Err(error(
                ErrorCode::Busy,
                "control requests are paused in the window; try again later",
            ));
        }
        if owner.pending.len() >= MAX_PENDING {
            return Err(error(
                ErrorCode::Busy,
                "too many requests are waiting for approval",
            ));
        }
        let operation_id =
            self.record(key, operation, mac, epoch, OperationState::AwaitingApproval)?;
        let store = self.store.as_mut().ok_or_else(unavailable)?;
        let _ = store.control_audit(operation.name(), "requested", &audit_detail(operation));
        let requester = pid.map_or_else(
            || "A local program".to_owned(),
            |pid| format!("A local program (process {pid})"),
        );
        if let Some(owner) = self.control.as_mut() {
            owner.pending.push(PendingApproval {
                operation_id: operation_id.clone(),
                description: describe(operation),
                requester,
                deadline: now + APPROVAL_WINDOW,
                operation: operation.clone(),
            });
        }
        Ok(Response::pending(
            id,
            Status::ApprovalRequired,
            &operation_id,
        ))
    }

    /// Write a new ledger row before anything happens; returns its id.
    fn record(
        &mut self,
        key: &str,
        operation: &Operation,
        mac: Vec<u8>,
        epoch: i64,
        state: OperationState,
    ) -> Result<String, ControlError> {
        let unavailable = || {
            error(
                ErrorCode::StorageUnavailable,
                "the ledger is unavailable; nothing was done",
            )
        };
        let operation_id = self
            .control
            .as_ref()
            .ok_or_else(|| error(ErrorCode::Internal, "control is not active"))?
            .operation_id()
            .map_err(|e| error(ErrorCode::Internal, e))?;
        self.store
            .as_mut()
            .ok_or_else(unavailable)?
            .control_insert(&OperationRecord {
                operation_id: operation_id.clone(),
                epoch,
                idempotency_key: key.to_owned(),
                operation: operation.name().to_owned(),
                payload_mac: mac,
                state,
                outcome: None,
                created_utc: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX)),
            })
            .map_err(|_| unavailable())?;
        Ok(operation_id)
    }

    /// Record the final state; the audit row names the operation only.
    fn finish(&mut self, operation_id: &str, name: &str, state: OperationState, outcome: &Value) {
        if let Some(store) = self.store.as_mut() {
            let _ = store.control_update(operation_id, state, Some(&outcome.to_string()));
            let _ = store.control_audit(name, state_name(state), "-");
        }
    }

    /// Deny every waiting request and refuse new mutations for
    /// [`crate::owner::PAUSE_AFTER_DENY_ALL`].
    pub fn control_deny_all(&mut self, backend: &dyn Backend, now: Instant) {
        let ids: Vec<String> = self
            .pending_approvals()
            .iter()
            .map(|p| p.operation_id.clone())
            .collect();
        for id in ids {
            self.control_decide(&id, false, backend, now);
        }
        if let Some(owner) = self.control.as_mut() {
            owner.paused_until = Some(now + crate::owner::PAUSE_AFTER_DENY_ALL);
        }
    }

    /// The person approved or denied a pending request.
    pub fn control_decide(
        &mut self,
        operation_id: &str,
        approve: bool,
        backend: &dyn Backend,
        now: Instant,
    ) {
        let Some(owner) = self.control.as_mut() else {
            return;
        };
        let Some(index) = owner
            .pending
            .iter()
            .position(|p| p.operation_id == operation_id)
        else {
            return;
        };
        let pending = owner.pending.remove(index);
        let name = pending.operation.name();
        if !approve {
            self.finish(
                operation_id,
                name,
                OperationState::Denied,
                &json!({"reason": "denied"}),
            );
            return;
        }
        if now > pending.deadline {
            self.finish(
                operation_id,
                name,
                OperationState::Expired,
                &json!({"reason": "expired"}),
            );
            return;
        }
        self.audit_named(name, "approved", &pending.operation);
        self.execute(operation_id, &pending.operation, backend, now);
    }

    fn audit_named(&mut self, name: &str, decision: &str, operation: &Operation) {
        if let Some(store) = self.store.as_mut() {
            let _ = store.control_audit(name, decision, &audit_detail(operation));
        }
    }

    /// Run an authorized operation once. The ledger says `executing` before
    /// anything is sent, so a crash leaves `outcome_unknown`, never a retry.
    fn execute(
        &mut self,
        operation_id: &str,
        operation: &Operation,
        backend: &dyn Backend,
        now: Instant,
    ) {
        let call = match operation.call_ref().map(|c| self.check_call(c)).transpose() {
            Ok(call) => call,
            Err(_) => {
                let outcome = json!({"reason": "the call ended or changed before approval"});
                self.finish(
                    operation_id,
                    operation.name(),
                    OperationState::Failed,
                    &outcome,
                );
                return;
            }
        };
        let persisted = self.store.as_mut().is_some_and(|s| {
            s.control_update(operation_id, OperationState::Executing, None)
                .is_ok()
        });
        if !persisted {
            // Nothing is sent without a durable `executing` record.
            self.set_notice(
                "A control request could not be recorded; nothing was done",
                now,
            );
            let outcome = json!({"reason": "the ledger is unavailable; nothing was done"});
            self.finish(
                operation_id,
                operation.name(),
                OperationState::Failed,
                &outcome,
            );
            return;
        }
        let command = match (operation, call) {
            (Operation::CallDial { target }, _) => {
                DialTarget::parse(target).ok().map(Command::Dial)
            }
            (Operation::CallAnswer(_), Some(id)) => Some(Command::Answer(id)),
            (Operation::CallReject(_), Some(id)) => Some(Command::Reject(id)),
            (Operation::CallHangup(_), Some(id)) => Some(Command::Hangup(id)),
            (Operation::CallMute { muted, .. }, Some(id)) => Some(Command::SetMute(id, *muted)),
            (Operation::CallHold { held: true, .. }, Some(id)) => Some(Command::Hold(id)),
            (Operation::CallHold { held: false, .. }, Some(id)) => Some(Command::Resume(id)),
            (Operation::CallTransfer { target, .. }, Some(id)) => DialTarget::parse(target)
                .ok()
                .map(|t| Command::Transfer(id, t)),
            (Operation::DndSet { enabled }, _) => Some(Command::SetDnd(*enabled)),
            (Operation::CallDtmf { digits, .. }, Some(id)) => {
                let keys = digits
                    .chars()
                    .filter_map(|c| DtmfKey::new(c).ok())
                    .collect();
                if let Some(owner) = self.control.as_mut() {
                    owner.dtmf.push_back(DtmfJob {
                        operation_id: operation_id.to_owned(),
                        call: id,
                        keys,
                        sent: 0,
                        next: now,
                    });
                }
                return;
            }
            _ => None,
        };
        let Some(command) = command else {
            self.finish(
                operation_id,
                operation.name(),
                OperationState::Failed,
                &json!({"reason": "invalid"}),
            );
            return;
        };
        let dnd = match operation {
            Operation::DndSet { enabled } => Some(*enabled),
            _ => None,
        };
        if self.send(backend, command, now) {
            if let Some(enabled) = dnd {
                self.dnd = enabled;
            }
            self.finish(
                operation_id,
                operation.name(),
                OperationState::Succeeded,
                &json!({"sent": true}),
            );
        } else {
            let outcome = json!({"reason": "the engine refused the command"});
            self.finish(
                operation_id,
                operation.name(),
                OperationState::Failed,
                &outcome,
            );
        }
    }

    /// Expire undecided requests, run granted operations and pace queued
    /// DTMF digits.
    pub fn control_tick(&mut self, backend: &dyn Backend, now: Instant) {
        while let Some((operation_id, operation)) =
            self.control.as_mut().and_then(|o| o.granted.pop_front())
        {
            self.execute(&operation_id, &operation, backend, now);
        }
        let expired: Vec<PendingApproval> = self.control.as_mut().map_or_else(Vec::new, |owner| {
            let (gone, kept): (Vec<_>, Vec<_>) =
                owner.pending.drain(..).partition(|p| now > p.deadline);
            owner.pending = kept;
            gone
        });
        for pending in expired {
            let outcome = json!({"reason": "expired"});
            self.finish(
                &pending.operation_id,
                pending.operation.name(),
                OperationState::Expired,
                &outcome,
            );
        }
        let Some(job) = self.control.as_mut().and_then(|o| o.dtmf.front_mut()) else {
            return;
        };
        if now < job.next {
            return;
        }
        let (operation_id, call) = (job.operation_id.clone(), job.call);
        // A stalled queue (window not updated for a while) must not resume
        // into another context: give up and report how many digits went out.
        if now.saturating_duration_since(job.next) > STALLED_DTMF {
            let sent = self
                .control
                .as_mut()
                .and_then(|o| o.dtmf.pop_front())
                .map_or(0, |j| j.sent);
            let outcome = json!({"reason": "stalled", "sent": sent});
            self.finish(&operation_id, "call_dtmf", OperationState::Failed, &outcome);
            return;
        }
        let alive = self
            .call
            .as_ref()
            .is_some_and(|c| c.snapshot.id == call && c.snapshot.phase == CallPhase::Established);
        let key = self
            .control
            .as_mut()
            .and_then(|o| o.dtmf.front_mut())
            .and_then(|j| j.keys.pop_front());
        match key {
            Some(key) if alive && self.send(backend, Command::Dtmf(call, key), now) => {
                if let Some(job) = self.control.as_mut().and_then(|o| o.dtmf.front_mut()) {
                    job.sent += 1;
                    job.next = now + DTMF_GAP;
                }
            }
            Some(_) => {
                let sent = self
                    .control
                    .as_mut()
                    .and_then(|o| o.dtmf.pop_front())
                    .map_or(0, |j| j.sent);
                let outcome = json!({"reason": "the call ended", "sent": sent});
                self.finish(&operation_id, "call_dtmf", OperationState::Failed, &outcome);
            }
            None => {
                let sent = self
                    .control
                    .as_mut()
                    .and_then(|o| o.dtmf.pop_front())
                    .map_or(0, |j| j.sent);
                self.finish(
                    &operation_id,
                    "call_dtmf",
                    OperationState::Succeeded,
                    &json!({"sent": sent}),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::owner::ControlOwner;
    use crate::ui::UiSettings;
    use rust_microsip_core::{CallSnapshot, Event, Notice};
    use rust_microsip_storage::Store;
    use std::cell::RefCell;
    use std::time::Duration;

    #[derive(Default)]
    struct Engine(RefCell<Vec<String>>);
    impl Backend for Engine {
        fn send(&self, command: Command) -> Result<(), Notice> {
            let text = match &command {
                Command::Dial(t) => format!("Dial({t})"),
                Command::Dtmf(id, k) => format!("Dtmf({}, {})", id.0, k.as_char()),
                other => format!("{other:?}"),
            };
            self.0.borrow_mut().push(text);
            Ok(())
        }
        fn poll(&self) -> Vec<Event> {
            Vec::new()
        }
    }

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn owner_state(mode: OwnerMode, durable: bool) -> Result<UiState, Box<dyn std::error::Error>> {
        let mut st = UiState::new(UiSettings::default());
        let mut store = Store::open_in_memory()?;
        let epoch = store.control_begin_epoch()?;
        st.attach_store(Some(store), None, true, Instant::now());
        st.attach_control(ControlOwner::new(mode, epoch, durable)?);
        Ok(st)
    }

    fn request(key: Option<&str>, operation: Operation) -> Request {
        Request {
            schema_version: 1,
            request_id: "r1".into(),
            idempotency_key: key.map(Into::into),
            operation,
            capability: None,
        }
    }

    fn dial(target: &str) -> Operation {
        Operation::CallDial {
            target: target.into(),
        }
    }

    fn established(st: &mut UiState, id: u64, version: u64) {
        st.apply_event(
            Event::Call(CallSnapshot {
                id: CallId(id),
                direction: Direction::Outgoing,
                remote: "Ana 1002".into(),
                phase: CallPhase::Established,
                muted: false,
                secure: true,
                transfer: None,
                version,
            }),
            Instant::now(),
        );
    }

    fn code(response: &Response) -> Option<ErrorCode> {
        response.error.as_ref().map(|e| e.code)
    }

    #[test]
    fn status_is_masked_and_personal_reads_need_a_grant() -> TestResult {
        let mut st = owner_state(OwnerMode::Gui, true)?;
        established(&mut st, 3, 2);
        let now = Instant::now();
        let status = st.control_response(&request(None, Operation::StatusGet), None, now);
        let text = serde_json::to_string(&status)?;
        assert!(!text.contains("1002") && !text.contains("Ana"), "{text}");
        assert!(text.contains("\"call_id\":3") && text.contains("\"phase\":\"established\""));
        for read in [
            Operation::ConfigShow,
            Operation::DevicesList,
            Operation::ContactsSearch {
                query: String::new(),
                cursor: None,
                limit: None,
            },
            Operation::HistoryList {
                filter: None,
                cursor: None,
                limit: None,
            },
        ] {
            let response = st.control_response(&request(None, read), None, now);
            assert_eq!(code(&response), Some(ErrorCode::GrantRequired));
        }
        Ok(())
    }

    #[test]
    fn a_person_approves_exactly_once_and_replays_never_repeat() -> TestResult {
        let mut st = owner_state(OwnerMode::Gui, true)?;
        let engine = Engine::default();
        let now = Instant::now();
        let first = st.control_response(&request(Some("k1"), dial("1002")), Some(42), now);
        assert_eq!(first.status, Status::ApprovalRequired);
        let operation_id = first.operation_id.clone().ok_or("no id")?;
        assert_eq!(st.pending_approvals().len(), 1);
        assert!(st.pending_approvals()[0].requester.contains("42"));
        assert!(
            engine.0.borrow().is_empty(),
            "nothing happens before approval"
        );
        // A retransmission returns the same pending operation.
        let again = st.control_response(&request(Some("k1"), dial("1002")), None, now);
        assert_eq!(again.operation_id.as_deref(), Some(operation_id.as_str()));
        assert_eq!(st.pending_approvals().len(), 1);
        // The same key for another destination is a conflict.
        let other = st.control_response(&request(Some("k1"), dial("1003")), None, now);
        assert_eq!(code(&other), Some(ErrorCode::Conflict));
        st.control_decide(&operation_id, true, &engine, now);
        assert_eq!(engine.0.take(), ["Dial(1002)"]);
        // Replays after execution report the outcome and send nothing.
        let replay = st.control_response(&request(Some("k1"), dial("1002")), None, now);
        assert_eq!(replay.status, Status::Ok);
        assert!(engine.0.borrow().is_empty());
        let get = st.control_response(
            &request(
                None,
                Operation::OperationGet {
                    operation_id: operation_id.clone(),
                },
            ),
            None,
            now,
        );
        let data = get.data.ok_or("no data")?;
        assert_eq!(data["state"], "succeeded");
        assert_eq!(data["current_epoch"], true);
        // Deciding twice does nothing.
        st.control_decide(&operation_id, true, &engine, now);
        assert!(engine.0.borrow().is_empty());
        Ok(())
    }

    #[test]
    fn denial_and_expiry_have_no_effect() -> TestResult {
        let mut st = owner_state(OwnerMode::Gui, true)?;
        let engine = Engine::default();
        let now = Instant::now();
        let denied = st
            .control_response(&request(Some("k-deny"), dial("1002")), None, now)
            .operation_id
            .ok_or("no id")?;
        st.control_decide(&denied, false, &engine, now);
        let expired = st
            .control_response(
                &request(Some("k-late"), Operation::DndSet { enabled: true }),
                None,
                now,
            )
            .operation_id
            .ok_or("no id")?;
        st.control_tick(&engine, now + APPROVAL_WINDOW + Duration::from_secs(1));
        assert!(st.pending_approvals().is_empty());
        assert!(engine.0.borrow().is_empty());
        for (id, state) in [(denied, "denied"), (expired, "expired")] {
            let get = st.control_response(
                &request(None, Operation::OperationGet { operation_id: id }),
                None,
                now,
            );
            assert_eq!(get.data.ok_or("no data")?["state"], state);
        }
        Ok(())
    }

    #[test]
    fn calls_must_match_and_modes_and_storage_gate_mutations() -> TestResult {
        let mut st = owner_state(OwnerMode::Gui, true)?;
        established(&mut st, 3, 5);
        let now = Instant::now();
        let stale = Operation::CallHangup(CallRef {
            call_id: 3,
            expected_version: 4,
        });
        let response = st.control_response(&request(Some("k-v"), stale), None, now);
        assert_eq!(code(&response), Some(ErrorCode::NotFound));
        assert!(st.pending_approvals().is_empty(), "nothing recorded");
        let mut service = owner_state(OwnerMode::Service, true)?;
        let response = service.control_response(&request(Some("k-s"), dial("1002")), None, now);
        assert_eq!(code(&response), Some(ErrorCode::GrantRequired));
        assert!(service.pending_approvals().is_empty());
        let mut volatile = owner_state(OwnerMode::Gui, false)?;
        let response = volatile.control_response(&request(Some("k-m"), dial("1002")), None, now);
        assert_eq!(code(&response), Some(ErrorCode::StorageUnavailable));
        Ok(())
    }

    #[test]
    fn keys_from_an_earlier_epoch_are_stale_and_never_rerun() -> TestResult {
        let mut st = owner_state(OwnerMode::Gui, true)?;
        let now = Instant::now();
        let old = st
            .control_response(&request(Some("k-old"), dial("1002")), None, now)
            .operation_id
            .ok_or("no id")?;
        // Restart: new epoch on the same store, new in-memory key.
        let epoch = st.store.as_mut().ok_or("store")?.control_begin_epoch()?;
        st.attach_control(ControlOwner::new(OwnerMode::Gui, epoch, true)?);
        let response = st.control_response(&request(Some("k-old"), dial("1002")), None, now);
        assert_eq!(code(&response), Some(ErrorCode::StaleEpoch));
        assert_eq!(response.operation_id.as_deref(), Some(old.as_str()));
        let get = st.control_response(
            &request(None, Operation::OperationGet { operation_id: old }),
            None,
            now,
        );
        let data = get.data.ok_or("no data")?;
        assert_eq!(data["state"], "expired");
        assert_eq!(data["current_epoch"], false);
        Ok(())
    }

    #[test]
    fn dtmf_is_paced_and_digits_are_not_stored() -> TestResult {
        let mut st = owner_state(OwnerMode::Gui, true)?;
        established(&mut st, 7, 1);
        let engine = Engine::default();
        let now = Instant::now();
        let operation = Operation::CallDtmf {
            call_id: 7,
            expected_version: 1,
            digits: "90#".into(),
        };
        let id = st
            .control_response(&request(Some("k-d"), operation), None, now)
            .operation_id
            .ok_or("no id")?;
        st.control_decide(&id, true, &engine, now);
        assert!(st.control_busy());
        st.control_tick(&engine, now);
        st.control_tick(&engine, now + Duration::from_millis(50));
        assert_eq!(engine.0.borrow().len(), 1, "one digit per gap");
        for step in 1..=3 {
            st.control_tick(&engine, now + DTMF_GAP * step);
        }
        assert_eq!(engine.0.take(), ["Dtmf(7, 9)", "Dtmf(7, 0)", "Dtmf(7, #)"]);
        assert!(!st.control_busy());
        let record = st
            .store
            .as_ref()
            .ok_or("store")?
            .control_get(&id)?
            .ok_or("record")?;
        assert_eq!(record.state, OperationState::Succeeded);
        let stored = format!("{record:?}");
        assert!(!stored.contains("90#"), "{stored}");
        Ok(())
    }

    #[test]
    fn stalled_dtmf_is_abandoned_and_old_keys_conflict() -> TestResult {
        let mut st = owner_state(OwnerMode::Gui, true)?;
        established(&mut st, 7, 1);
        let engine = Engine::default();
        let now = Instant::now();
        let operation = Operation::CallDtmf {
            call_id: 7,
            expected_version: 1,
            digits: "123".into(),
        };
        let id = st
            .control_response(&request(Some("k-stall"), operation), None, now)
            .operation_id
            .ok_or("no id")?;
        st.control_decide(&id, true, &engine, now);
        st.control_tick(&engine, now);
        // The window was not updated for two seconds: the rest is dropped.
        st.control_tick(&engine, now + Duration::from_secs(2));
        assert_eq!(engine.0.take(), ["Dtmf(7, 1)"]);
        assert!(!st.control_busy());
        let record = st
            .store
            .as_ref()
            .ok_or("store")?
            .control_get(&id)?
            .ok_or("record")?;
        assert_eq!(record.state, OperationState::Failed);
        assert_eq!(
            record.outcome.as_deref(),
            Some("{\"reason\":\"stalled\",\"sent\":1}")
        );
        // A key recorded in this session but past retention is a conflict,
        // not a storage error.
        let epoch = st.control.as_ref().ok_or("owner")?.epoch;
        st.store
            .as_mut()
            .ok_or("store")?
            .control_insert(&OperationRecord {
                operation_id: "op-old".into(),
                epoch,
                idempotency_key: "k-ancient".into(),
                operation: "call_dial".into(),
                payload_mac: vec![0; 32],
                state: OperationState::Succeeded,
                outcome: None,
                created_utc: 0,
            })?;
        let response = st.control_response(&request(Some("k-ancient"), dial("1002")), None, now);
        assert_eq!(code(&response), Some(ErrorCode::Conflict));
        Ok(())
    }

    #[test]
    fn deny_all_pauses_control_and_orphans_are_not_re_promised() -> TestResult {
        let mut st = owner_state(OwnerMode::Gui, true)?;
        let engine = Engine::default();
        let now = Instant::now();
        for key in ["k-a", "k-b"] {
            st.control_response(&request(Some(key), dial("1002")), None, now);
        }
        st.control_deny_all(&engine, now);
        assert!(st.pending_approvals().is_empty());
        assert!(engine.0.borrow().is_empty());
        let paused = st.control_response(&request(Some("k-c"), dial("1002")), None, now);
        assert_eq!(code(&paused), Some(ErrorCode::Busy));
        let later = now + crate::owner::PAUSE_AFTER_DENY_ALL + Duration::from_secs(1);
        let resumed = st.control_response(&request(Some("k-d"), dial("1002")), None, later);
        assert_eq!(resumed.status, Status::ApprovalRequired);
        // A ledger row waiting for approval without a prompt (e.g. the prompt
        // state was lost) is reported, never shown as still pending.
        let id = resumed.operation_id.ok_or("no id")?;
        if let Some(owner) = st.control.as_mut() {
            owner.pending.retain(|p| p.operation_id != id);
        }
        let orphan = st.control_response(&request(Some("k-d"), dial("1002")), None, later);
        assert_eq!(code(&orphan), Some(ErrorCode::StorageUnavailable));
        Ok(())
    }

    fn grant_spec(scopes: &[&str], destinations: &[&str]) -> super::super::grants::GrantSpec {
        super::super::grants::GrantSpec {
            scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
            destinations: destinations.iter().map(|s| (*s).to_owned()).collect(),
            incoming: Vec::new(),
            minutes: 10,
            quota: 1,
        }
    }

    fn with_capability(mut request: Request, capability: &str) -> Request {
        request.capability = Some(capability.to_owned());
        request
    }

    #[test]
    fn grants_run_only_what_they_cover_and_never_prompt() -> TestResult {
        let mut st = owner_state(OwnerMode::Gui, true)?;
        let engine = Engine::default();
        let now = Instant::now();
        let issued = st.issue_grant(
            &grant_spec(&["control:dial", "read:history"], &["1002"]),
            false,
        )?;
        let cap = issued.capability.as_str();
        // Covered: runs on the next tick without a prompt.
        let ok = st.control_response(
            &with_capability(request(Some("g-1"), dial("1002")), cap),
            None,
            now,
        );
        assert_eq!(ok.status, Status::Accepted);
        assert!(st.pending_approvals().is_empty());
        st.control_tick(&engine, now);
        assert_eq!(engine.0.take(), ["Dial(1002)"]);
        // Quota of one call: the next dial falls back to a person in the window.
        let again = st.control_response(
            &with_capability(request(Some("g-2"), dial("1002")), cap),
            None,
            now,
        );
        assert_eq!(again.status, Status::ApprovalRequired);
        // Destination outside the grant: a prompt, never a silent call.
        let other = st.control_response(
            &with_capability(request(Some("g-3"), dial("1003")), cap),
            None,
            now,
        );
        assert_eq!(other.status, Status::ApprovalRequired);
        assert!(engine.0.borrow().is_empty());
        // Reads within scope work; others do not.
        let history = st.control_response(
            &with_capability(
                request(
                    None,
                    Operation::HistoryList {
                        filter: None,
                        cursor: None,
                        limit: Some(5),
                    },
                ),
                cap,
            ),
            None,
            now,
        );
        assert_eq!(history.status, Status::Ok);
        let contacts = st.control_response(
            &with_capability(
                request(
                    None,
                    Operation::ContactsSearch {
                        query: String::new(),
                        cursor: None,
                        limit: None,
                    },
                ),
                cap,
            ),
            None,
            now,
        );
        assert_eq!(code(&contacts), Some(ErrorCode::GrantRequired));
        // Unknown and revoked capabilities are refused outright.
        let fake = st.control_response(
            &with_capability(request(None, Operation::ConfigShow), &"0".repeat(64)),
            None,
            now,
        );
        assert_eq!(code(&fake), Some(ErrorCode::GrantRequired));
        st.revoke_grant(&issued.id);
        let revoked = st.control_response(
            &with_capability(
                request(
                    None,
                    Operation::HistoryList {
                        filter: None,
                        cursor: None,
                        limit: None,
                    },
                ),
                cap,
            ),
            None,
            now,
        );
        assert_eq!(code(&revoked), Some(ErrorCode::GrantRequired));
        Ok(())
    }

    #[test]
    fn the_service_runs_only_granted_requests() -> TestResult {
        let mut st = owner_state(OwnerMode::Service, true)?;
        let engine = Engine::default();
        let now = Instant::now();
        let issued = st.issue_grant(&grant_spec(&["control:dnd"], &[]), false)?;
        let on = st.control_response(
            &with_capability(
                request(Some("s-1"), Operation::DndSet { enabled: true }),
                &issued.capability,
            ),
            None,
            now,
        );
        assert_eq!(on.status, Status::Accepted);
        st.control_tick(&engine, now);
        assert_eq!(engine.0.take(), ["SetDnd(true)"]);
        let dial = st.control_response(
            &with_capability(request(Some("s-2"), dial("1002")), &issued.capability),
            None,
            now,
        );
        assert_eq!(code(&dial), Some(ErrorCode::GrantRequired));
        assert!(
            st.pending_approvals().is_empty(),
            "the service never prompts"
        );
        // Bad grant specs are refused without storing anything.
        assert!(
            st.issue_grant(&grant_spec(&["control:shell"], &[]), false)
                .is_err()
        );
        let mut long = grant_spec(&["control:dnd"], &[]);
        long.minutes = 61;
        assert!(st.issue_grant(&long, false).is_err());
        assert!(
            st.issue_grant(&grant_spec(&["control:dial"], &["10 02"]), false)
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn the_audit_log_is_cleared_only_after_confirmation() -> TestResult {
        let mut st = owner_state(OwnerMode::Gui, true)?;
        let engine = Engine::default();
        let now = Instant::now();
        st.control_response(&request(Some("k-a"), dial("1002")), None, now);
        let rows = |st: &UiState| {
            st.store
                .as_ref()
                .map_or(0, |s| s.control_audit_rows(10).map_or(0, |r| r.len()))
        };
        assert!(rows(&st) >= 1);
        let mut save =
            |_: &crate::ui::UiSettings, _: Option<&mut rust_microsip_storage::Store>| Ok(());
        st.dispatch(super::super::Action::ClearAudit, &engine, &mut save, now);
        assert!(rows(&st) >= 1, "nothing deleted before confirming");
        st.dispatch(super::super::Action::Confirm, &engine, &mut save, now);
        assert_eq!(rows(&st), 0);
        Ok(())
    }
}
