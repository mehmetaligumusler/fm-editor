//! Player search page: FMSE-style filter row + scrollable result grid.
//!
//! Search is now cache-driven: `filter()` runs every frame against the
//! in-memory `EntityCache` — no bridge round-trip needed.

use crate::i18n::UiTr;
use eframe::egui;

use crate::cache::CachedPlayer;
use crate::ui::field_widget;

/// Schema keys for the 15 position familiarity fields (direct, 1-byte each).
/// Order matches the `positions` bitmask: bit 0 = GK, bit 1 = SW, ..., bit 14 = WBR.
/// Offsets are resolved per game version through the supplied game schema.
pub const POSITION_OFFSETS: [(&str, &str); 15] = [
    ("pos_gk", "GK"),
    ("pos_sw", "SW"),
    ("pos_dl", "DL"),
    ("pos_dc", "DC"),
    ("pos_dr", "DR"),
    ("pos_dm", "DM"),
    ("pos_ml", "ML"),
    ("pos_mc", "MC"),
    ("pos_mr", "MR"),
    ("pos_aml", "AML"),
    ("pos_amc", "AMC"),
    ("pos_amr", "AMR"),
    ("pos_st", "ST"),
    ("pos_wbl", "WBL"),
    ("pos_wbr", "WBR"),
];

#[derive(Default, Clone, Copy, PartialEq)]
pub enum SortColumn {
    #[default]
    Uid,
    Name,
    Ca,
    Pa,
    Age,
}

#[derive(Default, Clone, Copy, PartialEq)]
pub enum SortDir {
    #[default]
    Asc,
    Desc,
}

/// All filter inputs for the player-search page.
/// Clone is required so `app.rs` can snapshot state when the advanced modal opens,
/// restoring it on Cancel.
#[derive(Clone)]
pub struct PlayerSearchState {
    // --- Quick filters (visible in filter row) ---
    pub query: String,
    pub club_id: String,
    pub my_clubs: bool,
    pub ca_min: String,
    pub pa_min: String,
    pub age_min: String,
    pub age_max: String,
    // --- Advanced filters (modal only) ---
    pub ca_max: String,
    pub pa_max: String,
    /// Bitmask of selected positions; bit layout matches POSITION_OFFSETS.
    /// 0 = no position filter (show all).
    pub positions: u16,
    /// Inclusive familiarity minimum per position (1..=20), active only when selected.
    pub position_minimums: [u8; 15],
    // --- Sort state ---
    pub sort_col: SortColumn,
    pub sort_dir: SortDir,
}

impl Default for PlayerSearchState {
    fn default() -> Self {
        Self {
            query: String::new(),
            club_id: String::new(),
            my_clubs: false,
            ca_min: String::new(),
            pa_min: String::new(),
            age_min: String::new(),
            age_max: String::new(),
            ca_max: String::new(),
            pa_max: String::new(),
            positions: 0,
            position_minimums: [2; 15],
            sort_col: SortColumn::Pa,
            sort_dir: SortDir::Desc,
        }
    }
}

impl PlayerSearchState {
    /// Reset everything edited by the advanced modal, keeping search and sorting.
    pub fn clear_advanced(&mut self) {
        self.club_id.clear();
        self.ca_min.clear();
        self.ca_max.clear();
        self.pa_min.clear();
        self.pa_max.clear();
        self.age_min.clear();
        self.age_max.clear();
        self.positions = 0;
        self.position_minimums = [2; 15];
    }

    pub fn clear_filters(&mut self) {
        self.query.clear();
        self.my_clubs = false;
        self.clear_advanced();
    }

    pub fn validation_error(&self) -> Option<String> {
        if !self.my_clubs
            && !self.club_id.trim().is_empty()
            && self.club_id.trim().parse::<u32>().is_err()
        {
            return Some("Club ID: enter a whole number from 0 to 4294967295.".into());
        }
        for (label, min, max, limit) in [
            ("CA", &self.ca_min, &self.ca_max, 200u16),
            ("PA", &self.pa_min, &self.pa_max, 200u16),
            ("Age", &self.age_min, &self.age_max, 150u16),
        ] {
            let parse = |value: &str| -> Result<Option<u16>, ()> {
                if value.trim().is_empty() {
                    return Ok(None);
                }
                value
                    .trim()
                    .parse::<u16>()
                    .ok()
                    .filter(|v| *v <= limit)
                    .map(Some)
                    .ok_or(())
            };
            let (Ok(lo), Ok(hi)) = (parse(min), parse(max)) else {
                return Some(format!("{label}: enter a whole number from 0 to {limit}."));
            };
            if matches!((lo, hi), (Some(lo), Some(hi)) if lo > hi) {
                return Some(format!("{label}: minimum must not exceed maximum."));
            }
        }
        for (bit, &(_, label)) in POSITION_OFFSETS.iter().enumerate() {
            if self.positions & (1 << bit) != 0 && !(1..=20).contains(&self.position_minimums[bit])
            {
                return Some(format!(
                    "{label}: minimum familiarity must be from 1 to 20."
                ));
            }
        }
        None
    }
}

/// Count how many advanced filters differ from their default.
/// Used for the badge on the "⚙ Advanced" button.
pub fn advanced_filter_count(state: &PlayerSearchState) -> usize {
    let mut n = usize::from(!state.my_clubs && !state.club_id.trim().is_empty());
    if !state.ca_max.trim().is_empty() {
        n += 1;
    }
    if !state.pa_max.trim().is_empty() {
        n += 1;
    }
    if state.positions != 0 {
        n += 1;
    }
    n
}

/// Filter the full player slice down to matching entries.
/// Runs every frame — all comparisons are on already-decoded cache fields.
pub fn filter<'a>(
    schemas: &'static crate::fields::VersionedSchemas,
    today: Option<crate::game_date::GameDate>,
    cache: &'a [CachedPlayer],
    state: &PlayerSearchState,
) -> Vec<&'a CachedPlayer> {
    filter_with_clubs(schemas, today, cache, state, None)
}

pub fn filter_with_clubs<'a>(
    schemas: &'static crate::fields::VersionedSchemas,
    today: Option<crate::game_date::GameDate>,
    cache: &'a [CachedPlayer],
    state: &PlayerSearchState,
    my_clubs: Option<&crate::cache::MyClubs>,
) -> Vec<&'a CachedPlayer> {
    if (state.my_clubs && my_clubs.is_none())
        || state.validation_error().is_some()
        || (!state.my_clubs && !state.club_id.trim().is_empty() && !club_filter_supported(schemas))
    {
        return Vec::new();
    }
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

    let club_id: Option<u32> = state.club_id.trim().parse().ok();
    let ca_min: Option<u8> = state.ca_min.trim().parse().ok();
    let ca_max: Option<u8> = state.ca_max.trim().parse().ok();
    let pa_min: Option<u8> = state.pa_min.trim().parse().ok();
    let pa_max: Option<u8> = state.pa_max.trim().parse().ok();
    let age_min: Option<u16> = state.age_min.trim().parse().ok();
    let age_max: Option<u16> = state.age_max.trim().parse().ok();

    cache
        .iter()
        .filter(|p| {
            if state.my_clubs {
                let Some(scope) = my_clubs else {
                    return false;
                };
                if ![p.club_id, p.owning_club_id]
                    .into_iter()
                    .flatten()
                    .any(|id| scope.club_ids.contains(&id))
                {
                    return false;
                }
            } else if club_id.is_some_and(|id| p.club_id != Some(id)) {
                return false;
            }
            if let Some(m) = ca_min {
                if p.ca < m {
                    return false;
                }
            }
            if let Some(m) = ca_max {
                if p.ca > m {
                    return false;
                }
            }
            if let Some(m) = pa_min {
                if p.pa < m {
                    return false;
                }
            }
            if let Some(m) = pa_max {
                if p.pa > m {
                    return false;
                }
            }
            if let Some(age) = player_age(schemas, today, p) {
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
            // Selected positions combine with OR; only valid, mapped familiarity
            // values can match, including when the inclusive minimum is 1.
            if state.positions != 0 {
                let any_match = POSITION_OFFSETS.iter().enumerate().any(|(bit, &(key, _))| {
                    if state.positions & (1u16 << bit) == 0 {
                        return false;
                    }
                    schemas
                        .offset_of(key)
                        .and_then(|o| p.raw.get(o).copied())
                        .is_some_and(|value| {
                            (1..=20).contains(&value) && value >= state.position_minimums[bit]
                        })
                });
                if !any_match {
                    return false;
                }
            }
            if let Some(uid) = uid_query {
                return p.uid == uid;
            }
            if name_query_active {
                let name_lower = p.name.to_lowercase();
                if !words.iter().all(|w| name_lower.contains(w.as_str())) {
                    return false;
                }
            }
            true
        })
        .collect()
}

/// Render just the filter row (CA/PA/Age/Name-UID). Always visible,
/// even when a detail panel has taken over the results area.
/// Filtering is live — no submit button needed.
/// Returns true if the Advanced button was clicked.
pub fn filter_row(
    ui: &mut egui::Ui,
    state: &mut PlayerSearchState,
    schemas: Option<&'static crate::fields::VersionedSchemas>,
    my_clubs: Option<&crate::cache::MyClubs>,
) -> bool {
    let mut advanced_clicked = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(ui.tr("Name/UID:"));
        ui.add(egui::TextEdit::singleline(&mut state.query)
            .desired_width(220.0).hint_text(ui.tr("Name or exact UID")))
            .on_hover_text("Live search: all name words must match (case-insensitive), or enter an exact numeric UID. Filters combine with AND; bounds are inclusive.");
    });
    ui.horizontal_wrapped(|ui| {
        ui.horizontal(|ui| {
            ui.label("CA ≥");
            ui.add(egui::TextEdit::singleline(&mut state.ca_min).desired_width(42.0));
        });
        ui.horizontal(|ui| {
            ui.label("PA ≥");
            ui.add(egui::TextEdit::singleline(&mut state.pa_min).desired_width(42.0));
        });
        ui.horizontal(|ui| {
            ui.label("Age ≥");
            ui.add(egui::TextEdit::singleline(&mut state.age_min).desired_width(36.0));
            ui.label("≤");
            ui.add(egui::TextEdit::singleline(&mut state.age_max).desired_width(36.0));
        });
        ui.add_enabled(
            my_clubs.is_some() || state.my_clubs,
            egui::Checkbox::new(&mut state.my_clubs, &ui.tr("My clubs")),
        )
        .on_hover_text(my_clubs.map_or("Managed club unavailable", |c| c.label.as_str()))
        .on_disabled_hover_text(
            "Unavailable: no managed club resolved, incomplete reserve data, or unsupported build.",
        );
        let count = advanced_filter_count(state);
        let label = if count > 0 {
            format!("{} ({count})", ui.tr("Advanced"))
        } else {
            ui.tr("Advanced")
        };
        advanced_clicked = ui.button(label).clicked();
        if ui.button(ui.tr("Reset all")).clicked() {
            state.clear_filters();
        }
    });
    if let Some(error) = state.validation_error() {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    if state.my_clubs && my_clubs.is_none() {
        ui.colored_label(
            ui.visuals().error_fg_color,
            "My clubs is unavailable for this save. Reload Save to retry.",
        );
    }
    if !state.my_clubs
        && !state.club_id.trim().is_empty()
        && !schemas.is_some_and(club_filter_supported)
    {
        ui.colored_label(
            ui.visuals().error_fg_color,
            "Current club filtering is unavailable for this attachment.",
        );
    }
    advanced_clicked
}

/// Render the results grid (headers + scrollable rows). Returns the original
/// (pre-sort) index of a clicked row, if any.
pub fn results_grid(
    schemas: &'static crate::fields::VersionedSchemas,
    today: Option<crate::game_date::GameDate>,
    ui: &mut egui::Ui,
    state: &mut PlayerSearchState,
    results: &[&CachedPlayer],
) -> Option<usize> {
    let mut clicked: Option<usize> = None;

    const COL_NAME: f32 = 200.0;
    const COL_UID: f32 = 80.0;
    const COL_CA: f32 = 50.0;
    const COL_PA: f32 = 50.0;
    const COL_AGE: f32 = 50.0;
    const ROW_HEIGHT: f32 = 22.0;

    let header_grey = egui::Color32::from_rgb(140, 140, 140);
    let header_active = egui::Color32::from_rgb(210, 210, 210);

    // Snapshot sort state so the closure below can read it without borrowing state.
    let active_col = state.sort_col;
    let active_dir = state.sort_dir;

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

    // Update sort state after the horizontal block (avoids borrow conflict).
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
    order.sort_by(|&a, &b| {
        compare_players(
            schemas,
            today,
            results[a],
            results[b],
            state.sort_col,
            state.sort_dir,
        )
    });

    let selection_id = ui.make_persistent_id("player_selected");
    let mut selected: Option<u64> = ui.ctx().data_mut(|d| d.get_temp(selection_id));
    let mut keyboard_row = None;
    if crate::ui::navigation::result_keyboard_enabled(ui) && !ui.ctx().wants_keyboard_input() {
        let (up, down, enter) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::Enter),
            )
        });
        let current = order
            .iter()
            .position(|&i| Some(results[i].addr) == selected);
        if up || down {
            let next = match current {
                Some(i) if up => i.saturating_sub(1),
                Some(i) => (i + 1).min(order.len() - 1),
                None if up => order.len() - 1,
                None => 0,
            };
            selected = Some(results[order[next]].addr);
            keyboard_row = Some(next);
        }
        if enter {
            clicked = order
                .iter()
                .copied()
                .find(|&i| Some(results[i].addr) == selected);
        }
    }

    let mut scroll = egui::ScrollArea::vertical();
    if let Some(row) = keyboard_row {
        scroll =
            scroll.vertical_scroll_offset(row as f32 * (ROW_HEIGHT + ui.spacing().item_spacing.y));
    }
    scroll.show_rows(ui, ROW_HEIGHT, order.len(), |ui, row_range| {
        for visual_i in row_range {
            let orig_i = order[visual_i];
            let p = results[orig_i];
            let age = player_age(schemas, today, p)
                .map(|age| age.to_string())
                .unwrap_or_else(|| "-".into());
            let ca_color = field_widget::ca_pa_color(p.ca);
            let pa_color = field_widget::ca_pa_color(p.pa);

            let row_inner = ui.horizontal(|ui| {
                ui.add_sized(
                    [COL_NAME, ROW_HEIGHT],
                    egui::Label::new(&*p.name).truncate(),
                );
                ui.add_sized([COL_UID, ROW_HEIGHT], egui::Label::new(p.uid.to_string()));
                ui.add_sized(
                    [COL_CA, ROW_HEIGHT],
                    egui::Label::new(egui::RichText::new(p.ca.to_string()).color(ca_color)),
                );
                ui.add_sized(
                    [COL_PA, ROW_HEIGHT],
                    egui::Label::new(egui::RichText::new(p.pa.to_string()).color(pa_color)),
                );
                ui.add_sized([COL_AGE, ROW_HEIGHT], egui::Label::new(age));
            });

            let row_resp = ui.interact(
                row_inner.response.rect,
                ui.make_persistent_id(("player_row", p.addr)),
                egui::Sense::click(),
            );
            if row_resp.hovered() || selected == Some(p.addr) {
                ui.painter().rect_filled(
                    row_resp.rect,
                    egui::CornerRadius::ZERO,
                    if selected == Some(p.addr) {
                        ui.visuals().selection.bg_fill.gamma_multiply(0.5)
                    } else {
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 20)
                    },
                );
            }
            if row_resp.clicked() {
                selected = Some(p.addr);
                clicked = Some(orig_i);
            }
        }
    });
    ui.ctx().data_mut(|d| {
        if let Some(addr) = selected {
            d.insert_temp(selection_id, addr);
        }
    });

    clicked
}

fn compare_players(
    schemas: &'static crate::fields::VersionedSchemas,
    today: Option<crate::game_date::GameDate>,
    a: &CachedPlayer,
    b: &CachedPlayer,
    col: SortColumn,
    dir: SortDir,
) -> std::cmp::Ordering {
    if col == SortColumn::Age {
        match (
            player_age(schemas, today, a).is_none(),
            player_age(schemas, today, b).is_none(),
        ) {
            (true, false) => return std::cmp::Ordering::Greater,
            (false, true) => return std::cmp::Ordering::Less,
            _ => {}
        }
    }
    let order = match col {
        SortColumn::Name => a.name.cmp(&b.name),
        SortColumn::Uid => a.uid.cmp(&b.uid),
        SortColumn::Ca => a.ca.cmp(&b.ca),
        SortColumn::Pa => a.pa.cmp(&b.pa),
        SortColumn::Age => player_age(schemas, today, a).cmp(&player_age(schemas, today, b)),
    };
    let order = if dir == SortDir::Desc {
        order.reverse()
    } else {
        order
    };
    order.then_with(|| a.addr.cmp(&b.addr))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(addr: u64, year: u16) -> CachedPlayer {
        CachedPlayer {
            owning_club_id: None,
            addr,
            index: 0,
            name: "Test Player".into(),
            uid: addr as u32,
            ca: 100,
            pa: 150,
            dob_year: year,
            raw: Vec::new(),
            person_shift: 0,
            nation_id: None,
            club_id: None,
            value: None,
        }
    }

    #[test]
    fn numeric_validation_rejects_invalid_and_reversed_ranges() {
        let mut state = PlayerSearchState::default();
        for bad in ["abc", "-1", "201", "1.5"] {
            state.ca_min = bad.into();
            assert!(state.validation_error().is_some());
            assert!(
                filter(
                    crate::game_version::GameBuild::Fm26Steam.schemas(),
                    None,
                    &[player(1, 2000)],
                    &state
                )
                .is_empty()
            );
        }
        state.ca_min = "150".into();
        state.ca_max = "100".into();
        assert!(state.validation_error().unwrap().contains("minimum"));
        state.ca_min = "100".into();
        state.ca_max = "100".into();
        assert_eq!(
            filter(
                crate::game_version::GameBuild::Fm26Steam.schemas(),
                None,
                &[player(1, 2000)],
                &state
            )
            .len(),
            1
        );
    }

    #[test]
    fn resets_all_modal_fields_without_changing_query_or_sort() {
        let mut state = PlayerSearchState::default();
        state.query = "test".into();
        state.ca_min = "100".into();
        state.ca_max = "180".into();
        state.pa_min = "120".into();
        state.pa_max = "190".into();
        state.age_min = "18".into();
        state.age_max = "30".into();
        state.positions = 3;
        state.clear_advanced();
        assert_eq!(state.query, "test");
        assert_eq!(advanced_filter_count(&state), 0);
        assert!(
            [
                &state.ca_min,
                &state.ca_max,
                &state.pa_min,
                &state.pa_max,
                &state.age_min,
                &state.age_max
            ]
            .iter()
            .all(|s| s.is_empty())
        );
        assert!(state.sort_col == SortColumn::Pa && state.sort_dir == SortDir::Desc);
        state.clear_filters();
        assert!(state.query.is_empty());
    }

    #[test]
    fn unknown_ages_sort_last_both_directions() {
        for dir in [SortDir::Asc, SortDir::Desc] {
            assert_eq!(
                compare_players(
                    crate::game_version::GameBuild::Fm26Steam.schemas(),
                    None,
                    &player(1, 0),
                    &player(2, 2000),
                    SortColumn::Age,
                    dir
                ),
                std::cmp::Ordering::Greater
            );
        }
    }
}

fn player_age(
    schemas: &'static crate::fields::VersionedSchemas,
    today: Option<crate::game_date::GameDate>,
    player: &CachedPlayer,
) -> Option<u16> {
    if today.is_some() {
        return crate::ui::entity_header::age(schemas, today, &player.raw, "p_dob").map(u16::from);
    }
    let year = schemas.age_reference_year;
    (player.dob_year > 0 && player.dob_year <= year).then(|| year - player.dob_year)
}

#[cfg(test)]
mod search_tests {
    use super::*;

    #[test]
    fn query_uses_all_words_or_exact_uid_and_combines_with_filters() {
        let cache = [CachedPlayer {
            owning_club_id: None,
            addr: 1,
            index: 0,
            name: "Ada Example".into(),
            uid: 42,
            ca: 100,
            pa: 150,
            dob_year: 2000,
            raw: vec![],
            person_shift: 0,
            nation_id: None,
            club_id: None,
            value: None,
        }];
        let mut state = PlayerSearchState::default();
        for query in ["EXAMPLE ada", "42", "  ada  "] {
            state.query = query.into();
            assert_eq!(
                filter(
                    crate::game_version::GameBuild::Fm26Steam.schemas(),
                    None,
                    &cache,
                    &state
                )
                .len(),
                1
            );
        }
        for query in ["ada missing", "4"] {
            state.query = query.into();
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
        state.query = "42".into();
        state.pa_min = "151".into();
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
}

#[cfg(test)]
mod position_tests {
    use super::*;

    fn witness(values: &[(usize, u8)]) -> CachedPlayer {
        let schemas = crate::game_version::GameBuild::Fm26Steam.schemas();
        let mut raw = vec![0; schemas.player_struct_size];
        for &(bit, value) in values {
            raw[schemas.offset_of(POSITION_OFFSETS[bit].0).unwrap()] = value;
        }
        CachedPlayer {
            owning_club_id: None,
            addr: 1,
            index: 0,
            name: "Witness".into(),
            uid: 1,
            ca: 100,
            pa: 150,
            dob_year: 2000,
            raw,
            person_shift: 0,
            nation_id: None,
            club_id: None,
            value: None,
        }
    }

    #[test]
    fn inclusive_per_position_minima_and_or_semantics() {
        let schemas = crate::game_version::GameBuild::Fm26Steam.schemas();
        let cache = [witness(&[(0, 1), (1, 20)]), witness(&[(0, 10), (1, 19)])];
        let mut state = PlayerSearchState::default();
        assert_eq!(state.position_minimums, [2; 15]);
        assert_eq!(filter(schemas, None, &cache, &state).len(), 2);
        state.position_minimums = [20; 15];
        assert_eq!(filter(schemas, None, &cache, &state).len(), 2);
        assert_eq!(advanced_filter_count(&state), 0);
        state.position_minimums = [2; 15];
        state.positions = 1;
        assert_eq!(filter(schemas, None, &cache, &state).len(), 1);
        state.position_minimums[0] = 1;
        assert_eq!(filter(schemas, None, &cache, &state).len(), 2);
        state.position_minimums[0] = 11;
        assert!(filter(schemas, None, &cache, &state).is_empty());
        state.positions |= 1 << 1;
        state.position_minimums[1] = 20;
        let matches = filter(schemas, None, &cache, &state);
        assert_eq!(matches.len(), 1);
        assert!(std::ptr::eq(matches[0], &cache[0]));
    }

    #[test]
    fn missing_unmapped_and_corrupt_familiarity_never_match() {
        let schemas = crate::game_version::GameBuild::Fm26Steam.schemas();
        let mut state = PlayerSearchState::default();
        state.positions = 1;
        state.position_minimums[0] = 1;
        for value in [0, 21, 255] {
            assert!(filter(schemas, None, &[witness(&[(0, value)])], &state).is_empty());
        }
        let mut missing = witness(&[(0, 20)]);
        missing.raw.clear();
        assert!(filter(schemas, None, &[missing], &state).is_empty());
        static UNMAPPED: crate::fields::VersionedSchemas = crate::fields::VersionedSchemas {
            player_fields: &[],
            ..crate::fields::fm26::FM26
        };
        assert!(filter(&UNMAPPED, None, &[witness(&[(0, 20)])], &state).is_empty());
    }

    #[test]
    fn selected_minima_validate_and_reset_and_cancel_restores_snapshot() {
        let mut state = PlayerSearchState::default();
        state.positions = 1 << 14;
        state.position_minimums[14] = 20;
        let snapshot = state.clone();
        for invalid in [0, 21, 255] {
            state.position_minimums[14] = invalid;
            assert!(state.validation_error().is_some());
        }
        state.positions = 0;
        assert!(state.validation_error().is_none());
        state = snapshot.clone(); // Same complete-state restoration used by Cancel.
        assert_eq!(state.positions, 1 << 14);
        assert_eq!(state.position_minimums[14], 20);
        state.clear_advanced();
        assert_eq!(state.positions, 0);
        assert_eq!(state.position_minimums, [2; 15]);
        state.position_minimums = [20; 15];
        state.clear_filters();
        assert_eq!(state.position_minimums, [2; 15]);
    }
}

/// The Game Pass schema is inherited and must not imply verified support.
pub fn club_filter_supported(schemas: &'static crate::fields::VersionedSchemas) -> bool {
    !std::ptr::eq(
        schemas,
        crate::game_version::GameBuild::Fm27GamePass.schemas(),
    ) && schemas.player_sub_records.iter().any(|sr| sr.id == "club")
}

#[cfg(test)]
mod club_filter_tests {
    use super::*;
    use crate::game_version::GameBuild;
    fn row(uid: u32, club_id: Option<u32>) -> CachedPlayer {
        CachedPlayer {
            owning_club_id: None,
            addr: 0x20000 + uid as u64,
            index: 0,
            name: "Witness".into(),
            uid,
            ca: 100,
            pa: 150,
            dob_year: 2000,
            raw: vec![],
            person_shift: 0,
            nation_id: None,
            club_id,
            value: None,
        }
    }
    #[test]
    fn exact_current_club_combines_with_other_filters() {
        let rows = [row(1, Some(1751)), row(2, Some(814499)), row(3, None)];
        let mut state = PlayerSearchState {
            club_id: " 1751 ".into(),
            ..Default::default()
        };
        let schemas = GameBuild::Fm27Steam.schemas();
        assert_eq!(
            filter(schemas, None, &rows, &state)
                .iter()
                .map(|p| p.uid)
                .collect::<Vec<_>>(),
            [1]
        );
        state.query = "2".into();
        assert!(filter(schemas, None, &rows, &state).is_empty());
        state.query.clear();
        state.ca_min = "101".into();
        assert!(filter(schemas, None, &rows, &state).is_empty());
        state.clear_filters();
        assert_eq!(filter(schemas, None, &rows, &state).len(), 3);
    }
    #[test]
    fn my_clubs_unions_current_and_owned_membership_and_overrides_manual() {
        let mut outgoing = row(3, Some(9999));
        outgoing.owning_club_id = Some(1751);
        let rows = [
            row(1, Some(1751)),
            row(2, Some(814499)),
            outgoing,
            row(4, None),
            row(5, Some(9999)),
        ];
        let scope = crate::cache::MyClubs {
            club_ids: [1751, 814499].into_iter().collect(),
            label: "Huelva".into(),
        };
        let mut state = PlayerSearchState {
            my_clubs: true,
            club_id: "invalid".into(),
            ..Default::default()
        };
        let schemas = GameBuild::Fm27Steam.schemas();
        assert_eq!(
            filter_with_clubs(schemas, None, &rows, &state, Some(&scope))
                .iter()
                .map(|p| p.uid)
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert!(filter_with_clubs(schemas, None, &rows, &state, None).is_empty());
        let other = crate::cache::MyClubs {
            club_ids: [8888].into_iter().collect(),
            label: "Other save".into(),
        };
        assert!(filter_with_clubs(schemas, None, &rows, &state, Some(&other)).is_empty());
        state.ca_min = "101".into();
        assert!(filter_with_clubs(schemas, None, &rows, &state, Some(&scope)).is_empty());
    }
    #[test]
    fn invalid_unknown_and_unsupported_clubs_fail_closed() {
        let rows = [row(1, Some(1751))];
        let mut state = PlayerSearchState::default();
        for value in ["-1", "1.5", "abc", "4294967296"] {
            state.club_id = value.into();
            assert!(state.validation_error().is_some());
            assert!(filter(GameBuild::Fm27Steam.schemas(), None, &rows, &state).is_empty());
        }
        state.club_id = "9999".into();
        assert!(state.validation_error().is_none());
        assert!(filter(GameBuild::Fm27Steam.schemas(), None, &rows, &state).is_empty());
        state.club_id = "1751".into();
        // FM26 maps the current club (player+0x130 -> team+0x30) since 2026-11-01.
        assert_eq!(
            filter(GameBuild::Fm26Steam.schemas(), None, &rows, &state).len(),
            1
        );
        assert!(filter(GameBuild::Fm27GamePass.schemas(), None, &rows, &state).is_empty());
    }
    #[test]
    fn inactive_manual_id_is_preserved_and_not_validated_or_counted() {
        let mut state = PlayerSearchState {
            club_id: "invalid".into(),
            my_clubs: true,
            ..Default::default()
        };
        assert!(state.validation_error().is_none());
        assert_eq!(advanced_filter_count(&state), 0);
        let snapshot = state.clone();
        state.clear_advanced();
        assert!(state.my_clubs);
        assert!(state.club_id.is_empty());
        state = snapshot;
        state.my_clubs = false;
        assert_eq!(state.club_id, "invalid");
        assert!(state.validation_error().is_some());
        state.clear_filters();
        assert!(!state.my_clubs);
    }
}

#[cfg(test)]
mod game_date_tests {
    use super::*;
    use crate::game_date::GameDate;
    use crate::game_version::GameBuild;

    fn row(addr: u64, day: u16) -> CachedPlayer {
        let schemas = GameBuild::Fm27Steam.schemas();
        let offset = schemas.offset_of("p_dob").unwrap();
        let mut raw = vec![0; schemas.player_struct_size];
        raw[offset..offset + 2].copy_from_slice(&(day | 0x8000).to_le_bytes());
        raw[offset + 2..offset + 4].copy_from_slice(&1991u16.to_le_bytes());
        CachedPlayer {
            owning_club_id: None,
            addr,
            index: 0,
            name: "Witness".into(),
            uid: addr as u32,
            ca: 100,
            pa: 150,
            dob_year: 1991,
            raw,
            person_shift: 0,
            nation_id: None,
            club_id: None,
            value: None,
        }
    }

    #[test]
    fn birthday_changes_column_filter_and_sort_age() {
        let schemas = GameBuild::Fm27Steam.schemas();
        let rows = [row(1, 195), row(2, 196), row(3, 0)];
        let today = Some(GameDate {
            day: 195,
            year: 2026,
        });
        assert_eq!(player_age(schemas, None, &rows[0]), Some(36));
        assert_eq!(player_age(schemas, today, &rows[0]), Some(35));
        assert_eq!(player_age(schemas, today, &rows[1]), Some(34));
        assert_eq!(player_age(schemas, today, &rows[2]), None);
        let state = PlayerSearchState {
            age_min: "35".into(),
            age_max: "35".into(),
            ..Default::default()
        };
        let found = filter(schemas, today, &rows, &state);
        assert_eq!(found.iter().map(|p| p.uid).collect::<Vec<_>>(), [1]);
        assert!(filter(schemas, None, &rows, &state).is_empty());
        assert_eq!(
            compare_players(
                schemas,
                today,
                &rows[0],
                &rows[1],
                SortColumn::Age,
                SortDir::Asc
            ),
            std::cmp::Ordering::Greater
        );
        for dir in [SortDir::Asc, SortDir::Desc] {
            assert_eq!(
                compare_players(schemas, today, &rows[2], &rows[0], SortColumn::Age, dir),
                std::cmp::Ordering::Greater
            );
        }
    }
}
