//! Staff search page: filter row with role dropdown + scrollable result grid.
//!
//! Search is now cache-driven: `filter()` runs every frame against the
//! in-memory `EntityCache` — no bridge round-trip needed.

use crate::i18n::UiTr;
use eframe::egui;

use crate::cache::CachedStaff;
use crate::ui::field_widget;

#[derive(Default, Clone, Copy, PartialEq)]
pub enum SortColumn {
    #[default]
    Uid,
    Name,
    Ca,
    Pa,
    Age,
    Role,
}

#[derive(Default, Clone, Copy, PartialEq)]
pub enum SortDir {
    #[default]
    Asc,
    Desc,
}

#[derive(Default)]
pub struct StaffSearchState {
    pub query: String,
    pub ca_min: String,
    pub pa_min: String,
    pub age_min: String,
    pub age_max: String,
    pub role: Option<usize>,
    pub sort_col: SortColumn,
    pub sort_dir: SortDir,
}

/// Role filter dropdown entries. The `usize` is an index into a staff's
/// 15-byte role rating sub-record; None means "All Roles".
const ROLE_FILTER_OPTIONS: &[(Option<usize>, &str)] = &[
    (None, "All Roles"),
    (Some(0), "Manager"),
    (Some(1), "Asst Manager"),
    (Some(2), "Coach"),
    (Some(5), "GK Coach"),
    (Some(6), "Fitness Coach"),
    (Some(7), "Set Piece Coach"),
    (Some(4), "Scout"),
    (Some(10), "Head of Youth Dev"),
    (Some(12), "Data Analyst"),
    (Some(11), "Loan Manager"),
    (Some(9), "Dir of Football"),
    (Some(3), "Physio"),
    (Some(13), "Sports Scientist"),
    (Some(14), "Technical Director"),
];

/// Short role names for the grid's "Role" column.
const ROLE_SHORT: [&str; 15] = [
    "Manager",
    "Asst Manager",
    "Coach",
    "Physio",
    "Scout",
    "GK Coach",
    "Fitness Coach",
    "Set Piece Coach",
    "(Internal)",
    "Dir of Football",
    "Head of Youth Dev",
    "Loan Manager",
    "Data Analyst",
    "Sports Scientist",
    "Technical Director",
];

/// Pick the most distinctive role label to show in the grid's Role column.
fn primary_role(roles: &[u8; 15], filter_idx: Option<usize>) -> String {
    if let Some(i) = filter_idx {
        if let Some(&v) = roles.get(i) {
            if v > 1 {
                return ROLE_SHORT[i].to_string();
            }
        }
    }
    let mut best: Option<(usize, u8)> = None;
    for (i, &v) in roles.iter().enumerate() {
        if i == 8 || v <= 1 {
            continue;
        }
        if best.map(|(_, bv)| v > bv).unwrap_or(true) {
            best = Some((i, v));
        }
    }
    match best {
        Some((i, _)) => ROLE_SHORT[i].to_string(),
        None => "-".to_string(),
    }
}

#[derive(Default)]
struct NumericFilters {
    ca_min: Option<u16>,
    pa_min: Option<u16>,
    age_min: Option<u16>,
    age_max: Option<u16>,
}

fn numeric_bound(text: &str, label: &str, max: u16) -> Result<Option<u16>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    match text.parse::<u16>() {
        Ok(value) if value <= max => Ok(Some(value)),
        _ => Err(format!("{label}: enter a whole number from 0 to {max}.")),
    }
}

fn numeric_filters(state: &StaffSearchState) -> Result<NumericFilters, String> {
    let filters = NumericFilters {
        ca_min: numeric_bound(&state.ca_min, "CA minimum", 200)?,
        pa_min: numeric_bound(&state.pa_min, "PA minimum", 200)?,
        age_min: numeric_bound(&state.age_min, "Minimum age", 150)?,
        age_max: numeric_bound(&state.age_max, "Maximum age", 150)?,
    };
    if matches!((filters.age_min, filters.age_max), (Some(min), Some(max)) if min > max) {
        return Err("Minimum age must not exceed maximum age.".to_string());
    }
    Ok(filters)
}

fn staff_age(
    schemas: &'static crate::fields::VersionedSchemas,
    today: Option<crate::game_date::GameDate>,
    staff: &CachedStaff,
) -> Option<u16> {
    if today.is_some() {
        return crate::ui::entity_header::age(schemas, today, &staff.raw, "s_dob").map(u16::from);
    }
    let year = schemas.age_reference_year;
    (staff.dob_year > 0 && staff.dob_year <= year).then(|| year - staff.dob_year)
}

fn compare_ages(a: Option<u16>, b: Option<u16>, direction: SortDir) -> std::cmp::Ordering {
    match (a, b) {
        (Some(a), Some(b)) => match direction {
            SortDir::Asc => a.cmp(&b),
            SortDir::Desc => b.cmp(&a),
        },
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
    }
}

/// Filter the full staff slice down to matching entries.
pub fn filter<'a>(
    schemas: &'static crate::fields::VersionedSchemas,
    today: Option<crate::game_date::GameDate>,
    cache: &'a [CachedStaff],
    state: &StaffSearchState,
) -> Vec<&'a CachedStaff> {
    let trimmed = state.query.trim();
    let uid_query: Option<u32> = trimmed.parse().ok();
    let words: Vec<String> = if uid_query.is_some() {
        Vec::new()
    } else {
        trimmed
            .to_lowercase()
            .split_whitespace()
            .map(|w| w.to_string())
            .collect()
    };
    let name_query_active = !words.is_empty();

    let Ok(NumericFilters {
        ca_min,
        pa_min,
        age_min,
        age_max,
    }) = numeric_filters(state)
    else {
        return Vec::new();
    };

    cache
        .iter()
        .filter(|s| {
            if let Some(m) = ca_min {
                if u16::from(s.ca) < m {
                    return false;
                }
            }
            if let Some(m) = pa_min {
                if u16::from(s.pa) < m {
                    return false;
                }
            }
            if let Some(age) = staff_age(schemas, today, s) {
                if let Some(m) = age_min {
                    if age < m {
                        return false;
                    }
                }
                if let Some(m) = age_max {
                    if age > m {
                        return false;
                    }
                }
            } else if age_min.is_some() || age_max.is_some() {
                return false;
            }
            if let Some(role_idx) = state.role {
                if s.roles.get(role_idx).copied().unwrap_or(0) <= 1 {
                    return false;
                }
            }
            if let Some(uid) = uid_query {
                return s.uid == uid;
            }
            if name_query_active {
                let name_lower = s.name.to_lowercase();
                if !words.iter().all(|w| name_lower.contains(w.as_str())) {
                    return false;
                }
            }
            true
        })
        .collect()
}

fn reset_filters(state: &mut StaffSearchState) {
    *state = StaffSearchState {
        sort_col: state.sort_col,
        sort_dir: state.sort_dir,
        ..Default::default()
    };
}

/// Filtering is live; only functional filters are shown.
pub fn filter_row(ui: &mut egui::Ui, state: &mut StaffSearchState) {
    ui.horizontal_wrapped(|ui| {
        ui.label(ui.tr("Name/UID:"));
        ui.add_sized(
            [220.0, 20.0],
            egui::TextEdit::singleline(&mut state.query).hint_text("Name or exact UID"),
        ).on_hover_text("Live search: numeric queries match an exact UID. Otherwise all words must occur in the name (case-insensitive, any order). Filters are combined with AND; bounds are inclusive.");
    });
    ui.horizontal_wrapped(|ui| {
        ui.horizontal(|ui| {
            ui.label("CA ≥");
            ui.add_sized([42.0, 20.0], egui::TextEdit::singleline(&mut state.ca_min));
        });
        ui.horizontal(|ui| {
            ui.label("PA ≥");
            ui.add_sized([42.0, 20.0], egui::TextEdit::singleline(&mut state.pa_min));
        });
        ui.horizontal(|ui| {
            ui.label("Age ≥");
            ui.add_sized([36.0, 20.0], egui::TextEdit::singleline(&mut state.age_min));
            ui.label("≤");
            ui.add_sized([36.0, 20.0], egui::TextEdit::singleline(&mut state.age_max));
        });
        ui.horizontal(|ui| {
            ui.label("Role:");
            let current_label = ROLE_FILTER_OPTIONS
                .iter()
                .find(|(idx, _)| *idx == state.role)
                .map(|(_, label)| *label)
                .unwrap_or("All Roles");
            egui::ComboBox::from_id_salt("staff_role_filter")
                .selected_text(current_label)
                .width(150.0)
                .show_ui(ui, |ui| {
                    for (idx, label) in ROLE_FILTER_OPTIONS {
                        ui.selectable_value(&mut state.role, *idx, *label);
                    }
                });
        });
        if ui.button(ui.tr("Reset all")).clicked() {
            reset_filters(state);
        }
    });
    if let Err(message) = numeric_filters(state) {
        ui.colored_label(ui.visuals().error_fg_color, message);
    }
}

/// Render the results grid. Returns the original (pre-sort) index of a clicked
/// row, if any.
pub fn results_grid(
    schemas: &'static crate::fields::VersionedSchemas,
    today: Option<crate::game_date::GameDate>,
    ui: &mut egui::Ui,
    state: &mut StaffSearchState,
    results: &[&CachedStaff],
) -> Option<usize> {
    let mut clicked: Option<usize> = None;

    const COL_NAME: f32 = 200.0;
    const COL_ROLE: f32 = 160.0;
    const COL_UID: f32 = 80.0;
    const COL_CA: f32 = 50.0;
    const COL_PA: f32 = 50.0;
    const COL_AGE: f32 = 50.0;
    const ROW_HEIGHT: f32 = 22.0;

    let header_grey = egui::Color32::from_rgb(140, 140, 140);
    let header_active = egui::Color32::from_rgb(210, 210, 210);

    let active_col = state.sort_col;
    let active_dir = state.sort_dir;
    let filter_role = state.role;

    let col_label = |col: SortColumn, base: &str| -> egui::RichText {
        if active_col == col {
            let arrow = if active_dir == SortDir::Asc {
                " ^"
            } else {
                " v"
            };
            egui::RichText::new(format!("{}{}", base, arrow))
                .small()
                .color(header_active)
        } else {
            egui::RichText::new(base).small().color(header_grey)
        }
    };
    let show_nation = results.iter().any(|staff| staff.nation_id.is_some());
    let show_club = results.iter().any(|staff| staff.club_id.is_some());
    let static_label = |base: &str| egui::RichText::new(base).small().color(header_grey);

    let mut sort_clicked: Option<SortColumn> = None;
    ui.horizontal(|ui| {
        if ui
            .add_sized(
                [COL_NAME, ROW_HEIGHT],
                egui::Button::new(col_label(SortColumn::Name, &ui.tr("Name"))).frame(false),
            )
            .clicked()
        {
            sort_clicked = Some(SortColumn::Name);
        }
        if show_nation {
            ui.add_sized(
                [70.0, ROW_HEIGHT],
                egui::Label::new(static_label("Nation UID")),
            );
        }
        if ui
            .add_sized(
                [COL_ROLE, ROW_HEIGHT],
                egui::Button::new(col_label(SortColumn::Role, &ui.tr("Role"))).frame(false),
            )
            .clicked()
        {
            sort_clicked = Some(SortColumn::Role);
        }
        if show_club {
            ui.add_sized(
                [80.0, ROW_HEIGHT],
                egui::Label::new(static_label("Club UID")),
            );
        }
        if ui
            .add_sized(
                [COL_UID, ROW_HEIGHT],
                egui::Button::new(col_label(SortColumn::Uid, "UID")).frame(false),
            )
            .clicked()
        {
            sort_clicked = Some(SortColumn::Uid);
        }
        if ui
            .add_sized(
                [COL_CA, ROW_HEIGHT],
                egui::Button::new(col_label(SortColumn::Ca, "CA")).frame(false),
            )
            .clicked()
        {
            sort_clicked = Some(SortColumn::Ca);
        }
        if ui
            .add_sized(
                [COL_PA, ROW_HEIGHT],
                egui::Button::new(col_label(SortColumn::Pa, "PA")).frame(false),
            )
            .clicked()
        {
            sort_clicked = Some(SortColumn::Pa);
        }
        if ui
            .add_sized(
                [COL_AGE, ROW_HEIGHT],
                egui::Button::new(col_label(SortColumn::Age, &ui.tr("Age"))).frame(false),
            )
            .clicked()
        {
            sort_clicked = Some(SortColumn::Age);
        }
    });
    ui.separator();

    if let Some(col) = sort_clicked {
        if state.sort_col == col {
            state.sort_dir = match state.sort_dir {
                SortDir::Asc => SortDir::Desc,
                SortDir::Desc => SortDir::Asc,
            };
        } else {
            state.sort_col = col;
            state.sort_dir = SortDir::Asc;
        }
    }

    if results.is_empty() {
        ui.label(ui.tr("No results."));
        return clicked;
    }

    // Build a sorted index map: order[visual_row] = original results index.
    let mut order: Vec<usize> = (0..results.len()).collect();
    match (state.sort_col, state.sort_dir) {
        (SortColumn::Name, SortDir::Asc) => {
            order.sort_by(|&a, &b| results[a].name.cmp(&results[b].name))
        }
        (SortColumn::Name, SortDir::Desc) => {
            order.sort_by(|&a, &b| results[b].name.cmp(&results[a].name))
        }
        (SortColumn::Uid, SortDir::Asc) => order.sort_by_key(|&i| results[i].uid),
        (SortColumn::Uid, SortDir::Desc) => {
            order.sort_by(|&a, &b| results[b].uid.cmp(&results[a].uid))
        }
        (SortColumn::Ca, SortDir::Asc) => order.sort_by_key(|&i| results[i].ca),
        (SortColumn::Ca, SortDir::Desc) => {
            order.sort_by(|&a, &b| results[b].ca.cmp(&results[a].ca))
        }
        (SortColumn::Pa, SortDir::Asc) => order.sort_by_key(|&i| results[i].pa),
        (SortColumn::Pa, SortDir::Desc) => {
            order.sort_by(|&a, &b| results[b].pa.cmp(&results[a].pa))
        }
        (SortColumn::Age, direction) => order.sort_by(|&a, &b| {
            compare_ages(
                staff_age(schemas, today, results[a]),
                staff_age(schemas, today, results[b]),
                direction,
            )
        }),
        (SortColumn::Role, SortDir::Asc) => order.sort_by(|&a, &b| {
            primary_role(&results[a].roles, filter_role)
                .cmp(&primary_role(&results[b].roles, filter_role))
        }),
        (SortColumn::Role, SortDir::Desc) => order.sort_by(|&a, &b| {
            primary_role(&results[b].roles, filter_role)
                .cmp(&primary_role(&results[a].roles, filter_role))
        }),
    }

    let selection_id = ui.id().with("staff_search_selection");
    let mut selected = ui.ctx().data_mut(|data| data.get_temp::<u64>(selection_id));
    let mut scroll_to = None;
    if crate::ui::navigation::result_keyboard_enabled(ui) && !ui.ctx().wants_keyboard_input() {
        let (up, down, enter) = ui.input(|input| {
            (
                input.key_pressed(egui::Key::ArrowUp),
                input.key_pressed(egui::Key::ArrowDown),
                input.key_pressed(egui::Key::Enter),
            )
        });
        let current = order
            .iter()
            .position(|&i| Some(results[i].addr) == selected);
        if up || down {
            let next = navigation_row(current, order.len(), up);
            if let Some(next) = next {
                selected = Some(results[order[next]].addr);
                scroll_to = Some(next);
            }
        }
        if enter {
            clicked = order
                .iter()
                .copied()
                .find(|&i| Some(results[i].addr) == selected);
        }
    }
    let mut scroll = egui::ScrollArea::vertical();
    if let Some(row) = scroll_to {
        scroll =
            scroll.vertical_scroll_offset(row as f32 * (ROW_HEIGHT + ui.spacing().item_spacing.y));
    }
    scroll.show_rows(ui, ROW_HEIGHT, order.len(), |ui, row_range| {
        for visual_i in row_range {
            let orig_i = order[visual_i];
            let s = results[orig_i];
            let role_text = primary_role(&s.roles, filter_role);
            let age = staff_age(schemas, today, s)
                .map(|age| age.to_string())
                .unwrap_or_else(|| "-".to_string());
            let ca_color = field_widget::ca_pa_color(s.ca);
            let pa_color = field_widget::ca_pa_color(s.pa);

            let row_inner = ui.horizontal(|ui| {
                ui.add_sized([COL_NAME, ROW_HEIGHT], egui::Label::new(&s.name));
                if show_nation {
                    ui.add_sized(
                        [70.0, ROW_HEIGHT],
                        egui::Label::new(
                            s.nation_id
                                .map(|id| id.to_string())
                                .unwrap_or_else(|| "-".to_string()),
                        ),
                    );
                }
                ui.add_sized([COL_ROLE, ROW_HEIGHT], egui::Label::new(role_text));
                if show_club {
                    ui.add_sized(
                        [80.0, ROW_HEIGHT],
                        egui::Label::new(
                            s.club_id
                                .map(|id| id.to_string())
                                .unwrap_or_else(|| "-".to_string()),
                        ),
                    );
                }
                ui.add_sized([COL_UID, ROW_HEIGHT], egui::Label::new(s.uid.to_string()));
                ui.add_sized(
                    [COL_CA, ROW_HEIGHT],
                    egui::Label::new(egui::RichText::new(s.ca.to_string()).color(ca_color)),
                );
                ui.add_sized(
                    [COL_PA, ROW_HEIGHT],
                    egui::Label::new(egui::RichText::new(s.pa.to_string()).color(pa_color)),
                );
                ui.add_sized([COL_AGE, ROW_HEIGHT], egui::Label::new(age));
            });

            let row_resp = ui.interact(
                row_inner.response.rect,
                ui.id().with(("staff_row", s.addr)),
                egui::Sense::click(),
            );
            if selected == Some(s.addr) || row_resp.hovered() {
                ui.painter().rect_filled(
                    row_resp.rect,
                    egui::CornerRadius::ZERO,
                    if selected == Some(s.addr) {
                        ui.visuals().selection.bg_fill.linear_multiply(0.3)
                    } else {
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 20)
                    },
                );
            }
            if row_resp.clicked() {
                selected = Some(s.addr);
                clicked = Some(orig_i);
            }
        }
    });
    ui.ctx().data_mut(|data| {
        if let Some(addr) = selected {
            data.insert_temp(selection_id, addr);
        }
    });

    clicked
}

fn navigation_row(current: Option<usize>, len: usize, up: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match current {
        Some(row) if up => row.saturating_sub(1),
        Some(row) => (row + 1).min(len - 1),
        None if up => len - 1,
        None => 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staff(uid: u32, name: &str, age: Option<u16>) -> CachedStaff {
        CachedStaff {
            addr: u64::from(uid),
            index: 0,
            name: name.to_string(),
            uid,
            ca: 100,
            pa: 150,
            dob_year: age
                .map(|age| {
                    crate::game_version::GameBuild::Fm26Steam
                        .schemas()
                        .age_reference_year
                        - age
                })
                .unwrap_or(0),
            roles: [10; 15],
            role_ptr: 0,
            raw: Vec::new(),
            identity_shift: 0,
            nation_id: None,
            club_id: None,
        }
    }

    #[test]
    fn search_matches_all_name_words_or_exact_uid() {
        let cache = vec![
            staff(12, "Alex Smith", Some(40)),
            staff(123, "Smith Jones", Some(50)),
        ];
        let mut state = StaffSearchState {
            query: " SMITH alex ".into(),
            ..Default::default()
        };
        assert_eq!(
            filter(
                crate::game_version::GameBuild::Fm26Steam.schemas(),
                None,
                &cache,
                &state
            )
            .iter()
            .map(|s| s.uid)
            .collect::<Vec<_>>(),
            vec![12]
        );
        state.query = "12".into();
        assert_eq!(
            filter(
                crate::game_version::GameBuild::Fm26Steam.schemas(),
                None,
                &cache,
                &state
            )
            .iter()
            .map(|s| s.uid)
            .collect::<Vec<_>>(),
            vec![12]
        );
        state.query = "missing".into();
        assert!(
            filter(
                crate::game_version::GameBuild::Fm26Steam.schemas(),
                None,
                &cache,
                &state
            )
            .is_empty()
        );
    }

    #[test]
    fn bounds_are_inclusive_and_unknown_ages_are_excluded_only_with_age_filter() {
        let cache = vec![staff(1, "Known", Some(40)), staff(2, "Unknown", None)];
        let mut state = StaffSearchState {
            ca_min: "100".into(),
            pa_min: "150".into(),
            ..Default::default()
        };
        assert_eq!(
            filter(
                crate::game_version::GameBuild::Fm26Steam.schemas(),
                None,
                &cache,
                &state
            )
            .len(),
            2
        );
        state.age_min = "40".into();
        state.age_max = "40".into();
        assert_eq!(
            filter(
                crate::game_version::GameBuild::Fm26Steam.schemas(),
                None,
                &cache,
                &state
            )
            .iter()
            .map(|s| s.uid)
            .collect::<Vec<_>>(),
            vec![1]
        );
        state.ca_min = "101".into();
        assert!(
            filter(
                crate::game_version::GameBuild::Fm26Steam.schemas(),
                None,
                &cache,
                &state
            )
            .is_empty()
        );
    }

    #[test]
    fn invalid_numeric_filters_fail_closed() {
        let cache = vec![staff(1, "Known", Some(40))];
        for value in ["bad", "-1", "201", "65536", "1.5"] {
            let state = StaffSearchState {
                ca_min: value.into(),
                ..Default::default()
            };
            assert!(numeric_filters(&state).is_err());
            assert!(
                filter(
                    crate::game_version::GameBuild::Fm26Steam.schemas(),
                    None,
                    &cache,
                    &state
                )
                .is_empty()
            );
        }
        for state in [
            StaffSearchState {
                pa_min: "201".into(),
                ..Default::default()
            },
            StaffSearchState {
                age_min: "151".into(),
                ..Default::default()
            },
            StaffSearchState {
                age_max: "bad".into(),
                ..Default::default()
            },
            StaffSearchState {
                age_min: "50".into(),
                age_max: "40".into(),
                ..Default::default()
            },
        ] {
            assert!(numeric_filters(&state).is_err());
            assert!(
                filter(
                    crate::game_version::GameBuild::Fm26Steam.schemas(),
                    None,
                    &cache,
                    &state
                )
                .is_empty()
            );
        }
        let state = StaffSearchState {
            ca_min: " 0 ".into(),
            pa_min: "200".into(),
            age_max: "150".into(),
            ..Default::default()
        };
        assert!(numeric_filters(&state).is_ok());
    }

    #[test]
    fn unknown_ages_sort_last_in_both_directions() {
        let mut ages = vec![None, Some(50), Some(30), None];
        ages.sort_by(|a, b| compare_ages(*a, *b, SortDir::Asc));
        assert_eq!(ages, vec![Some(30), Some(50), None, None]);
        ages.sort_by(|a, b| compare_ages(*a, *b, SortDir::Desc));
        assert_eq!(ages, vec![Some(50), Some(30), None, None]);
    }

    #[test]
    fn reset_clears_all_filters_and_preserves_sort() {
        let mut state = StaffSearchState {
            query: "Alex".into(),
            ca_min: "100".into(),
            pa_min: "150".into(),
            age_min: "30".into(),
            age_max: "60".into(),
            role: Some(2),
            sort_col: SortColumn::Pa,
            sort_dir: SortDir::Desc,
        };
        reset_filters(&mut state);
        assert!(state.query.is_empty());
        assert!(state.ca_min.is_empty());
        assert!(state.pa_min.is_empty());
        assert!(state.age_min.is_empty());
        assert!(state.age_max.is_empty());
        assert!(state.role.is_none());
        assert!(state.sort_col == SortColumn::Pa);
        assert!(state.sort_dir == SortDir::Desc);
    }

    #[test]
    fn navigation_selects_and_clamps_at_edges() {
        assert_eq!(navigation_row(None, 0, false), None);
        assert_eq!(navigation_row(None, 3, false), Some(0));
        assert_eq!(navigation_row(None, 3, true), Some(2));
        assert_eq!(navigation_row(Some(0), 3, true), Some(0));
        assert_eq!(navigation_row(Some(2), 3, false), Some(2));
        assert_eq!(navigation_row(Some(1), 3, true), Some(0));
        assert_eq!(navigation_row(Some(1), 3, false), Some(2));
    }
}

#[cfg(test)]
mod game_date_tests {
    use super::*;
    use crate::game_date::GameDate;
    use crate::game_version::GameBuild;

    fn row(uid: u32, day: u16) -> CachedStaff {
        let schemas = GameBuild::Fm27Steam.schemas();
        let offset = schemas.offset_of("s_dob").unwrap();
        let mut raw = vec![0; schemas.staff_struct_size];
        raw[offset..offset + 2].copy_from_slice(&(day | 0x8000).to_le_bytes());
        raw[offset + 2..offset + 4].copy_from_slice(&1982u16.to_le_bytes());
        CachedStaff {
            addr: uid as u64,
            index: 0,
            name: "Witness".into(),
            uid,
            ca: 100,
            pa: 150,
            dob_year: 1982,
            roles: [10; 15],
            role_ptr: 0,
            raw,
            identity_shift: 0,
            nation_id: None,
            club_id: None,
        }
    }

    #[test]
    fn birthday_changes_column_filter_and_sort_age() {
        let schemas = GameBuild::Fm27Steam.schemas();
        let rows = [row(1, 85), row(2, 86), row(3, 0)];
        let today = Some(GameDate {
            day: 85,
            year: 2026,
        });
        assert_eq!(staff_age(schemas, None, &rows[0]), Some(45));
        assert_eq!(staff_age(schemas, today, &rows[0]), Some(44));
        assert_eq!(staff_age(schemas, today, &rows[1]), Some(43));
        assert_eq!(staff_age(schemas, today, &rows[2]), None);
        let state = StaffSearchState {
            age_min: "44".into(),
            age_max: "44".into(),
            ..Default::default()
        };
        let found = filter(schemas, today, &rows, &state);
        assert_eq!(found.iter().map(|s| s.uid).collect::<Vec<_>>(), [1]);
        assert!(filter(schemas, None, &rows, &state).is_empty());
        assert_eq!(
            compare_ages(
                staff_age(schemas, today, &rows[0]),
                staff_age(schemas, today, &rows[1]),
                SortDir::Asc
            ),
            std::cmp::Ordering::Greater
        );
    }
}
