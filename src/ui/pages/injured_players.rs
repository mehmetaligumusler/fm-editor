//! Injured-players page: lists every active injury from the entity cache
//! with a per-row "heal" action that zeroes the day counts through the
//! acknowledged compare-and-swap path (undoable like any other edit).

use std::collections::HashSet;

use eframe::egui;

use crate::cache::{CachedPlayer, EntityCache, InjuryInfo};
use crate::edit_history::{EditPresentation, EntityKind, EntityRef};
use crate::game_version::GameBuild;
use crate::i18n::UiTr;
use crate::injury::InjuryLayout;
use crate::ui::field_widget::FieldEdit;

#[derive(Default)]
pub struct InjuredPlayersState {
    search: String,
    /// Record addresses healed since the last cache load (greyed out).
    healed: HashSet<u64>,
}

/// One heal request: the entity the history should attribute it to and the
/// compare-and-swap edit that zeroes the injury's day counts.
pub type HealRequest = (EntityRef, FieldEdit);

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
                            .on_hover_text(ui.tr("Zeroes the injury's remaining days through the \
acknowledged compare-and-swap path; undoable, and refused if the game \
changed the values in the meantime. Save in game to make it permanent."))
                            .clicked()
                        {
                            state.healed.insert(injury.record_addr);
                            let mut old_raw = injury.full_days.to_le_bytes().to_vec();
                            old_raw.extend_from_slice(&injury.light_days.to_le_bytes());
                            requests.push((
                                entity_ref(build, player),
                                FieldEdit {
                                    addr: injury.record_addr + layout.full_days,
                                    old_raw,
                                    new_raw: vec![0, 0, 0, 0],
                                    description: format!(
                                        "{}: {}",
                                        ui.tr("Heal injury"),
                                        injury.name
                                    ),
                                    presentation: Some(EditPresentation::new(
                                        ui.tr("Injury days"),
                                        format!("{} / {}", injury.full_days, injury.light_days),
                                        "0 / 0",
                                    )),
                                },
                            ));
                        }
                        ui.end_row();
                    }
                });
        });
    requests
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
