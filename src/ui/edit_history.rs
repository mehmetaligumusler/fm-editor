//! Read-only audit presentation. Actions are requests for the app to revalidate.
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Color32, RichText};

use crate::edit_history::{
    ActionReport, ActionStatus, AttemptOutcome, EntityRef, Operation, SessionId,
};
use crate::i18n::UiTr;

#[derive(Default)]
pub struct HistoryUi {
    pub open: bool,
    search: String,
    status: StatusFilter,
    current_entity_only: bool,
    include_closed_sessions: bool,
    #[cfg(test)]
    controls: Vec<(&'static str, egui::Rect, bool)>,
    #[cfg(test)]
    window_rect: Option<egui::Rect>,
}

#[derive(Clone)]
pub struct HistoryUiContext<'a> {
    pub current_session: SessionId,
    /// Actions that can be undone right now (live session, payload kept).
    pub undoable_ids: std::collections::HashSet<crate::edit_history::ActionId>,
    pub current_entity: Option<&'a EntityRef>,
    pub undo_label: Option<&'a str>,
    pub undo_reason: Option<&'a str>,
    pub navigation_enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryAction {
    UndoLatest,
    UndoListed(Vec<crate::edit_history::ActionId>),
    OpenEntity(EntityRef),
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum StatusFilter {
    /// Everything except fully undone actions — the default view, so
    /// reverted edits disappear from the working list.
    #[default]
    Active,
    All,
    Pending,
    Applied,
    Undone,
    NeedsAttention,
}

impl StatusFilter {
    const ALL: [Self; 6] = [
        Self::Active,
        Self::All,
        Self::Pending,
        Self::Applied,
        Self::Undone,
        Self::NeedsAttention,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::All => "All",
            Self::Pending => "Pending",
            Self::Applied => "Applied",
            Self::Undone => "Undone",
            Self::NeedsAttention => "Needs attention",
        }
    }

    fn matches(self, status: ActionStatus) -> bool {
        match self {
            Self::Active => status != ActionStatus::Undone,
            Self::All => true,
            Self::Pending => matches!(status, ActionStatus::Applying | ActionStatus::Undoing),
            Self::Applied => status == ActionStatus::Applied,
            Self::Undone => status == ActionStatus::Undone,
            Self::NeedsAttention => matches!(
                status,
                ActionStatus::PartiallyApplied
                    | ActionStatus::Refused
                    | ActionStatus::Unconfirmed
                    | ActionStatus::PartiallyUndone
                    | ActionStatus::UndoRefused
                    | ActionStatus::UndoUnconfirmed
            ),
        }
    }
}

fn ui_tr(ctx: &egui::Context, text: &str) -> String {
    crate::i18n::tr(ctx, text)
}

impl HistoryUi {
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        reports: &[ActionReport],
        context: &HistoryUiContext<'_>,
    ) -> Option<HistoryAction> {
        #[cfg(test)]
        self.controls.clear();
        let mut open = self.open;
        let mut action = None;
        let available = ctx.screen_rect().size() - egui::vec2(24.0, 40.0);
        let _window = egui::Window::new(ui_tr(ctx, "History"))
            .id(egui::Id::new("edit_history_window"))
            .open(&mut open)
            .resizable(true)
            .default_size(egui::vec2(920.0, 540.0).min(available))
            .min_width(240.0)
            .max_size(available.max(egui::vec2(240.0, 120.0)))
            .show(ctx, |ui| {
                action = self.contents(ui, reports, &context);
            });
        #[cfg(test)]
        {
            self.window_rect = _window.map(|window| window.response.rect);
        }
        self.open = open;
        action
    }

    fn contents(
        &mut self,
        ui: &mut egui::Ui,
        reports: &[ActionReport],
        context: &HistoryUiContext<'_>,
    ) -> Option<HistoryAction> {
        let mut action = None;
        let undo_reason = context.undo_reason.or_else(|| {
            context
                .undo_label
                .is_none()
                .then_some("No confirmed changes are available to undo.")
        });
        let mut visible: Vec<_> = reports
            .iter()
            .filter(|report| self.matches(report, context))
            .collect();
        // IDs are submission order even if the wall clock is adjusted backwards.
        visible.sort_by_key(|report| std::cmp::Reverse(report.id));
        if self.button(ui, "Undo latest", undo_reason).clicked() {
            action = Some(HistoryAction::UndoLatest);
        }
        // Undo everything the current filter/search lists (newest first):
        // e.g. search a player and revert all of their listed edits at once.
        let listed_undoable: Vec<_> = visible
            .iter()
            .map(|report| report.id)
            .filter(|id| context.undoable_ids.contains(id))
            .collect();
        let listed_reason = (!listed_undoable.is_empty())
            .then_some(None)
            .unwrap_or_else(|| Some("No listed action can be undone in this session."));
        if self.button(ui, "Undo listed", listed_reason).clicked() {
            action = Some(HistoryAction::UndoListed(listed_undoable));
        }
        if let Some(reason) = undo_reason {
            wrapped(ui, &ui.tr(reason));
        } else if let Some(label) = context.undo_label {
            short(ui, &format!("{} {label}", ui.tr("Next:")));
        }
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text(ui.tr("Search names, UID, fields, values or messages"))
                .desired_width(ui.available_width()),
        );
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("history_status")
                .selected_text(ui.tr(self.status.label()))
                .show_ui(ui, |ui| {
                    for filter in StatusFilter::ALL {
                        ui.selectable_value(&mut self.status, filter, &ui.tr(filter.label()));
                    }
                });
            ui.checkbox(&mut self.current_entity_only, &ui.tr("Current entity only"));
            ui.checkbox(
                &mut self.include_closed_sessions,
                &ui.tr("Include previous closed sessions"),
            );
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("history_rows")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width());
                if reports.is_empty() {
                    wrapped(
                        ui,
                        &ui.tr("No edit history yet. Submitted edits will appear here."),
                    );
                } else if visible.is_empty() {
                    wrapped(ui, &ui.tr("No history matches these filters."));
                }
                for report in visible {
                    ui.push_id((report.session_id, report.id), |ui| {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            self.row(ui, report, context, &mut action);
                        });
                        ui.add_space(5.0);
                    });
                }
            });
        action
    }

    fn matches(&self, report: &ActionReport, context: &HistoryUiContext<'_>) -> bool {
        (self.include_closed_sessions
            || (report.session_id == context.current_session && report.session_closed.is_none()))
            && self.status.matches(report.status)
            && (!self.current_entity_only
                || context
                    .current_entity
                    .is_some_and(|entity| same_entity(entity, &report.entity)))
            && search_matches(report, &self.search)
    }

    fn button(
        &mut self,
        ui: &mut egui::Ui,
        label: &'static str,
        disabled_reason: Option<&str>,
    ) -> egui::Response {
        let response = ui.add_enabled(disabled_reason.is_none(), egui::Button::new(ui.tr(label)));
        #[cfg(test)]
        self.controls
            .push((label, response.rect, response.enabled()));
        if let Some(reason) = disabled_reason {
            response.on_disabled_hover_text(ui.tr(reason))
        } else {
            response
        }
    }

    fn row(
        &mut self,
        ui: &mut egui::Ui,
        report: &ActionReport,
        context: &HistoryUiContext<'_>,
        action: &mut Option<HistoryAction>,
    ) {
        let narrow = ui.available_width() <= 650.0;
        let identity = format!(
            "{} · UID {}",
            ui.tr(report.entity.kind.label()),
            report
                .entity
                .uid
                .map_or_else(|| "unavailable".into(), |uid| uid.to_string())
        );
        if narrow {
            ui.horizontal_wrapped(|ui| {
                ui.label(utc_datetime(report.submitted_at));
                status_badge(ui, report.status);
            });
            short(ui, &report.entity.name);
            short(ui, &identity);
            short(ui, &report.label);
            values(ui, &report.before, &report.after);
        } else {
            ui.columns(4, |columns| {
                columns[0].label(utc_datetime(report.submitted_at));
                status_badge(&mut columns[0], report.status);
                short(&mut columns[1], &report.entity.name);
                short(&mut columns[1], &identity);
                short(&mut columns[2], &report.label);
                values(&mut columns[3], &report.before, &report.after);
            });
        }
        let reason = navigation_reason(report, context);
        if self.button(ui, "Open profile", reason).clicked() {
            *action = Some(HistoryAction::OpenEntity(report.entity.clone()));
        }
        // Per-action undo: reverts this one action through the acknowledged
        // compare-and-swap path (memory must still hold the written bytes).
        let undo_reason = if context.undoable_ids.contains(&report.id) {
            context.undo_reason
        } else {
            Some(
                report
                    .session_closed
                    .as_deref()
                    .or(report.undo_unavailable_reason.as_deref())
                    .or(context.undo_reason)
                    .unwrap_or("Already undone or not eligible"),
            )
        };
        if self.button(ui, "Undo", undo_reason).clicked() {
            *action = Some(HistoryAction::UndoListed(vec![report.id]));
        }
        if let Some(reason) = reason {
            short(ui, &ui.tr(reason));
        }
        egui::CollapsingHeader::new(ui.tr("Details"))
            .id_salt("details")
            .show(ui, |ui| details(ui, report));
    }
}

fn same_entity(left: &EntityRef, right: &EntityRef) -> bool {
    left.kind == right.kind
        && left.build == right.build
        && left.address == right.address
        && left.uid == right.uid
}

fn navigation_reason<'a>(
    report: &ActionReport,
    context: &HistoryUiContext<'a>,
) -> Option<&'static str> {
    if report.session_id != context.current_session || report.session_closed.is_some() {
        Some("Session closed: this record is audit-only; profiles cannot be opened.")
    } else if !context.navigation_enabled {
        Some("Profile navigation is unavailable while disconnected or loading.")
    } else if report.entity.uid.is_none_or(|uid| uid == 0)
        || report.entity.address <= 0x10000
        || report.entity.address >= 0x7FFF_FFFF_FFFF
    {
        Some("A valid entity UID and address are required to open a profile.")
    } else {
        None
    }
}

fn search_matches(report: &ActionReport, search: &str) -> bool {
    let query = search.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    let contains = |value: &str| value.to_lowercase().contains(&query);
    contains(&report.entity.name)
        || contains(report.entity.kind.label())
        || report
            .entity
            .uid
            .is_some_and(|uid| contains(&uid.to_string()))
        || contains(&report.label)
        || contains(&report.before)
        || contains(&report.after)
        || contains(report.status.label())
        || report.session_closed.as_deref().is_some_and(contains)
        || report
            .undo_unavailable_reason
            .as_deref()
            .is_some_and(contains)
        || report.changes.iter().any(|change| {
            contains(&change.field)
                || contains(&change.before)
                || contains(&change.after)
                || change
                    .attempts
                    .iter()
                    .any(|attempt| attempt.message.as_deref().is_some_and(contains))
        })
}

fn status_badge(ui: &mut egui::Ui, status: ActionStatus) {
    let dark = ui.visuals().dark_mode;
    let color = match status {
        ActionStatus::Applying | ActionStatus::Undoing => {
            if dark {
                Color32::from_rgb(145, 200, 255)
            } else {
                Color32::from_rgb(0, 70, 145)
            }
        }
        ActionStatus::Applied | ActionStatus::Undone => {
            if dark {
                Color32::from_rgb(130, 225, 165)
            } else {
                Color32::from_rgb(0, 100, 45)
            }
        }
        _ => {
            if dark {
                Color32::from_rgb(255, 195, 115)
            } else {
                Color32::from_rgb(135, 60, 0)
            }
        }
    };
    ui.add(egui::Label::new(RichText::new(ui.tr(status.label())).strong().color(color)).wrap());
}

// One-line summaries are bounded; full source values remain available on hover.
fn short(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(text).truncate())
        .on_hover_text(text);
}

fn wrapped(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(text).wrap());
}

fn values(ui: &mut egui::Ui, before: &str, after: &str) {
    short(ui, &format!("{} {before}", ui.tr("Before:")));
    short(ui, &format!("{} {after}", ui.tr("After:")));
}

fn details(ui: &mut egui::Ui, report: &ActionReport) {
    wrapped(
        ui,
        &ui.tr(
            "Acknowledgements describe reported outcomes, not an atomic transaction. Unconfirmed or interrupted writes may have changed memory; inspect the game before further edits.",
        ),
    );
    if let Some(reason) = &report.session_closed {
        wrapped(
            ui,
            &format!(
                "{} {reason}. {}",
                ui.tr("Session closed:"),
                ui.tr("This record has no write or navigation authority.")
            ),
        );
    }
    if let Some(reason) = &report.undo_unavailable_reason {
        wrapped(ui, &format!("{} {reason}", ui.tr("Undo unavailable:")));
    }
    for (index, change) in report.changes.iter().enumerate() {
        ui.separator();
        short(
            ui,
            &format!(
                "{} {}: {}",
                ui.tr("Physical change"),
                index + 1,
                change.field
            ),
        );
        values(ui, &change.before, &change.after);
        for attempt in &change.attempts {
            let operation = match attempt.operation {
                Operation::Apply => ui.tr("Apply"),
                Operation::Undo => ui.tr("Undo"),
            };
            let outcome = match attempt.outcome {
                AttemptOutcome::Pending => ui.tr("Pending acknowledgement"),
                AttemptOutcome::Confirmed => ui.tr("Confirmed"),
                AttemptOutcome::Refused => ui.tr("Refused before write"),
                AttemptOutcome::Unconfirmed => ui.tr("Unconfirmed — outcome unknown"),
                AttemptOutcome::Interrupted => ui.tr("Interrupted — outcome unknown"),
            };
            wrapped(
                ui,
                &format!(
                    "{operation} · {} · {outcome}",
                    utc_datetime(attempt.started_at)
                ),
            );
            if let Some(completed) = attempt.completed_at {
                wrapped(
                    ui,
                    &format!("{} {}", ui.tr("Completed:"), utc_datetime(completed)),
                );
            }
            if let Some(message) = &attempt.message {
                short(
                    ui,
                    &format!("{} {message}", ui.tr("Acknowledgement message / warning:")),
                );
            }
        }
        egui::CollapsingHeader::new(ui.tr("Technical information (read-only)"))
            .id_salt((index, "technical"))
            .show(ui, |ui| {
                wrapped(ui, &format!("{} 0x{:X}", ui.tr("Address:"), change.address));
                wrapped(
                    ui,
                    &format!("{} {}", ui.tr("Raw before:"), raw_preview(&change.old_raw)),
                );
                wrapped(
                    ui,
                    &format!("{} {}", ui.tr("Raw after:"), raw_preview(&change.new_raw)),
                );
            });
    }
}

fn raw_preview(bytes: &[u8]) -> String {
    const LIMIT: usize = 32;
    let mut text = bytes
        .iter()
        .take(LIMIT)
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ");
    if bytes.len() > LIMIT {
        text.push_str(&format!(
            " … ({} bytes total; first {LIMIT} shown)",
            bytes.len()
        ));
    } else {
        text.push_str(&format!(" ({} bytes)", bytes.len()));
    }
    text
}

fn utc_time(time: SystemTime) -> String {
    let seconds = match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => (duration.as_secs() % 86_400) as i64,
        Err(error) => {
            let duration = error.duration();
            let ceil = duration.as_secs() % 86_400 + u64::from(duration.subsec_nanos() != 0);
            -(ceil as i64)
        }
    }
    .rem_euclid(86_400);
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// `YYYY-MM-DD HH:MM:SS UTC` (proleptic Gregorian, floor for pre-epoch times).
fn utc_datetime(time: SystemTime) -> String {
    let secs = match time.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => {
            let d = e.duration();
            -(d.as_secs() as i64) - i64::from(d.subsec_nanos() != 0)
        }
    };
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {} UTC", utc_time(time))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit_history::{AttemptReport, ChangeReport, EntityKind};
    use crate::game_version::GameBuild;
    use std::time::Duration;

    fn fixture(status: ActionStatus) -> ActionReport {
        let time = UNIX_EPOCH + Duration::from_secs(3_723);
        ActionReport {
            id: 1,
            session_id: 2,
            pid: 0,
            entity: EntityRef {
                kind: EntityKind::Player,
                build: GameBuild::Fm27Steam,
                address: 0x123456,
                index: 5,
                uid: Some(123),
                name: "Example player".into(),
            },
            label: "Current ability".into(),
            before: "100".into(),
            after: "120".into(),
            submitted_at: time,
            updated_at: time,
            status,
            changes: vec![ChangeReport {
                field: "Current ability".into(),
                before: "100".into(),
                after: "120".into(),
                address: 0x123456,
                old_raw: vec![100, 0],
                new_raw: vec![120, 0],
                attempts: vec![AttemptReport {
                    operation: Operation::Apply,
                    started_at: time,
                    completed_at: Some(time + Duration::from_secs(1)),
                    outcome: AttemptOutcome::Refused,
                    message: Some("Expected bytes did not match".into()),
                }],
            }],
            session_closed: None,
            undo_unavailable_reason: None,
        }
    }

    fn context() -> HistoryUiContext<'static> {
        HistoryUiContext {
            current_session: 2,
            undoable_ids: [1u64].into_iter().collect(),
            current_entity: None,
            undo_label: Some("Example player: Current ability"),
            undo_reason: None,
            navigation_enabled: true,
        }
    }

    #[test]
    fn filters_retain_failed_undone_and_match_identity_not_name() {
        let mut history = HistoryUi::default();
        let statuses = [
            ActionStatus::Applying,
            ActionStatus::Applied,
            ActionStatus::PartiallyApplied,
            ActionStatus::Refused,
            ActionStatus::Unconfirmed,
            ActionStatus::Undoing,
            ActionStatus::Undone,
            ActionStatus::PartiallyUndone,
            ActionStatus::UndoRefused,
            ActionStatus::UndoUnconfirmed,
        ];
        // The default Active view hides fully undone actions.
        assert!(!history.matches(&fixture(ActionStatus::Undone), &context()));
        assert!(history.matches(&fixture(ActionStatus::Applied), &context()));
        history.status = StatusFilter::All;
        for status in statuses {
            assert!(history.matches(&fixture(status), &context()));
        }
        for (filter, expected) in [
            (StatusFilter::Active, 9),
            (StatusFilter::All, 10),
            (StatusFilter::Pending, 2),
            (StatusFilter::Applied, 1),
            (StatusFilter::Undone, 1),
            (StatusFilter::NeedsAttention, 6),
        ] {
            history.status = filter;
            assert_eq!(
                statuses
                    .iter()
                    .filter(|s| history.matches(&fixture(**s), &context()))
                    .count(),
                expected
            );
        }
        history.status = StatusFilter::All;
        let report = fixture(ActionStatus::Refused);
        for query in [
            "example",
            "PLAYER",
            "123",
            "ability",
            "100",
            "120",
            "did not match",
            "refused",
        ] {
            history.search = query.into();
            assert!(history.matches(&report, &context()), "{query}");
        }
        history.search = "no such entity".into();
        assert!(!history.matches(&report, &context()));
        history.search.clear();
        history.current_entity_only = true;
        assert!(!history.matches(&report, &context()));
        let mut renamed = report.entity.clone();
        renamed.name = "Renamed".into();
        renamed.index += 1;
        let mut current = context();
        current.current_entity = Some(&renamed);
        assert!(history.matches(&report, &current));
        for changed in 0..4 {
            let mut other = report.entity.clone();
            match changed {
                0 => other.kind = EntityKind::Staff,
                1 => other.build = GameBuild::Fm24Steam,
                2 => other.address += 8,
                _ => other.uid = Some(124),
            }
            let other_context = HistoryUiContext {
                current_entity: Some(&other),
                ..context()
            };
            assert!(!history.matches(&report, &other_context));
        }
    }

    #[test]
    fn old_and_closed_sessions_are_opt_in_and_never_navigable() {
        let mut history = HistoryUi::default();
        // This test exercises session opt-in, not status filtering.
        history.status = StatusFilter::All;
        let mut report = fixture(ActionStatus::Undone);
        report.session_id = 1;
        assert!(!history.matches(&report, &context()));
        assert!(navigation_reason(&report, &context()).is_some());
        history.include_closed_sessions = true;
        assert!(history.matches(&report, &context()));
        report.session_id = 2;
        report.session_closed = Some("Save replaced".into());
        assert!(history.matches(&report, &context()));
        assert!(navigation_reason(&report, &context()).is_some());
        history.include_closed_sessions = false;
        assert!(!history.matches(&report, &context()));
    }

    fn frame(
        history: &mut HistoryUi,
        ctx: &egui::Context,
        reports: &[ActionReport],
        context: HistoryUiContext<'_>,
        events: Vec<egui::Event>,
        width: f32,
    ) -> (Option<HistoryAction>, egui::FullOutput) {
        let mut action = None;
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 600.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| action = history.show(ctx, reports, &context),
        );
        (action, output)
    }

    fn click(
        history: &mut HistoryUi,
        ctx: &egui::Context,
        reports: &[ActionReport],
        context: impl Fn() -> HistoryUiContext<'static>,
        label: &str,
    ) -> Option<HistoryAction> {
        for _ in 0..3 {
            assert!(
                frame(history, ctx, reports, context(), vec![], 400.0)
                    .0
                    .is_none()
            );
        }
        let rect = history
            .controls
            .iter()
            .find(|control| control.0 == label)
            .unwrap()
            .1;
        let pos = rect.center();
        let down = vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ];
        assert!(
            frame(history, ctx, reports, context(), down, 400.0)
                .0
                .is_none()
        );
        frame(
            history,
            ctx,
            reports,
            context(),
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            400.0,
        )
        .0
    }

    #[test]
    fn explicit_enabled_clicks_only_emit_top_undo_or_profile_request() {
        let ctx = egui::Context::default();
        let mut history = HistoryUi {
            open: true,
            ..Default::default()
        };
        let reports = vec![fixture(ActionStatus::Applied)];
        assert_eq!(
            click(&mut history, &ctx, &reports, context, "Undo latest"),
            Some(HistoryAction::UndoLatest)
        );
        assert_eq!(
            click(&mut history, &ctx, &reports, context, "Open profile"),
            Some(HistoryAction::OpenEntity(reports[0].entity.clone()))
        );
        assert!(
            click(
                &mut history,
                &ctx,
                &reports,
                || HistoryUiContext {
                    undo_reason: Some("Waiting for acknowledgement"),
                    ..context()
                },
                "Undo latest"
            )
            .is_none()
        );
        assert!(
            click(
                &mut history,
                &ctx,
                &reports,
                || HistoryUiContext {
                    undo_label: None,
                    ..context()
                },
                "Undo latest"
            )
            .is_none()
        );
        assert!(
            click(
                &mut history,
                &ctx,
                &reports,
                || HistoryUiContext {
                    navigation_enabled: false,
                    ..context()
                },
                "Open profile"
            )
            .is_none()
        );
    }

    #[test]
    fn invalid_identity_and_closed_session_disable_profile_clicks() {
        for variant in 0..6 {
            let ctx = egui::Context::default();
            let mut history = HistoryUi {
                open: true,
                include_closed_sessions: true,
                ..Default::default()
            };
            let mut report = fixture(ActionStatus::Refused);
            match variant {
                0 => report.entity.uid = None,
                1 => report.entity.uid = Some(0),
                2 => report.entity.address = 0x10000,
                3 => report.entity.address = 0x7FFF_FFFF_FFFF,
                4 => report.session_closed = Some("Disconnected".into()),
                _ => report.session_id = 1,
            }
            assert!(click(&mut history, &ctx, &[report], context, "Open profile").is_none());
            assert!(
                history
                    .controls
                    .iter()
                    .filter(|control| control.0 == "Open profile")
                    .all(|control| !control.2)
            );
        }
    }

    #[test]
    fn utc_time_and_raw_previews_are_bounded() {
        assert_eq!(utc_time(UNIX_EPOCH), "00:00:00");
        assert_eq!(
            utc_time(UNIX_EPOCH + Duration::from_secs(86_400 + 3_723)),
            "01:02:03"
        );
        assert_eq!(utc_time(UNIX_EPOCH - Duration::from_millis(1)), "23:59:59");
        assert_eq!(
            utc_time(UNIX_EPOCH - Duration::from_secs(86_400)),
            "00:00:00"
        );
        assert_eq!(raw_preview(&[0, 255]), "00 FF (2 bytes)");
        for (time, text) in [
            (UNIX_EPOCH, "1970-01-01 00:00:00 UTC"),
            (
                UNIX_EPOCH - Duration::from_millis(1),
                "1969-12-31 23:59:59 UTC",
            ),
            (
                UNIX_EPOCH + Duration::from_secs(951_782_400),
                "2000-02-29 00:00:00 UTC",
            ),
            (
                UNIX_EPOCH + Duration::from_secs(4_107_542_400),
                "2100-03-01 00:00:00 UTC",
            ),
        ] {
            assert_eq!(utc_datetime(time), text);
        }
        assert!(raw_preview(&vec![255; 100_000]).len() < 180);
        assert!(raw_preview(&vec![255; 100_000]).contains("100000 bytes total"));
    }

    fn rendered_text(output: &egui::FullOutput) -> String {
        fn append(shape: &egui::epaint::Shape, text: &mut String) {
            match shape {
                egui::epaint::Shape::Text(shape) => {
                    text.push_str(shape.galley.text());
                    text.push('\n');
                }
                egui::epaint::Shape::Vec(shapes) => {
                    for shape in shapes {
                        append(shape, text);
                    }
                }
                _ => {}
            }
        }
        let mut text = String::new();
        for shape in &output.shapes {
            append(&shape.shape, &mut text);
        }
        text
    }

    #[test]
    fn empty_and_filtered_empty_have_distinct_messages() {
        let ctx = egui::Context::default();
        let mut history = HistoryUi {
            open: true,
            ..Default::default()
        };
        frame(&mut history, &ctx, &[], context(), vec![], 400.0);
        let (_, empty) = frame(&mut history, &ctx, &[], context(), vec![], 400.0);
        assert!(rendered_text(&empty).contains("No edit history yet"));
        history.search = "not found".into();
        let (_, filtered) = frame(
            &mut history,
            &ctx,
            &[fixture(ActionStatus::Refused)],
            context(),
            vec![],
            400.0,
        );
        assert!(rendered_text(&filtered).contains("No history matches these filters"));
    }

    #[test]
    fn long_content_and_expanded_details_fit_both_widths_and_themes() {
        for width in [400.0, 1280.0] {
            for dark in [false, true] {
                let ctx = egui::Context::default();
                ctx.set_visuals(if dark {
                    egui::Visuals::dark()
                } else {
                    egui::Visuals::light()
                });
                let mut report = fixture(ActionStatus::UndoUnconfirmed);
                report.entity.name = "UnbrokenVeryLongEntityName".repeat(100);
                report.before = "very long before value ".repeat(100);
                report.after = "UnbrokenAfterValue".repeat(100);
                report.changes[0].attempts[0].message = Some("UnbrokenErrorMessage".repeat(100));
                report.changes[0].old_raw = vec![1; 10_000];
                report.session_closed = Some("Old save replaced".into());
                report.undo_unavailable_reason = Some("No confirmed writes remain".into());
                let mut history = HistoryUi {
                    open: true,
                    include_closed_sessions: true,
                    ..Default::default()
                };
                for _ in 0..3 {
                    assert!(
                        frame(
                            &mut history,
                            &ctx,
                            &[report.clone()],
                            context(),
                            vec![],
                            width
                        )
                        .0
                        .is_none()
                    );
                }
                let window = history.window_rect.unwrap();
                assert!(
                    window.left() >= 0.0 && window.right() <= width,
                    "window {window:?}, width {width}"
                );
                assert!(
                    window.top() >= 0.0 && window.bottom() <= 600.0,
                    "window {window:?}"
                );
                let mut extent = 0.0_f32;
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 600.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                let mut action = None;
                                history.row(ui, &report, &context(), &mut action);
                                details(ui, &report);
                                extent = ui.min_rect().right();
                                assert!(action.is_none());
                            });
                        });
                    },
                );
                assert!(
                    extent <= width,
                    "width {width}, dark {dark}: extent {extent}"
                );
                let text = rendered_text(&output);
                assert!(text.contains("Session closed: Old save replaced"));
                assert!(text.contains("Undo unavailable: No confirmed writes remain"));
                assert!(text.contains("Acknowledgements describe reported outcomes"));
            }
        }
    }

    #[test]
    fn submitted_order_and_failed_undone_rows_remain_visible_without_actions() {
        let ctx = egui::Context::default();
        let mut history = HistoryUi {
            open: true,
            ..Default::default()
        };
        let mut reports = vec![
            fixture(ActionStatus::Undone),
            fixture(ActionStatus::Refused),
            fixture(ActionStatus::Unconfirmed),
        ];
        for (index, report) in reports.iter_mut().enumerate() {
            report.id = index as u64;
            report.entity.name = format!("Chronology{index}");
            // Reverse both wall-clock and completion order: monotonic action IDs
            // still determine the same chronology as Undo.
            report.submitted_at += Duration::from_secs(10 - index as u64);
            report.updated_at += Duration::from_secs(index as u64);
        }
        // Undone rows only appear when the filter asks for them.
        history.status = StatusFilter::All;
        frame(&mut history, &ctx, &reports, context(), vec![], 1280.0);
        let (action, output) = frame(&mut history, &ctx, &reports, context(), vec![], 1280.0);
        assert!(action.is_none());
        let text = rendered_text(&output);
        assert!(text.find("Chronology2").unwrap() < text.find("Chronology1").unwrap());
        assert!(text.find("Chronology1").unwrap() < text.find("Chronology0").unwrap());
        for status in ["Refused", "Undone", "Unconfirmed"] {
            assert!(text.contains(status));
        }
        history.status = StatusFilter::default();
        let (_, active_output) = frame(&mut history, &ctx, &reports, context(), vec![], 1280.0);
        let active_text = rendered_text(&active_output);
        assert!(!active_text.contains("Undone"));
        assert!(active_text.contains("Refused"));
        assert!(active_text.contains("Unconfirmed"));
        assert_eq!(
            history
                .controls
                .iter()
                .filter(|control| control.0 == "Undo latest")
                .count(),
            1
        );
    }

    #[test]
    fn details_include_physical_changes_attempts_and_acknowledgement_warnings() {
        let ctx = egui::Context::default();
        let mut report = fixture(ActionStatus::Undone);
        report.changes[0].attempts.push(AttemptReport {
            operation: Operation::Undo,
            started_at: report.submitted_at + Duration::from_secs(10),
            completed_at: Some(report.submitted_at + Duration::from_secs(11)),
            outcome: AttemptOutcome::Confirmed,
            message: Some("Readback warning retained".into()),
        });
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| details(ui, &report));
        });
        let text = rendered_text(&output);
        for expected in [
            "Physical change 1: Current ability",
            "Before: 100",
            "After: 120",
            "Apply · 1970-01-01 01:02:03 UTC · Refused before write",
            "Undo · 1970-01-01 01:02:13 UTC · Confirmed",
            "Completed: 1970-01-01 01:02:14 UTC",
            "Readback warning retained",
            "Expected bytes did not match",
        ] {
            assert!(text.contains(expected), "missing {expected}: {text}");
        }
    }
}
