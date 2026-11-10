//! Net-modification overlay: per-address cumulative deltas of the user's
//! effective interventions, derived from the change-history reports.
//!
//! For every address touched by at least one in-effect action the overlay
//! keeps the baseline bytes (the oldest effective edit's `old_raw`) and the
//! current bytes (the newest effective edit's `new_raw`), so widgets can
//! render a "+6"-style badge next to the value. Undoing an action drops it
//! from the chain, so the delta always reflects what is actually in effect.
//! The compare-and-swap guard on each undo write remains the real safety net.

use std::collections::HashMap;

use eframe::egui;

use crate::edit_history::{ActionId, ActionReport, ActionStatus, AttemptOutcome, EntityKind};

/// One address's cumulative intervention.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlayEntry {
    /// Byte width of the field (1, 2 or 4).
    pub width: u8,
    /// Bytes before the user's first effective edit at this address.
    pub baseline: Vec<u8>,
    /// Bytes in effect after the newest effective edit (may differ from the
    /// live bytes — players develop on their own).
    pub current: Vec<u8>,
    /// Sum of the user's per-edit DISPLAYED deltas (attr_edit decodes
    /// encoded attributes before differencing). Natural development never
    /// enters this sum: only user submissions do.
    pub user_delta: i64,
    /// Telescoping raw delta `last_new - first_old` — for money fields
    /// (raw is linear GBP) where the badge converts by the currency factor.
    pub raw_delta: i64,
    /// Newest effective action touching this address — undoing it reverts
    /// the field to the previous state in the chain.
    pub action_id: ActionId,
}

/// Effective for overlay purposes: the action took effect and was not undone.
fn effective(status: ActionStatus) -> bool {
    matches!(
        status,
        ActionStatus::Applied | ActionStatus::PartiallyApplied
    )
}

/// Parse an integer from a change's displayed value ("14", "-1",
/// "4,388,574 €", "12000 GBP/week" all work): digits, sign and thousands
/// separators only. Legacy entries (saved before numeric_delta tagging)
/// carry their displayed values here, letting the overlay compute their
/// deltas without raw-byte guessing.
fn parse_displayed(text: &str) -> Option<i64> {
    let cleaned: String = text
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    cleaned.parse::<i64>().ok()
}

/// Numeric little-endian value of `bytes`, sign-extended for width 1/2/4
/// (wrapped through u64 so raw bit patterns stay faithful).
fn numeric(bytes: &[u8]) -> i64 {
    let mut v: u64 = 0;
    for (i, b) in bytes.iter().enumerate().take(8) {
        v |= u64::from(*b) << (8 * i);
    }
    match bytes.len() {
        1 => v as u8 as i64,
        2 => v as u16 as i64,
        4 => v as u32 as i64,
        _ => v as i64,
    }
}

/// Build the overlay for the entity the user is currently viewing. Reports
/// are filtered by identity (kind + UID): players develop on their own, so
/// the LIVE bytes may drift from what the user wrote — the badge still shows
/// the user's intervention delta, independent of natural growth. An entity
/// from a different save never matches (its UID differs), which is what
/// keeps stale addresses from other careers out of the map.
pub fn build_overlay(
    reports: &[ActionReport],
    current: Option<(EntityKind, u32)>,
    current_save: Option<&str>,
) -> HashMap<u64, OverlayEntry> {
    let mut ordered: Vec<&ActionReport> = reports
        .iter()
        .filter(|r| match (current, &r.entity.kind) {
            (Some((kind, uid)), report_kind) => {
                kind == *report_kind && r.entity.uid.is_some_and(|u| u == uid)
            }
            (None, _) => false,
        })
        // Save separation: edits made on another save never enter this
        // save's badges. Legacy entries (None, saved before save tracking)
        // stay visible so existing history keeps its badges.
        .filter(|r| match (&r.save_name, current_save) {
            (Some(saved), Some(active)) => saved == active,
            (None, _) => true,
            (Some(_), None) => true,
        })
        .collect();
    ordered.sort_by_key(|r| r.id);
    // addr -> (baseline_raw, last_new_raw, user_delta_displayed, raw_delta,
    // all_deltas_known, action_id)
    let mut chains: HashMap<u64, (Vec<u8>, Vec<u8>, i64, i64, bool, ActionId)> = HashMap::new();
    for report in ordered {
        // Only actions that took effect enter the chain: undone edits drop
        // out (their payload was consumed), refused/unconfirmed never
        // happened reliably.
        if !effective(report.status) {
            continue;
        }
        for change in &report.changes {
            if change.old_raw.is_empty() || change.old_raw.len() != change.new_raw.len() {
                continue;
            }
            if !change
                .attempts
                .iter()
                .any(|a| a.outcome == AttemptOutcome::Confirmed)
            {
                continue;
            }
            let entry = chains.entry(change.address).or_insert_with(|| {
                (
                    change.old_raw.clone(),
                    change.old_raw.clone(),
                    0,
                    0,
                    true,
                    report.id,
                )
            });
            entry.3 += numeric(&change.new_raw) - numeric(&change.old_raw);
            let step_displayed = match change.numeric_delta {
                Some(d) => d,
                None => match (
                    parse_displayed(&change.before),
                    parse_displayed(&change.after),
                ) {
                    (Some(before), Some(after)) => after - before,
                    _ => {
                        // Non-numeric edit (dates, names): no displayed
                        // delta; the raw step keeps money badges working.
                        entry.4 = false;
                        numeric(&change.new_raw) - numeric(&change.old_raw)
                    }
                },
            };
            entry.2 += step_displayed;
            entry.1 = change.new_raw.clone();
            entry.5 = report.id;
        }
    }
    let mut out = HashMap::new();
    for (addr, (baseline, current, user_delta, raw_delta, _, action_id)) in chains {
        if raw_delta == 0 && user_delta == 0 {
            continue;
        }
        let width = current.len() as u8;
        out.insert(
            addr,
            OverlayEntry {
                width,
                baseline,
                current,
                user_delta,
                raw_delta,
                action_id,
            },
        );
    }
    out
}

// ─── egui plumbing ───────────────────────────────────────────────────────────

fn overlay_id() -> egui::Id {
    egui::Id::new("fm_overlay")
}

fn undo_request_id() -> egui::Id {
    egui::Id::new("fm_field_undo_request")
}

/// Publish this frame's overlay (call once per frame, before panels render).
pub fn publish(ctx: &egui::Context, overlay: &HashMap<u64, OverlayEntry>) {
    ctx.data_mut(|d| d.insert_temp(overlay_id(), overlay.clone()));
}

/// The overlay entry for an absolute address, if the current frame has one.
pub fn lookup(ui: &egui::Ui, addr: u64) -> Option<OverlayEntry> {
    ui.ctx()
        .data_mut(|d| d.get_temp::<HashMap<u64, OverlayEntry>>(overlay_id()))
        .and_then(|map| map.get(&addr).cloned())
}

/// Queue an undo of the newest effective action behind this overlay entry.
pub fn request_undo_for(ui: &egui::Ui, entry: &OverlayEntry) {
    ui.ctx()
        .data_mut(|d| d.insert_temp(undo_request_id(), entry.action_id));
}

/// Take the queued field-undo request (call once per frame in the app).
pub fn take_undo_request(ctx: &egui::Context) -> Option<ActionId> {
    ctx.data_mut(|d| d.remove_temp::<ActionId>(undo_request_id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit_history::{
        ActionReport, AttemptOutcome, AttemptReport, ChangeReport, EntityKind, EntityRef, Operation,
    };
    use crate::game_version::GameBuild;
    use std::time::SystemTime;

    fn entity() -> EntityRef {
        EntityRef {
            kind: EntityKind::Player,
            build: GameBuild::Fm24Epic,
            address: 0x20000,
            index: 0,
            uid: Some(7),
            name: "Test".into(),
        }
    }

    const VIEWING: Option<(EntityKind, u32)> = Some((EntityKind::Player, 7));

    fn change(addr: u64, old: u8, new: u8) -> ChangeReport {
        ChangeReport {
            field: "Morale".into(),
            before: old.to_string(),
            after: new.to_string(),
            address: addr,
            old_raw: vec![old],
            new_raw: vec![new],
            numeric_delta: Some(new as i64 - old as i64),
            attempts: vec![AttemptReport {
                operation: Operation::Apply,
                started_at: SystemTime::now(),
                completed_at: Some(SystemTime::now()),
                outcome: AttemptOutcome::Confirmed,
                message: None,
            }],
        }
    }

    fn report(id: u64, status: ActionStatus, changes: Vec<ChangeReport>) -> ActionReport {
        ActionReport {
            id,
            session_id: 1,
            pid: 0,
            save_name: None,
            entity: entity(),
            label: "test".into(),
            before: String::new(),
            after: String::new(),
            submitted_at: SystemTime::now(),
            updated_at: SystemTime::now(),
            status,
            changes,
            session_closed: None,
            undo_unavailable_reason: None,
        }
    }

    #[test]
    fn chained_edits_show_the_cumulative_delta() {
        // 8 -> 10, then 10 -> 14: net +6 over the original 8.
        let reports = vec![
            report(1, ActionStatus::Applied, vec![change(0x1000, 8, 10)]),
            report(2, ActionStatus::Applied, vec![change(0x1000, 10, 14)]),
        ];
        let map = build_overlay(&reports, VIEWING, None);
        let e = map.get(&0x1000).unwrap();
        assert_eq!(e.baseline, vec![8]);
        assert_eq!(e.current, vec![14]);
        assert_eq!(e.user_delta, 6);
        assert_eq!(e.raw_delta, 6);
        assert_eq!(e.action_id, 2);
    }

    #[test]
    fn undone_action_leaves_only_the_reapplied_delta() {
        // 8 -> 14 applied then undone, then 8 -> 10 applied: net +2.
        let reports = vec![
            report(1, ActionStatus::Undone, vec![change(0x1000, 8, 14)]),
            report(2, ActionStatus::Applied, vec![change(0x1000, 8, 10)]),
        ];
        let map = build_overlay(&reports, VIEWING, None);
        let e = map.get(&0x1000).unwrap();
        assert_eq!(e.user_delta, 2);
        assert_eq!(e.raw_delta, 2);
        assert_eq!(e.baseline, vec![8]);
        assert_eq!(e.current, vec![10]);
    }

    #[test]
    fn negative_delta_and_wider_fields_are_supported() {
        // u16 1000 -> 800: delta -200. u32-style 4-byte field untouched.
        let mut big = change(0x2000, 0, 0);
        big.old_raw = 1000u32.to_le_bytes().to_vec();
        big.new_raw = 800u32.to_le_bytes().to_vec();
        // The producing widget tags the displayed delta (-200).
        big.numeric_delta = Some(-200);
        let reports = vec![report(
            1,
            ActionStatus::Applied,
            vec![change(0x1000, 6, 3), big],
        )];
        let map = build_overlay(&reports, VIEWING, None);
        // Without numeric_delta tags the overlay falls back to the raw
        // telescoping delta; with one, the displayed sum is used.
        let mut tagged = reports.clone();
        tagged[0].changes[0].numeric_delta = Some(-3);
        assert_eq!(
            build_overlay(&tagged, VIEWING, None)
                .get(&0x1000)
                .unwrap()
                .user_delta,
            -3
        );
        assert_eq!(map.get(&0x1000).unwrap().raw_delta, -3);
        let wide = map.get(&0x2000).unwrap();
        assert_eq!(wide.width, 4);
        assert_eq!(wide.raw_delta, -200);
        assert_eq!(wide.user_delta, -200);
    }

    #[test]
    fn legacy_entries_derive_displayed_deltas_from_before_after_strings() {
        // Saved before numeric_delta tagging: only the displayed strings
        // exist, and the delta must still come out right (14 -> 17 = +3).
        let mut r = report(1, ActionStatus::Applied, vec![change(0x1000, 8, 14)]);
        r.changes[0].numeric_delta = None;
        r.changes[0].before = "14".into();
        r.changes[0].after = "17".into();
        let map = build_overlay(&[r], VIEWING, None);
        assert_eq!(map.get(&0x1000).unwrap().user_delta, 3);
    }

    #[test]
    fn refused_and_unconfirmed_actions_never_enter_the_chain() {
        let mut refused = change(0x1000, 8, 20);
        refused.attempts[0].outcome = AttemptOutcome::Refused;
        let reports = vec![report(1, ActionStatus::Refused, vec![refused])];
        assert!(build_overlay(&reports, VIEWING, None).is_empty());
    }

    #[test]
    fn other_entities_edits_never_leak_into_the_overlay() {
        let mut other = entity();
        other.uid = Some(99);
        other.address = 0x30000;
        let mut r = report(1, ActionStatus::Applied, vec![change(0x4000, 8, 20)]);
        r.entity = other;
        // Viewing player uid 7: uid-99 edits must not appear.
        assert!(build_overlay(&[r], VIEWING, None).is_empty());
    }

    #[test]
    fn net_zero_chain_produces_no_entry() {
        let reports = vec![
            report(1, ActionStatus::Applied, vec![change(0x1000, 8, 14)]),
            report(2, ActionStatus::Applied, vec![change(0x1000, 14, 8)]),
        ];
        assert!(build_overlay(&reports, VIEWING, None).is_empty());
    }
}
