//! Injured-players page: lists every active injury from the entity cache
//! with a per-row "heal" action that zeroes the day counts through the
//! acknowledged compare-and-swap path (undoable like any other edit).

use std::collections::HashSet;

use eframe::egui;

use std::time::Instant;

use crate::cache::{CachedPlayer, EntityCache, InjuryInfo};
use crate::edit_history::{
    ActionDraft, ChangeDraft, EditPresentation, EntityKind, EntityRef, UndoEntry,
};
use crate::game_version::GameBuild;
use crate::i18n::UiTr;
use crate::injury::InjuryLayout;

#[derive(Default)]
pub struct InjuredPlayersState {
    search: String,
    /// Record addresses healed since the last cache load (greyed out).
    healed: HashSet<u64>,
}

/// One heal request: the entity the history should attribute it to and the
/// one undoable action that clears the injury + absence vectors.
pub type HealRequest = (EntityRef, ActionDraft);

pub fn show(
    ui: &mut egui::Ui,
    cache: Option<&EntityCache>,
    build: GameBuild,
    layout: Option<&InjuryLayout>,
    state: &mut InjuredPlayersState,
) -> Vec<HealRequest> {
    let mut requests = Vec::new();
    ui.heading(ui.tr("Injured players"));
    ui.add_space(4.0);
    let Some(cache) = cache else {
        ui.label(ui.tr("Waiting for fm.exe…"));
        return requests;
    };
    let Some(layout) = layout else {
        ui.label(ui.tr("Injuries are not mapped for this game version."));
        return requests;
    };
    ui.add(
        egui::TextEdit::singleline(&mut state.search)
            .hint_text(ui.tr("Search injured players: name or exact UID"))
            .desired_width(ui.available_width()),
    );
    ui.add_space(4.0);

    let query = state.search.trim().to_lowercase();
    let exact_uid = (!query.is_empty() && query.bytes().all(|b| b.is_ascii_digit()))
        .then(|| query.parse::<u32>().ok())
        .flatten();

    // (player, injury) pairs, longest layoff first; healed rows disappear.
    let mut rows: Vec<(&CachedPlayer, &InjuryInfo)> = Vec::new();
    for (index, injuries) in cache.injuries.iter() {
        let Some(player) = cache.players.get(*index) else {
            continue;
        };
        if player.name.is_empty() {
            continue;
        }
        for injury in injuries {
            if state.healed.contains(&injury.record_addr) {
                continue;
            }
            let matches = match exact_uid {
                Some(uid) => player.uid == uid,
                None if query.is_empty() => true,
                None => {
                    player.name.to_lowercase().contains(&query)
                        || injury.name.to_lowercase().contains(&query)
                        || player
                            .club_id
                            .and_then(|id| cache.club_names.get(&id))
                            .is_some_and(|n| n.to_lowercase().contains(&query))
                }
            };
            if matches {
                rows.push((player, injury));
            }
        }
    }
    rows.sort_by(|a, b| b.1.full_days.cmp(&a.1.full_days));

    egui::ScrollArea::vertical()
        .id_salt("injured_rows")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if rows.is_empty() {
                ui.label(ui.tr("No injured players right now."));
                return;
            }
            egui::Grid::new("injured_grid")
                .num_columns(5)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    ui.strong(ui.tr("Player"));
                    ui.strong(ui.tr("Club"));
                    ui.strong(ui.tr("Injury"));
                    ui.strong(ui.tr("Start"));
                    ui.strong(ui.tr("Days (out / light)"));
                    ui.end_row();
                    for (player, injury) in rows {
                        ui.strong(&player.name);
                        ui.label(
                            player
                                .club_id
                                .and_then(|id| cache.club_names.get(&id))
                                .map(String::as_str)
                                .unwrap_or("-"),
                        );
                        ui.label(&injury.name);
                        ui.label(format_dmy(injury.start_doy, injury.start_year));
                        ui.label(format!("{0} / {1}", injury.full_days, injury.light_days));
                        if ui
                            .add_enabled(
                                crate::ui::navigation::editing_enabled(ui),
                                egui::Button::new(ui.tr("Heal injury")),
                            )
                            .on_hover_text(ui.tr(
                                "Clears the injury record and the game's absence entry (both keep the player marked injured) through the acknowledged compare-and-swap path; undoable, and refused if the game changed the values in the meantime. Save in game to make it permanent.",
                            ))
                            .clicked()
                        {
                            state.healed.insert(injury.record_addr);
                            requests.push((entity_ref(build, player), heal_draft(ui, layout, injury)));
                        }
                        ui.end_row();
                    }
                });
        });
    requests
}

/// Build the one-action heal draft: clear the injuries vector AND the
/// absence vector of the conditions object. Each vector edit keeps the
/// begin pointer (undo restores the exact original begin/end pair).
fn heal_draft(ui: &mut egui::Ui, layout: &InjuryLayout, injury: &InjuryInfo) -> ActionDraft {
    let mut changes = Vec::new();
    // Injuries vector: end := begin (count 0).
    if let Some(edit) = vec_clear_edit(
        ui,
        injury.conditions_addr + layout.injuries_vec,
        &injury.injuries_vec_raw,
        "Injuries vector",
    ) {
        changes.push(edit);
    }
    // Absence vector ("player is out"): end := begin (count 0).
    if let Some(edit) = vec_clear_edit(
        ui,
        injury.conditions_addr + layout.absences_vec,
        &injury.absences_vec_raw,
        "Absence vector",
    ) {
        changes.push(edit);
    }
    ActionDraft {
        label: format!("{}: {}", ui.tr("Heal injury"), injury.name),
        before: ui.tr("Injured"),
        after: ui.tr("Healthy"),
        changes,
    }
}

/// CAS edit clearing a pointer vector: new = [begin, begin] (end := begin).
/// `vec_raw` is the [begin][end] pair captured at cache-load time; the
/// worker re-verifies it against live memory before writing (CAS).
fn vec_clear_edit(
    ui: &mut egui::Ui,
    addr: u64,
    vec_raw: &[u8],
    field: &'static str,
) -> Option<ChangeDraft> {
    if vec_raw.len() != 16 {
        return None;
    }
    let begin = u64::from_le_bytes(vec_raw[0..8].try_into().ok()?);
    if !crate::player::is_valid_ptr(begin) {
        return None;
    }
    let new_raw = [begin.to_le_bytes(), begin.to_le_bytes()].concat();
    if new_raw == vec_raw {
        return None; // already empty
    }
    Some(ChangeDraft {
        entry: UndoEntry {
            description: format!("{field} @0x{addr:X}"),
            addr,
            old_raw: vec_raw.to_vec(),
            new_raw,
            timestamp: Instant::now(),
            policy_guard: None,
        },
        presentation: EditPresentation::new(ui.tr(field), ui.tr("1 entry"), ui.tr("empty")),
    })
}

fn entity_ref(build: GameBuild, player: &CachedPlayer) -> EntityRef {
    EntityRef {
        kind: EntityKind::Player,
        build,
        address: player.addr,
        index: player.index,
        uid: Some(player.uid),
        name: player.name.clone(),
    }
}

/// "17/9/2026" from the packed date (flag bits above the day are ignored).
fn format_dmy(doy_flags: u16, year: u16) -> String {
    match crate::game_date::dmy_from_packed(doy_flags, year) {
        Some((day, month, year)) => format!("{day}/{month}/{year}"),
        None => format!("doy{}/{year}", doy_flags & 0x1FF),
    }
}
