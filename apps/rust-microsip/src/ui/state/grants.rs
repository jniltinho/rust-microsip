//! Grant-authorized control (AG-04): reads of personal data and mutations
//! without a per-request prompt, only within what a person granted in the
//! window — scope, exact destinations or incoming callers, call quota,
//! expiry and owner epoch. Everything else falls back to the prompt (window)
//! or `grant_required` (service).

use super::UiState;
use crate::owner::{capability_hash, scope_for};
use rust_microsip_control::{ControlError, ErrorCode, HistoryFilter, MAX_PAGE, Operation, Request};
use rust_microsip_core::{CallPhase, Direction};
use rust_microsip_storage::{Grant, HistoryFilter as StoreFilter};
use serde_json::{Value, json};

/// Offset cursors (`o:N`), bounded so a client cannot page forever.
const MAX_OFFSET: u32 = 10_000;

fn denied(message: &str) -> ControlError {
    ControlError::new(ErrorCode::GrantRequired, message)
}

fn page(cursor: Option<&String>, limit: Option<u32>) -> Result<(u32, u32), ControlError> {
    let offset = match cursor {
        None => 0,
        Some(c) => c
            .strip_prefix("o:")
            .and_then(|n| n.parse::<u32>().ok())
            .filter(|n| *n <= MAX_OFFSET)
            .ok_or_else(|| ControlError::new(ErrorCode::Validation, "cursor is not valid"))?,
    };
    Ok((offset, limit.unwrap_or(20).min(MAX_PAGE)))
}

fn next_cursor(offset: u32, limit: u32, fetched: usize) -> Value {
    if fetched > limit as usize {
        json!(format!("o:{}", offset + limit))
    } else {
        Value::Null
    }
}

impl UiState {
    /// The active grant for the request's capability, if any.
    pub(super) fn request_grant(&self, request: &Request) -> Result<Option<Grant>, ControlError> {
        let Some(capability) = &request.capability else {
            return Ok(None);
        };
        let (Some(store), Some(owner)) = (self.store.as_ref(), self.control.as_ref()) else {
            return Err(denied("grants are unavailable"));
        };
        let grant = store
            .grant_active(&capability_hash(capability), owner.epoch)
            .map_err(|_| {
                ControlError::new(
                    ErrorCode::StorageUnavailable,
                    "the grant store is unavailable",
                )
            })?;
        grant
            .map(Some)
            .ok_or_else(|| denied("the grant is unknown, expired, revoked or from another session"))
    }

    /// A read of personal data, allowed only by a grant with its scope.
    pub(super) fn granted_read(&self, request: &Request) -> Result<Value, ControlError> {
        let grant = self
            .request_grant(request)?
            .ok_or_else(|| denied("this read needs a read grant issued in the window"))?;
        let scope = scope_for(&request.operation).unwrap_or_default();
        if !grant.scopes.iter().any(|s| s == scope) {
            return Err(denied("the grant does not cover this read"));
        }
        let store = self.store.as_ref().ok_or_else(|| {
            ControlError::new(ErrorCode::StorageUnavailable, "storage is unavailable")
        })?;
        let unavailable =
            |_| ControlError::new(ErrorCode::StorageUnavailable, "storage is unavailable");
        Ok(match &request.operation {
            Operation::ContactsSearch {
                query,
                cursor,
                limit,
            } => {
                let (offset, limit) = page(cursor.as_ref(), *limit)?;
                let rows = store
                    .search_contacts(query, offset + limit + 1)
                    .map_err(unavailable)?;
                let items: Vec<Value> = rows
                    .iter()
                    .skip(offset as usize)
                    .take(limit as usize)
                    .map(|c| {
                        json!({
                            "name": c.name,
                            "favorite": c.favorite,
                            "numbers": c.numbers.iter().map(|n| json!({"number": n.number, "label": n.label})).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                json!({"items": items, "next_cursor": next_cursor(offset, limit, rows.len().saturating_sub(offset as usize))})
            }
            Operation::HistoryList {
                filter,
                cursor,
                limit,
            } => {
                let (offset, limit) = page(cursor.as_ref(), *limit)?;
                let filter = match filter.unwrap_or_default() {
                    HistoryFilter::All => StoreFilter::All,
                    HistoryFilter::Incoming => StoreFilter::Incoming,
                    HistoryFilter::Outgoing => StoreFilter::Outgoing,
                    HistoryFilter::Missed => StoreFilter::Missed,
                };
                let rows = store
                    .recent_calls(offset + limit + 1, filter, None)
                    .map_err(unavailable)?;
                let items: Vec<Value> = rows
                    .iter()
                    .skip(offset as usize)
                    .take(limit as usize)
                    .map(|h| {
                        let c = &h.call;
                        json!({
                            "direction": if c.direction == Direction::Incoming { "incoming" } else { "outgoing" },
                            "remote": c.remote,
                            "outcome": format!("{:?}", c.outcome).to_ascii_lowercase(),
                            "started_utc": c.started_utc,
                            "duration_ms": c.duration_ms,
                            "secure": c.secure,
                        })
                    })
                    .collect();
                json!({"items": items, "next_cursor": next_cursor(offset, limit, rows.len().saturating_sub(offset as usize))})
            }
            Operation::DevicesList => json!({
                "input": self.audio_input,
                "output": self.audio_output,
                "available": self.audio_devices.as_ref().map(|d| json!({
                    "inputs": d.inputs.iter().map(|i| i.name.clone()).collect::<Vec<_>>(),
                    "outputs": d.outputs.iter().map(|o| o.name.clone()).collect::<Vec<_>>(),
                })),
            }),
            Operation::ConfigShow => json!({
                "account": self.account.as_ref().map(|a| json!({
                    "username": a.username,
                    "domain": a.domain,
                    "server": a.server,
                    "port": a.port,
                    "transport": format!("{:?}", a.transport).to_ascii_lowercase(),
                    "media": format!("{:?}", a.media).to_ascii_lowercase(),
                    "expires": a.expires,
                })),
                "dnd": self.dnd,
                "theme": format!("{:?}", self.theme).to_ascii_lowercase(),
            }),
            _ => return Err(denied("not a read")),
        })
    }

    /// Whether `grant` authorizes this mutation right now; consumes one call
    /// of the quota for dialing and answering.
    pub(super) fn grant_allows(
        &mut self,
        grant: &Grant,
        operation: &Operation,
    ) -> Result<(), ControlError> {
        let scope = scope_for(operation).unwrap_or_default();
        if !grant.scopes.iter().any(|s| s == scope) {
            return Err(denied("the grant does not cover this operation"));
        }
        match operation {
            Operation::CallDial { target } | Operation::CallTransfer { target, .. }
                if !grant.destinations.iter().any(|d| d == target) =>
            {
                return Err(denied("the destination is not in the grant"));
            }
            Operation::CallAnswer(_) | Operation::CallReject(_) => {
                let caller = self
                    .call
                    .as_ref()
                    .filter(|c| {
                        c.snapshot.direction == Direction::Incoming
                            && c.snapshot.phase == CallPhase::Ringing
                    })
                    .map(|c| c.snapshot.remote.clone())
                    .ok_or_else(|| {
                        ControlError::new(ErrorCode::InvalidState, "no call is ringing")
                    })?;
                if !grant.incoming.contains(&caller) {
                    return Err(denied("the caller is not in the grant"));
                }
            }
            _ => {}
        }
        if matches!(
            operation,
            Operation::CallDial { .. } | Operation::CallAnswer(_)
        ) {
            let store = self.store.as_mut().ok_or_else(|| {
                ControlError::new(ErrorCode::StorageUnavailable, "storage is unavailable")
            })?;
            if !store
                .grant_use_call(&grant.id)
                .map_err(|_| denied("the grant store is unavailable"))?
            {
                return Err(denied("the grant's call quota is used up"));
            }
        }
        Ok(())
    }
}

/// What a person grants in the window.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GrantSpec {
    /// Scopes from [`crate::owner::SCOPES`].
    pub scopes: Vec<String>,
    /// Exact numbers allowed for dialing and transfers.
    pub destinations: Vec<String>,
    /// Exact incoming callers that may be answered or rejected.
    pub incoming: Vec<String>,
    /// Validity in minutes (1 to 60).
    pub minutes: u32,
    /// Calls (dial or answer) allowed in total.
    pub quota: u32,
}

/// A grant as issued: the capability is shown or written once, never stored.
#[derive(Clone, PartialEq, Eq)]
pub struct IssuedGrant {
    /// Public id.
    pub id: String,
    /// Secret capability (64 hex characters).
    pub capability: String,
    /// Single-use id when exported for the headless service.
    pub bootstrap_id: Option<String>,
}

impl std::fmt::Debug for IssuedGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IssuedGrant")
            .field("id", &self.id)
            .field("capability", &"[REDACTED]")
            .field("bootstrap_id", &self.bootstrap_id)
            .finish()
    }
}

impl UiState {
    /// Issue a grant for this session (`bootstrap: false`) or a single-use
    /// bootstrap for the headless service (`bootstrap: true`).
    ///
    /// # Errors
    /// A user-safe reason (no grant was stored).
    pub fn issue_grant(
        &mut self,
        spec: &GrantSpec,
        bootstrap: bool,
    ) -> Result<IssuedGrant, String> {
        use rust_microsip_core::DialTarget;
        if spec.scopes.is_empty()
            || spec
                .scopes
                .iter()
                .any(|s| !crate::owner::SCOPES.contains(&s.as_str()))
        {
            return Err("choose at least one known permission".into());
        }
        if !(1..=60).contains(&spec.minutes) || spec.quota > 100 {
            return Err("validity must be 1 to 60 minutes and the call limit at most 100".into());
        }
        for number in spec.destinations.iter().chain(&spec.incoming) {
            if DialTarget::parse(number)
                .map(|t| t.as_str() != number)
                .unwrap_or(true)
            {
                return Err("every number must be a valid number or SIP user".into());
            }
        }
        let owner = self.control.as_ref().ok_or("control is not active")?;
        if !owner.durable {
            return Err("grants need the settings database".into());
        }
        let capability = owner.new_capability()?;
        let id = owner.operation_id()?.replacen("op-", "grant-", 1);
        let bootstrap_id = if bootstrap {
            Some(owner.operation_id()?.replacen("op-", "boot-", 1))
        } else {
            None
        };
        let grant = Grant {
            id: id.clone(),
            scopes: spec.scopes.clone(),
            destinations: spec.destinations.clone(),
            incoming: spec.incoming.clone(),
            expires_utc: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
                + i64::from(spec.minutes) * 60,
            quota: spec.quota,
            used: 0,
            epoch: if bootstrap { None } else { Some(owner.epoch) },
            bootstrap_id: bootstrap_id.clone(),
            revoked: false,
        };
        let hash = capability_hash(&capability);
        let store = self.store.as_mut().ok_or("storage is unavailable")?;
        store
            .grant_insert(&grant, &hash)
            .map_err(|_| "the grant could not be stored".to_owned())?;
        let detail = format!("{} scopes, {} min", grant.scopes.len(), spec.minutes);
        let _ = store.control_audit(
            "grant",
            if bootstrap {
                "bootstrap issued"
            } else {
                "issued"
            },
            &detail,
        );
        Ok(IssuedGrant {
            id,
            capability,
            bootstrap_id,
        })
    }

    /// Grants that can still be used or consumed, for the window's list.
    #[must_use]
    pub fn listed_grants(&self) -> Vec<Grant> {
        match (self.store.as_ref(), self.control.as_ref()) {
            (Some(store), Some(owner)) => store.grants_listed(owner.epoch).unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// Revoke a grant now.
    pub fn revoke_grant(&mut self, id: &str) {
        if let Some(store) = self.store.as_mut()
            && store.grant_revoke(id).is_ok()
        {
            let _ = store.control_audit("grant", "revoked", "-");
        }
    }
}

/// The "Agent access" form in Settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantForm {
    /// Checked scopes, parallel to [`crate::owner::SCOPES`].
    pub scopes: [bool; crate::owner::SCOPES.len()],
    /// Comma-separated numbers it may call or transfer to.
    pub destinations: String,
    /// Comma-separated callers it may answer or reject.
    pub incoming: String,
    /// Validity, minutes.
    pub minutes: u32,
    /// Calls allowed.
    pub quota: u32,
    /// Where the last capability was written (shown once).
    pub written: Option<String>,
}
impl Default for GrantForm {
    fn default() -> Self {
        Self {
            scopes: [false; crate::owner::SCOPES.len()],
            destinations: String::new(),
            incoming: String::new(),
            minutes: 15,
            quota: 1,
            written: None,
        }
    }
}
impl GrantForm {
    /// The spec this form describes.
    #[must_use]
    pub fn spec(&self) -> GrantSpec {
        let list = |text: &str| -> Vec<String> {
            text.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        };
        GrantSpec {
            scopes: crate::owner::SCOPES
                .iter()
                .zip(self.scopes)
                .filter(|(_, on)| *on)
                .map(|(s, _)| (*s).to_owned())
                .collect(),
            destinations: list(&self.destinations),
            incoming: list(&self.incoming),
            minutes: self.minutes,
            quota: self.quota,
        }
    }
}

/// Write `content` to a new file `name` in `dir` (created 0700) with mode
/// 0600; never overwrites. Returns the path.
///
/// # Errors
/// I/O failures, including an existing file.
pub fn write_private(
    dir: &std::path::Path,
    name: &str,
    content: &str,
) -> std::io::Result<std::path::PathBuf> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let meta = std::fs::symlink_metadata(dir)?;
    let uid = std::fs::metadata("/proc/self")?.uid();
    if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "directory is not private",
        ));
    }
    let path = dir.join(name);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    Ok(path)
}

impl UiState {
    /// Issue a grant from the form and write its capability file: for this
    /// session under the private runtime directory, or as a bootstrap for
    /// the headless service under the private configuration directory.
    pub fn create_grant_file(&mut self, bootstrap: bool, now: std::time::Instant) {
        let dir = if bootstrap {
            self.bootstrap_dir.clone()
        } else {
            self.grant_dir.clone()
        };
        let Some(dir) = dir else {
            return self.set_notice("No private directory is available for grant files", now);
        };
        let spec = self.grant_form.spec();
        let issued = match self.issue_grant(&spec, bootstrap) {
            Ok(issued) => issued,
            Err(reason) => return self.set_notice(reason, now),
        };
        let (name, content) = match &issued.bootstrap_id {
            Some(boot) => (
                format!("{}.json", issued.id),
                format!(
                    "{{\"bootstrap_id\":\"{boot}\",\"capability\":\"{}\"}}\n",
                    issued.capability
                ),
            ),
            None => (
                format!("{}.grant", issued.id),
                format!("{}\n", issued.capability),
            ),
        };
        match write_private(&dir, &name, &content) {
            Ok(path) => {
                self.grant_form.written = Some(path.display().to_string());
                self.set_notice("Grant created; pass its file with --grant-file", now);
            }
            Err(_) => {
                // A grant whose secret could not be delivered must not stay usable.
                self.revoke_grant(&issued.id);
                self.set_notice(
                    "The grant file could not be written; the grant was revoked",
                    now,
                );
            }
        }
    }
}
