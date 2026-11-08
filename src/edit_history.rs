//! Session-bound edit audit reports and chronological logical actions.
//! Reports are descriptive; only the controller's current-session state may undo.

use std::time::SystemTime;

use crate::game_version::GameBuild;
pub use crate::undo::UndoEntry;

pub type ActionId = u64;
pub type SessionId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EntityKind {
    Player,
    Staff,
    Club,
    HumanManager,
}

impl EntityKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Player => "Player",
            Self::Staff => "Staff",
            Self::Club => "Club",
            Self::HumanManager => "Human manager",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EntityRef {
    pub kind: EntityKind,
    pub build: GameBuild,
    pub address: u64,
    pub index: usize,
    pub uid: Option<u32>,
    pub name: String,
}

/// Explicit grouping within a single submission, never a time/frame heuristic.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EditGroup {
    pub key: String,
    pub label: String,
    pub before: String,
    pub after: String,
}

impl EditGroup {
    pub fn new(
        key: impl Into<String>,
        label: impl Into<String>,
        before: impl ToString,
        after: impl ToString,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            before: before.to_string(),
            after: after.to_string(),
        }
    }
}

/// Human values come from the producing widget, not parsed description strings.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EditPresentation {
    pub field: String,
    pub before: String,
    pub after: String,
    pub group: Option<EditGroup>,
}

impl EditPresentation {
    pub fn new(field: impl Into<String>, before: impl ToString, after: impl ToString) -> Self {
        Self {
            field: field.into(),
            before: before.to_string(),
            after: after.to_string(),
            group: None,
        }
    }

    pub fn in_group(mut self, group: EditGroup) -> Self {
        self.group = Some(group);
        self
    }
}

#[derive(Clone)]
pub struct ChangeDraft {
    pub entry: UndoEntry,
    pub presentation: EditPresentation,
}

#[derive(Clone)]
pub struct ActionDraft {
    pub label: String,
    pub before: String,
    pub after: String,
    pub changes: Vec<ChangeDraft>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Operation {
    Apply,
    Undo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AttemptOutcome {
    Pending,
    Confirmed,
    Refused,
    Unconfirmed,
    Interrupted,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AttemptReport {
    pub operation: Operation,
    pub started_at: SystemTime,
    pub completed_at: Option<SystemTime>,
    pub outcome: AttemptOutcome,
    pub message: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ChangeReport {
    pub field: String,
    pub before: String,
    pub after: String,
    pub address: u64,
    pub old_raw: Vec<u8>,
    pub new_raw: Vec<u8>,
    pub attempts: Vec<AttemptReport>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ActionStatus {
    Applying,
    Applied,
    PartiallyApplied,
    Refused,
    Unconfirmed,
    Undoing,
    Undone,
    PartiallyUndone,
    UndoRefused,
    UndoUnconfirmed,
}

impl ActionStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Applying => "Applying",
            Self::Applied => "Applied",
            Self::PartiallyApplied => "Partially applied",
            Self::Refused => "Refused",
            Self::Unconfirmed => "Unconfirmed",
            Self::Undoing => "Undoing",
            Self::Undone => "Undone",
            Self::PartiallyUndone => "Partially undone",
            Self::UndoRefused => "Undo refused",
            Self::UndoUnconfirmed => "Undo unconfirmed",
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ActionReport {
    pub id: ActionId,
    pub session_id: SessionId,
    /// PID of the game process the action was written to; used to revive
    /// undo payloads after an editor restart when the same game still runs.
    #[serde(default)]
    pub pid: u32,
    /// Loaded save's name when detected (None for legacy entries saved
    /// before save tracking, or when detection failed).
    #[serde(default)]
    pub save_name: Option<String>,
    pub entity: EntityRef,
    pub label: String,
    pub before: String,
    pub after: String,
    pub submitted_at: SystemTime,
    pub updated_at: SystemTime,
    pub status: ActionStatus,
    pub changes: Vec<ChangeReport>,
    pub session_closed: Option<String>,
    pub undo_unavailable_reason: Option<String>,
}

/// The raw completion is returned to the app for its existing buffer refresh path.
pub struct HistoryCompletion {
    pub entry: UndoEntry,
    pub was_undo: bool,
    pub success: bool,
    pub action_id: ActionId,
    pub action_finished: bool,
}

use crate::bridge::BridgeCmd;
use crate::write_tracker::WriteTracker;
use std::collections::HashMap;

const MAX_UNDO_ACTIONS: usize = 100;
const MAX_REPORTS: usize = 500;

/// Audit records never confer write authority: executable payloads live separately.
pub struct ChangeHistory {
    reports: Vec<ActionReport>,
    payloads: HashMap<(ActionId, usize), UndoEntry>,
    pending: HashMap<u64, (ActionId, usize, usize)>,
    tracker: WriteTracker,
    session: SessionId,
    last_action: ActionId,
    /// Game PID stamped onto new reports (set on every attach).
    pid: u32,
    /// Loaded save's name stamped onto new reports (set on every attach /
    /// save-ready), so the audit trail separates careers by save.
    save_name: Option<String>,
    /// True when the stored history file needs rewriting.
    dirty: bool,
    /// Undo payloads loaded from disk, revivable when the same game PID
    /// reconnects. Keyed by (action id, change index).
    revivable: HashMap<(ActionId, usize), StoredPayload>,
    /// Highest action id ever seen, so restored runs never reuse ids.
    restored_max_action: ActionId,
}

/// Disk mirror of one undo payload. `Instant` is not portable across
/// processes; it is recreated on revive.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StoredPayload {
    pub description: String,
    pub addr: u64,
    pub old_raw: Vec<u8>,
    pub new_raw: Vec<u8>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredHistory {
    version: u32,
    actions: Vec<StoredAction>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredAction {
    report: ActionReport,
    payloads: Vec<(usize, StoredPayload)>,
}

impl Default for ChangeHistory {
    fn default() -> Self {
        Self::new()
    }
}

impl ChangeHistory {
    pub fn new() -> Self {
        Self {
            reports: Vec::new(),
            payloads: HashMap::new(),
            pending: HashMap::new(),
            tracker: WriteTracker::new(),
            session: 1,
            last_action: 0,
            pid: 0,
            save_name: None,
            dirty: false,
            revivable: HashMap::new(),
            restored_max_action: 0,
        }
    }

    /// Stamp the attached game PID onto subsequently submitted reports.
    pub fn note_pid(&mut self, pid: u32) {
        self.pid = pid;
    }

    /// Stamp the loaded save's name onto subsequently submitted reports.
    pub fn note_save_name(&mut self, name: Option<String>) {
        self.save_name = name;
        self.dirty = true;
    }

    /// Load the persisted audit trail. Reports are always visible; undo
    /// payloads become revivable only when the same game PID reconnects
    /// (every revived write still passes the compare-and-swap guard).
    pub fn load(path: &std::path::Path) -> Self {
        let mut history = Self::new();
        let Ok(text) = std::fs::read_to_string(path) else {
            return history;
        };
        let Ok(stored) = serde_json::from_str::<StoredHistory>(&text) else {
            return history;
        };
        if stored.version != 1 {
            return history;
        }
        for action in stored.actions {
            history.restored_max_action = history.restored_max_action.max(action.report.id);
            for (index, payload) in action.payloads {
                history.revivable.insert((action.report.id, index), payload);
            }
            history.reports.push(action.report);
        }
        history.last_action = history.restored_max_action;
        // Loaded reports belong to earlier sessions; start a fresh one above.
        let max_session = history
            .reports
            .iter()
            .map(|r| r.session_id)
            .max()
            .unwrap_or(0);
        history.session = max_session + 1;
        history.dirty = false;
        history
    }

    /// Persist after any mutation. Cheap no-op when clean.
    pub fn save_if_dirty(&mut self, path: &std::path::Path) {
        if !self.dirty {
            return;
        }
        let stored = StoredHistory {
            version: 1,
            actions: self
                .reports
                .iter()
                .map(|report| {
                    let payloads = (0..report.changes.len())
                        .filter_map(|index| {
                            self.payloads
                                .get(&(report.id, index))
                                .map(|entry| (index, stored_payload(entry)))
                                .or_else(|| {
                                    self.revivable
                                        .get(&(report.id, index))
                                        .cloned()
                                        .map(|p| (index, p))
                                })
                        })
                        .collect();
                    StoredAction {
                        report: report.clone(),
                        payloads,
                    }
                })
                .collect(),
        };
        if let Ok(text) = serde_json::to_string(&stored) {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(path, text);
            self.dirty = false;
        }
    }

    /// A new game session connected: revive undo payloads recorded against
    /// the same PID (same game still running, addresses still meaningful).
    pub fn revive_session(&mut self, pid: u32) {
        if pid == 0 {
            return;
        }
        // Revive every report that has a stored undo payload — the CAS
        // guard on every write is the real safety net, not the PID or the
        // session state. If the game's memory changed, undo is refused.
        let ids: Vec<ActionId> = self
            .reports
            .iter()
            .filter(|r| r.pid == pid || r.pid == 0)
            .map(|r| r.id)
            .collect();
        for id in ids {
            let revive: Vec<usize> = (0..self
                .reports
                .iter()
                .find(|r| r.id == id)
                .map_or(0, |r| r.changes.len()))
                .filter(|index| self.revivable.contains_key(&(id, *index)))
                .collect();
            let has_any = !revive.is_empty();
            for index in revive {
                if let Some(payload) = self.revivable.remove(&(id, index)) {
                    self.payloads.insert(
                        (id, index),
                        UndoEntry {
                            description: payload.description,
                            addr: payload.addr,
                            old_raw: payload.old_raw,
                            new_raw: payload.new_raw,
                            timestamp: std::time::Instant::now(),
                            policy_guard: None,
                        },
                    );
                }
            }
            if has_any {
                if let Some(report) = self.reports.iter_mut().find(|r| r.id == id) {
                    report.session_id = self.session;
                    report.session_closed = None;
                    report.undo_unavailable_reason = None;
                }
            }
        }
        self.dirty = true;
    }

    /// Remove actions from the audit trail entirely (their stored undo
    /// payloads go with them). Never touches game memory. Pending actions
    /// refuse deletion until their acknowledgements arrive.
    pub fn delete_actions(&mut self, ids: &[ActionId]) -> Result<(), String> {
        if self.is_pending() {
            return Err("Waiting for write acknowledgements".into());
        }
        let pending_ids: Vec<ActionId> = ids
            .iter()
            .copied()
            .filter(|id| self.pending.values().any(|&(a, _, _)| a == *id))
            .collect();
        if !pending_ids.is_empty() {
            return Err("Waiting for write acknowledgements".into());
        }
        self.reports.retain(|r| !ids.contains(&r.id));
        self.payloads.retain(|(id, _), _| !ids.contains(id));
        self.revivable.retain(|(id, _), _| !ids.contains(id));
        self.dirty = true;
        self.prune();
        Ok(())
    }

    /// True when the action can be undone right now (same live session,
    /// not pending, undo payload retained).
    pub fn undoable(&self, id: ActionId) -> bool {
        self.reports.iter().any(|r| r.id == id && self.eligible(r))
    }

    /// Undo specific actions, newest-first as ordered by the caller. Skips
    /// actions that are not eligible; fails when none are.
    pub fn undo_actions(&mut self, ids: &[ActionId]) -> Result<Vec<BridgeCmd>, String> {
        if let Some(reason) = self.undo_reason() {
            return Err(reason);
        }
        let now = SystemTime::now();
        let mut commands = Vec::new();
        let mut undone_any = false;
        for &action_id in ids {
            let Some(report) = self.reports.iter().find(|r| r.id == action_id) else {
                continue;
            };
            if !self.eligible(report) {
                continue;
            }
            undone_any = true;
            let report = self
                .reports
                .iter_mut()
                .find(|r| r.id == action_id)
                .expect("just checked");
            for index in (0..report.changes.len()).rev() {
                if let Some(entry) = self.payloads.get(&(action_id, index)) {
                    let attempts = &mut report.changes[index].attempts;
                    let attempt_index = attempts.len();
                    attempts.push(new_attempt(Operation::Undo, now));
                    let cmd = self.tracker.submit_undo(entry.clone());
                    self.pending
                        .insert(command_id(&cmd), (action_id, index, attempt_index));
                    commands.push(cmd);
                }
            }
            update_status(report);
        }
        if !undone_any {
            return Err("No confirmed changes available to undo in this session".into());
        }
        self.dirty = true;
        Ok(commands)
    }

    pub fn submit(&mut self, entity: EntityRef, actions: Vec<ActionDraft>) -> Vec<BridgeCmd> {
        let mut commands = Vec::new();
        for action in actions.into_iter().filter(|a| !a.changes.is_empty()) {
            self.last_action = self
                .last_action
                .checked_add(1)
                .expect("action ID exhausted");
            let id = self.last_action;
            let now = SystemTime::now();
            let mut changes = Vec::new();
            for (index, change) in action.changes.into_iter().enumerate() {
                let entry = change.entry;
                changes.push(ChangeReport {
                    field: change.presentation.field,
                    before: change.presentation.before,
                    after: change.presentation.after,
                    address: entry.addr,
                    old_raw: entry.old_raw.clone(),
                    new_raw: entry.new_raw.clone(),
                    attempts: vec![new_attempt(Operation::Apply, now)],
                });
                let cmd = self
                    .tracker
                    .submit(vec![entry.clone()])
                    .pop()
                    .expect("one write");
                self.pending.insert(command_id(&cmd), (id, index, 0));
                self.payloads.insert((id, index), entry);
                commands.push(cmd);
            }
            self.reports.push(ActionReport {
                id,
                session_id: self.session,
                pid: self.pid,
                save_name: self.save_name.clone(),
                entity: entity.clone(),
                label: action.label,
                before: action.before,
                after: action.after,
                submitted_at: now,
                updated_at: now,
                status: ActionStatus::Applying,
                changes,
                session_closed: None,
                undo_unavailable_reason: None,
            });
        }
        self.dirty = true;
        self.prune();
        commands
    }

    pub fn is_pending(&self) -> bool {
        self.tracker.is_pending()
    }

    /// IDs only: callers may reconcile queued refusals without accessing payloads.
    pub(crate) fn pending_ids(&self) -> Vec<u64> {
        self.pending.keys().copied().collect()
    }

    pub fn complete(
        &mut self,
        id: u64,
        success: bool,
        message: Option<String>,
        write_attempted: Option<bool>,
    ) -> Option<HistoryCompletion> {
        let &(action_id, index, attempt_index) = self.pending.get(&id)?;
        let completion = self.tracker.complete(id, success)?;
        self.pending.remove(&id);
        let report = self
            .reports
            .iter_mut()
            .find(|r| r.id == action_id)
            .expect("pending report retained");
        let attempt = &mut report.changes[index].attempts[attempt_index];
        attempt.completed_at = Some(SystemTime::now());
        attempt.message = message;
        attempt.outcome = if success {
            AttemptOutcome::Confirmed
        } else if write_attempted == Some(false) {
            AttemptOutcome::Refused
        } else {
            AttemptOutcome::Unconfirmed
        };
        if (completion.was_undo && success) || (!completion.was_undo && !success) {
            self.payloads.remove(&(action_id, index));
        }
        update_status(report);
        let action_finished = !self.pending.values().any(|&(a, _, _)| a == action_id);
        self.dirty = true;
        self.prune();
        Some(HistoryCompletion {
            entry: completion.entry,
            was_undo: completion.was_undo,
            success: completion.success,
            action_id,
            action_finished,
        })
    }

    pub fn undo_latest(&mut self) -> Result<Vec<BridgeCmd>, String> {
        if let Some(reason) = self.undo_reason() {
            return Err(reason);
        }
        let id = self.latest().expect("eligible action").id;
        self.undo_actions(&[id])
    }

    fn eligible(&self, report: &ActionReport) -> bool {
        // CAS (compare-and-swap) on every undo write is the real safety
        // net: if the game's memory no longer matches what we wrote, the
        // undo is refused. Session tracking is unnecessary on top of it.
        !self.pending.values().any(|&(id, _, _)| id == report.id)
            && self.payloads.keys().any(|&(id, _)| id == report.id)
    }

    fn latest(&self) -> Option<&ActionReport> {
        self.reports.iter().rev().find(|r| self.eligible(r))
    }
    pub fn undo_len(&self) -> usize {
        self.reports.iter().filter(|r| self.eligible(r)).count()
    }
    pub fn undo_label(&self) -> Option<String> {
        self.latest()
            .map(|r| format!("{}: {}", r.entity.name, r.label))
    }
    pub fn undo_reason(&self) -> Option<String> {
        if self.is_pending() {
            Some("Waiting for write acknowledgements".into())
        } else if self.latest().is_none() {
            Some("No stored undo payloads available".into())
        } else {
            None
        }
    }
    pub fn reports(&self) -> &[ActionReport] {
        &self.reports
    }
    pub fn report(&self, id: ActionId) -> Option<&ActionReport> {
        self.reports.iter().find(|r| r.id == id)
    }
    pub fn session_id(&self) -> SessionId {
        self.session
    }

    pub fn end_session(&mut self, reason: impl Into<String>) {
        let reason = reason.into();
        self.tracker.clear();
        self.pending.clear();
        self.payloads.clear();
        for report in self
            .reports
            .iter_mut()
            .filter(|r| r.session_id == self.session)
        {
            for attempt in report.changes.iter_mut().flat_map(|c| &mut c.attempts) {
                if attempt.outcome == AttemptOutcome::Pending {
                    attempt.outcome = AttemptOutcome::Interrupted;
                    attempt.completed_at = Some(SystemTime::now());
                    attempt.message = Some(format!("Session ended: {reason}; outcome unknown"));
                }
            }
            update_status(report);
            report.session_closed = Some(reason.clone());
            report.undo_unavailable_reason = Some(format!("Session ended: {reason}"));
        }
        self.session = self.session.checked_add(1).expect("session ID exhausted");
        self.dirty = true;
        self.prune();
    }

    fn prune(&mut self) {
        let eligible: Vec<_> = self
            .reports
            .iter()
            .filter(|r| self.eligible(r))
            .map(|r| r.id)
            .collect();
        for id in eligible
            .iter()
            .take(eligible.len().saturating_sub(MAX_UNDO_ACTIONS))
        {
            self.payloads.retain(|&(action, _), _| action != *id);
            self.reports
                .iter_mut()
                .find(|r| r.id == *id)
                .unwrap()
                .undo_unavailable_reason = Some("Undo retention limit reached".into());
        }
        while self.reports.len() > MAX_REPORTS {
            let Some(index) = self.reports.iter().position(|r| {
                !self.pending.values().any(|&(id, _, _)| id == r.id)
                    && !self.payloads.keys().any(|&(id, _)| id == r.id)
            }) else {
                break;
            };
            self.reports.remove(index);
        }
    }
}

fn stored_payload(entry: &UndoEntry) -> StoredPayload {
    StoredPayload {
        description: entry.description.clone(),
        addr: entry.addr,
        old_raw: entry.old_raw.clone(),
        new_raw: entry.new_raw.clone(),
    }
}

fn command_id(cmd: &BridgeCmd) -> u64 {
    match cmd {
        BridgeCmd::WriteBytes { id, .. } => *id,
        _ => unreachable!("tracker only submits writes"),
    }
}

fn new_attempt(operation: Operation, started_at: SystemTime) -> AttemptReport {
    AttemptReport {
        operation,
        started_at,
        completed_at: None,
        outcome: AttemptOutcome::Pending,
        message: None,
    }
}

fn update_status(report: &mut ActionReport) {
    use AttemptOutcome::*;
    let pending = report
        .changes
        .iter()
        .flat_map(|c| &c.attempts)
        .find(|a| a.outcome == Pending);
    report.updated_at = SystemTime::now();
    if let Some(attempt) = pending {
        report.status = if attempt.operation == Operation::Apply {
            ActionStatus::Applying
        } else {
            ActionStatus::Undoing
        };
        return;
    }
    let confirmed = report
        .changes
        .iter()
        .filter(|c| c.attempts[0].outcome == Confirmed)
        .count();
    let uncertain = report
        .changes
        .iter()
        .any(|c| matches!(c.attempts[0].outcome, Unconfirmed | Interrupted));
    let restored = report
        .changes
        .iter()
        .filter(|c| {
            c.attempts
                .iter()
                .any(|a| a.operation == Operation::Undo && a.outcome == Confirmed)
        })
        .count();
    let undo_started = report.changes.iter().any(|c| c.attempts.len() > 1);
    report.status = if undo_started {
        if restored == confirmed {
            if uncertain {
                ActionStatus::Unconfirmed
            } else {
                ActionStatus::Undone
            }
        } else if restored > 0 {
            ActionStatus::PartiallyUndone
        } else if report.changes.iter().any(|c| {
            // A later preflight refusal does not resolve an earlier uncertain write.
            c.attempts.iter().any(|a| {
                a.operation == Operation::Undo && matches!(a.outcome, Unconfirmed | Interrupted)
            })
        }) {
            ActionStatus::UndoUnconfirmed
        } else {
            ActionStatus::UndoRefused
        }
    } else if confirmed == report.changes.len() {
        ActionStatus::Applied
    } else if confirmed > 0 {
        ActionStatus::PartiallyApplied
    } else if uncertain {
        ActionStatus::Unconfirmed
    } else {
        ActionStatus::Refused
    };
    report.undo_unavailable_reason = if uncertain && restored == confirmed {
        Some(
            "Unconfirmed apply portions require inspection; they cannot be undone automatically"
                .into(),
        )
    } else if confirmed == restored {
        Some("No remaining confirmed changes to undo".into())
    } else {
        None
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn entity() -> EntityRef {
        EntityRef {
            kind: EntityKind::Club,
            build: GameBuild::Fm27Steam,
            address: 0x20000,
            index: 3,
            uid: Some(42),
            name: "Test club".into(),
        }
    }
    fn draft(label: &str, count: usize) -> ActionDraft {
        ActionDraft {
            label: label.into(),
            before: "1".into(),
            after: "2".into(),
            changes: (0..count)
                .map(|i| ChangeDraft {
                    entry: UndoEntry {
                        description: label.into(),
                        addr: 0x20000 + i as u64,
                        old_raw: vec![1],
                        new_raw: vec![2],
                        timestamp: Instant::now(),
                        policy_guard: None,
                    },
                    presentation: EditPresentation::new(format!("part {i}"), 1, 2),
                })
                .collect(),
        }
    }
    fn finish(
        h: &mut ChangeHistory,
        cmd: &BridgeCmd,
        success: bool,
        attempted: Option<bool>,
    ) -> HistoryCompletion {
        h.complete(
            command_id(cmd),
            success,
            Some(if success { "warning" } else { "failure" }.into()),
            attempted,
        )
        .unwrap()
    }
    fn status(h: &ChangeHistory) -> ActionStatus {
        h.reports().last().unwrap().status
    }

    #[test]
    fn undo_actions_reverts_several_actions_newest_first() {
        let mut h = ChangeHistory::new();
        let first = h.submit(entity(), vec![draft("one", 1)]);
        let second = h.submit(entity(), vec![draft("two", 1)]);
        for cmd in first.iter().chain(&second) {
            finish(&mut h, cmd, true, None);
        }
        let ids: Vec<u64> = h.reports().iter().map(|r| r.id).collect();
        assert_eq!(ids.len(), 2);
        let undo = h.undo_actions(&ids).expect("both eligible");
        // Newest action's write is queued first.
        match &undo[0] {
            BridgeCmd::WriteBytes { addr, .. } => assert_eq!(*addr, second_cmd_addr(&second[0])),
            _ => panic!("write expected"),
        }
        assert_eq!(undo.len(), 2);
        for cmd in &undo {
            finish(&mut h, cmd, true, None);
        }
        assert!(h.reports().iter().all(|r| r.status == ActionStatus::Undone));
        assert_eq!(h.undo_len(), 0);
    }

    fn second_cmd_addr(cmd: &BridgeCmd) -> u64 {
        match cmd {
            BridgeCmd::WriteBytes { addr, .. } => *addr,
            _ => panic!("write expected"),
        }
    }

    #[test]
    fn delete_actions_removes_audit_and_payloads_but_refuses_pending() {
        let mut h = ChangeHistory::new();
        let first = h.submit(entity(), vec![draft("one", 1)]);
        let cmds = h.submit(entity(), vec![draft("two", 1)]);
        for cmd in first.iter().chain(&cmds) {
            finish(&mut h, cmd, true, None);
        }
        assert_eq!(h.reports().len(), 2);
        let older = h.reports()[0].id;
        let newer = h.reports()[1].id;

        // Deleting the older action drops its report and undo payload only.
        h.delete_actions(&[older]).unwrap();
        assert_eq!(h.reports().len(), 1);
        assert_eq!(h.reports()[0].id, newer);
        assert!(h.undoable(newer));
        assert!(!h.undoable(older));

        // Pending writes refuse deletion.
        let pending = h.submit(entity(), vec![draft("three", 1)]);
        assert!(h.delete_actions(&[newer]).is_err());
        finish(&mut h, &pending[0], true, None);
        h.delete_actions(&[newer]).unwrap();
        assert_eq!(h.reports().len(), 1);
    }

    #[test]
    fn history_persists_across_runs_and_revives_for_the_same_pid() {
        let dir = std::env::temp_dir().join(format!("fm_history_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.json");
        std::fs::remove_file(&path).ok();

        // Run 1: one confirmed edit; the editor closes without undoing.
        let action_id;
        {
            let mut h = ChangeHistory::new();
            h.note_pid(4242);
            let cmds = h.submit(entity(), vec![draft("facility", 1)]);
            assert_eq!(cmds.len(), 1);
            finish(&mut h, &cmds[0], true, None);
            action_id = h.reports()[0].id;
            assert_eq!(h.reports()[0].pid, 4242);
            h.save_if_dirty(&path);
            assert!(path.exists());
        }

        // Run 2: a fresh editor still sees the audit trail, view-only.
        let mut revived = ChangeHistory::load(&path);
        assert_eq!(revived.reports().len(), 1);
        assert_eq!(revived.reports()[0].id, action_id);
        assert!(!revived.undoable(action_id));
        assert!(revived.undo_actions(&[action_id]).is_err());

        // The same game reconnects: the undo payload revives (CAS-guarded).
        revived.note_pid(4242);
        revived.revive_session(4242);
        assert!(revived.undoable(action_id));
        let undo = revived.undo_actions(&[action_id]).unwrap();
        for cmd in &undo {
            finish(&mut revived, cmd, true, None);
        }
        assert_eq!(revived.reports()[0].status, ActionStatus::Undone);

        // Run 3: a different game PID never revives payloads.
        let mut other = ChangeHistory::load(&path);
        other.note_pid(9999);
        other.revive_session(9999);
        assert_eq!(other.reports().len(), 1);
        assert!(!other.undoable(action_id));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn grouped_action_is_confirmed_per_part_and_undone_in_reverse_order() {
        let mut h = ChangeHistory::new();
        assert!(h.submit(entity(), vec![draft("empty", 0)]).is_empty());
        let cmds = h.submit(entity(), vec![draft("facility", 2)]);
        assert!(h.is_pending());
        assert_eq!(status(&h), ActionStatus::Applying);
        assert_eq!(h.undo_len(), 0);
        assert!(h.undo_latest().is_err());
        assert!(!finish(&mut h, &cmds[0], true, None).action_finished);
        assert_eq!(status(&h), ActionStatus::Applying);
        assert!(finish(&mut h, &cmds[1], true, None).action_finished);
        assert_eq!(status(&h), ActionStatus::Applied);
        assert_eq!(h.undo_len(), 1);
        assert_eq!(h.undo_label().as_deref(), Some("Test club: facility"));
        assert!(h.undo_reason().is_none());
        let undo = h.undo_latest().unwrap();
        assert_eq!(status(&h), ActionStatus::Undoing);
        for (cmd, addr) in undo.iter().zip([0x20001, 0x20000]) {
            match cmd {
                BridgeCmd::WriteBytes {
                    addr: actual,
                    expected,
                    data,
                    ..
                } => {
                    assert_eq!(*actual, addr);
                    assert_eq!(expected, &[2]);
                    assert_eq!(data, &[1]);
                }
                _ => panic!("write expected"),
            }
            let c = finish(&mut h, cmd, true, None);
            assert!(c.was_undo);
            assert_eq!(c.entry.old_raw, [1]);
            assert_eq!(c.entry.new_raw, [2]);
        }
        assert_eq!(status(&h), ActionStatus::Undone);
        assert_eq!(h.undo_len(), 0);
        assert!(h.payloads.is_empty());
        assert_eq!(
            h.reports()[0].changes[0].attempts[0].message.as_deref(),
            Some("warning")
        );
    }

    #[test]
    fn refused_and_uncertain_initial_attempts_never_get_undo_authority() {
        for (attempted, expected) in [
            (Some(false), ActionStatus::Refused),
            (Some(true), ActionStatus::Unconfirmed),
            (None, ActionStatus::Unconfirmed),
        ] {
            let mut h = ChangeHistory::new();
            let cmds = h.submit(entity(), vec![draft("edit", 2)]);
            for cmd in &cmds {
                finish(&mut h, cmd, false, attempted);
            }
            assert_eq!(status(&h), expected);
            assert!(h.undo_latest().is_err());
            assert!(h.payloads.is_empty());
        }
    }

    #[test]
    fn mixed_apply_undo_restores_only_confirmed_and_reports_initial_uncertainty() {
        for attempted in [Some(false), Some(true), None] {
            let mut h = ChangeHistory::new();
            let cmds = h.submit(entity(), vec![draft("edit", 2)]);
            finish(&mut h, &cmds[0], true, None);
            finish(&mut h, &cmds[1], false, attempted);
            assert_eq!(status(&h), ActionStatus::PartiallyApplied);
            let undo = h.undo_latest().unwrap();
            assert_eq!(undo.len(), 1);
            finish(&mut h, &undo[0], true, None);
            assert_eq!(
                status(&h),
                if attempted == Some(false) {
                    ActionStatus::Undone
                } else {
                    ActionStatus::Unconfirmed
                }
            );
            assert!(h.undo_latest().is_err());
            if attempted != Some(false) {
                assert!(
                    h.reports()[0]
                        .undo_unavailable_reason
                        .as_ref()
                        .unwrap()
                        .contains("inspection")
                );
            }
        }
    }

    #[test]
    fn partial_undo_retry_skips_restored_parts_and_retains_all_failed_attempts() {
        for attempted in [Some(false), Some(true), None] {
            let mut h = ChangeHistory::new();
            let cmds = h.submit(entity(), vec![draft("edit", 2)]);
            for cmd in &cmds {
                finish(&mut h, cmd, true, None);
            }
            let undo = h.undo_latest().unwrap();
            finish(&mut h, &undo[0], true, None);
            finish(&mut h, &undo[1], false, attempted);
            assert_eq!(status(&h), ActionStatus::PartiallyUndone);
            assert_eq!(h.undo_len(), 1);
            let retry = h.undo_latest().unwrap();
            assert_eq!(retry.len(), 1);
            match &retry[0] {
                BridgeCmd::WriteBytes {
                    addr,
                    expected,
                    data,
                    ..
                } => {
                    assert_eq!(*addr, 0x20000);
                    assert_eq!(expected, &[2]);
                    assert_eq!(data, &[1]);
                }
                _ => panic!("write expected"),
            }
            finish(&mut h, &retry[0], true, None);
            assert_eq!(status(&h), ActionStatus::Undone);
            let attempts = &h.reports()[0].changes[0].attempts;
            assert_eq!(attempts.len(), 3);
            assert_eq!(
                attempts[1].outcome,
                if attempted == Some(false) {
                    AttemptOutcome::Refused
                } else {
                    AttemptOutcome::Unconfirmed
                }
            );
            assert_eq!(attempts[1].message.as_deref(), Some("failure"));
            assert_eq!(attempts[2].outcome, AttemptOutcome::Confirmed);
            assert_eq!(h.reports()[0].changes[1].attempts.len(), 2);
        }
    }

    #[test]
    fn failed_undo_retries_newest_action_before_earlier_action() {
        for attempted in [Some(false), Some(true), None] {
            let mut h = ChangeHistory::new();
            let cmds = h.submit(entity(), vec![draft("older", 1), draft("newer", 1)]);
            // Completion ordering cannot reorder submission chronology.
            let newer = finish(&mut h, &cmds[1], true, None).action_id;
            finish(&mut h, &cmds[0], true, None);
            let undo = h.undo_latest().unwrap();
            let c = finish(&mut h, &undo[0], false, attempted);
            assert_eq!(c.action_id, newer);
            assert_eq!(
                status(&h),
                if attempted == Some(false) {
                    ActionStatus::UndoRefused
                } else {
                    ActionStatus::UndoUnconfirmed
                }
            );
            assert_eq!(h.undo_label().as_deref(), Some("Test club: newer"));
            let retry = h.undo_latest().unwrap();
            assert_eq!(finish(&mut h, &retry[0], true, None).action_id, newer);
            assert_eq!(status(&h), ActionStatus::Undone);
            assert_eq!(h.undo_label().as_deref(), Some("Test club: older"));
        }
    }

    #[test]
    fn unknown_duplicate_and_closed_session_late_ids_are_rejected() {
        let mut h = ChangeHistory::new();
        assert!(h.complete(999, true, None, None).is_none());
        let cmds = h.submit(entity(), vec![draft("done", 1), draft("pending", 1)]);
        finish(&mut h, &cmds[0], true, None);
        assert!(h.complete(command_id(&cmds[0]), true, None, None).is_none());
        let session = h.session_id();
        h.end_session("save replaced");
        assert_ne!(h.session_id(), session);
        assert!(h.payloads.is_empty());
        assert!(h.pending.is_empty());
        assert!(!h.tracker.is_pending());
        assert_eq!(h.undo_len(), 0);
        assert!(h.undo_latest().is_err());
        assert!(h.complete(command_id(&cmds[1]), true, None, None).is_none());
        assert_eq!(status(&h), ActionStatus::Unconfirmed);
        assert!(
            h.reports()
                .iter()
                .all(|r| r.session_closed.as_deref() == Some("save replaced"))
        );
        assert_eq!(
            h.reports()[1].changes[0].attempts[0].outcome,
            AttemptOutcome::Interrupted
        );
        let new = h.submit(entity(), vec![draft("new session", 1)]);
        assert!(command_id(&new[0]) > command_id(&cmds[1]));
        assert!(h.complete(command_id(&cmds[1]), true, None, None).is_none());
        assert!(h.is_pending());
        finish(&mut h, &new[0], true, None);
        assert_eq!(h.undo_len(), 1);
    }

    #[test]
    fn interrupted_undo_loses_authority_but_keeps_honest_audit() {
        let mut h = ChangeHistory::new();
        let cmds = h.submit(entity(), vec![draft("edit", 2)]);
        for cmd in &cmds {
            finish(&mut h, cmd, true, None);
        }
        let undo = h.undo_latest().unwrap();
        finish(&mut h, &undo[0], true, None);
        h.end_session("disconnected");
        assert_eq!(status(&h), ActionStatus::PartiallyUndone);
        assert_eq!(
            h.reports()[0].changes[0].attempts[1].outcome,
            AttemptOutcome::Interrupted
        );
        assert!(h.payloads.is_empty());
        assert!(h.complete(command_id(&undo[1]), true, None, None).is_none());
    }

    #[test]
    fn caps_count_actions_not_parts_and_never_prune_pending_reports() {
        let mut h = ChangeHistory::new();
        for i in 0..101 {
            let cmds = h.submit(entity(), vec![draft(&format!("edit {i}"), 2)]);
            for cmd in &cmds {
                finish(&mut h, cmd, true, None);
            }
        }
        assert_eq!(h.undo_len(), 100);
        assert!(!h.payloads.keys().any(|&(id, _)| id == 1));
        assert!(
            h.reports()[0]
                .undo_unavailable_reason
                .as_ref()
                .unwrap()
                .contains("limit")
        );
        let pending = h.submit(entity(), vec![draft("pending", 1)]);
        let pending_action = h.reports().last().unwrap().id;
        for _ in 0..550 {
            let cmds = h.submit(entity(), vec![draft("refused", 1)]);
            finish(&mut h, &cmds[0], false, Some(false));
        }
        assert_eq!(h.reports().len(), MAX_REPORTS);
        assert!(h.report(pending_action).is_some());
        finish(&mut h, &pending[0], true, None);
        assert_eq!(h.undo_len(), 100);
        h.end_session("closed");
        let pending = h.submit(entity(), (0..501).map(|_| draft("pending", 1)).collect());
        assert_eq!(h.reports().len(), 501);
        assert!(
            h.reports()
                .iter()
                .all(|r| r.status == ActionStatus::Applying)
        );
        finish(&mut h, &pending[0], false, Some(false));
        assert_eq!(h.reports().len(), 500);
        assert!(h.is_pending());
    }
}

#[cfg(test)]
mod uncertainty_retention_tests {
    use super::*;

    #[test]
    fn refused_retry_does_not_resolve_prior_uncertain_undo() {
        let mut report = ActionReport {
            id: 1,
            session_id: 1,
            pid: 0,
            save_name: None,
            entity: EntityRef {
                kind: EntityKind::Club,
                build: GameBuild::Fm27Steam,
                address: 0x20000,
                index: 0,
                uid: None,
                name: "Club".into(),
            },
            label: "Edit".into(),
            before: "1".into(),
            after: "2".into(),
            submitted_at: SystemTime::now(),
            updated_at: SystemTime::now(),
            status: ActionStatus::Undoing,
            changes: vec![ChangeReport {
                field: "Part".into(),
                before: "1".into(),
                after: "2".into(),
                address: 0x20000,
                old_raw: vec![1],
                new_raw: vec![2],
                attempts: vec![],
            }],
            session_closed: None,
            undo_unavailable_reason: None,
        };
        for (operation, outcome) in [
            (Operation::Apply, AttemptOutcome::Confirmed),
            (Operation::Undo, AttemptOutcome::Unconfirmed),
            (Operation::Undo, AttemptOutcome::Refused),
        ] {
            let mut attempt = new_attempt(operation, SystemTime::now());
            attempt.outcome = outcome;
            attempt.completed_at = Some(SystemTime::now());
            report.changes[0].attempts.push(attempt);
        }
        update_status(&mut report);
        assert_eq!(report.status, ActionStatus::UndoUnconfirmed);
        let mut restored = new_attempt(Operation::Undo, SystemTime::now());
        restored.outcome = AttemptOutcome::Confirmed;
        report.changes[0].attempts.push(restored);
        update_status(&mut report);
        assert_eq!(report.status, ActionStatus::Undone);
    }
}
