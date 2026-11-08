//! Club lineup and position depth. This page never emits writes.
use crate::best_xi::{self, Candidate, FORMATIONS, Lineup};
use crate::cache::EntityCache;
use crate::fields::VersionedSchemas;
use crate::i18n::UiTr;
use eframe::egui;
use std::sync::Arc;

pub struct BestXiState {
    pub club: Option<u32>,
    formation: usize,
    growth: bool,
    minimum: u8,
    club_query: String,
    bench_only: bool,
    source: Option<Arc<EntityCache>>,
    lineup: Option<Lineup>,
}

impl Default for BestXiState {
    fn default() -> Self {
        Self {
            club: None,
            formation: 0,
            growth: false,
            minimum: 15,
            club_query: String::new(),
            bench_only: true,
            source: None,
            lineup: None,
        }
    }
}

impl BestXiState {
    /// Never retain a prior save's club selection or indexed solution.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

pub fn show(
    ui: &mut egui::Ui,
    state: &mut BestXiState,
    schemas: &'static VersionedSchemas,
    cache: Option<&Arc<EntityCache>>,
) -> Option<usize> {
    ui.heading(ui.tr("Best XI"));
    let Some(cache) = cache else {
        state.clear();
        ui.label("Load a save to build a lineup.");
        return None;
    };
    if !super::player_search::club_filter_supported(schemas) {
        state.clear();
        ui.label(
            "Best XI is unavailable: current club membership is not supported for this build.",
        );
        return None;
    }
    let new_source = !state
        .source
        .as_ref()
        .is_some_and(|old| Arc::ptr_eq(old, cache));
    if new_source {
        state.lineup = None;
        state.source = Some(cache.clone());
        if state
            .club
            .is_some_and(|id| !cache.clubs.iter().any(|c| c.uid == id))
        {
            state.club = None;
        }
    }
    let previous = (state.club, state.formation, state.growth, state.minimum);
    ui.horizontal_wrapped(|ui| {
        ui.label(ui.tr("Club:"));
        let label = state
            .club
            .and_then(|id| cache.club_names.get(&id))
            .map(String::from)
            .unwrap_or_else(|| ui.tr("Managed clubs"));
        egui::ComboBox::from_id_salt("best_xi_club").selected_text(label).width(185.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show_ui(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut state.club_query).hint_text("Name or club UID"));
            if ui.selectable_value(&mut state.club, None, &ui.tr("Managed clubs")).clicked() { ui.memory_mut(|m| m.close_popup()); }
            let query = state.club_query.trim().to_lowercase();
            let mut clubs: Vec<_> = cache.clubs.iter().filter(|c| query.is_empty() || c.name.to_lowercase().contains(&query) || c.uid.to_string() == query).collect();
            clubs.sort_by(|a, b| a.name.cmp(&b.name).then(a.uid.cmp(&b.uid)));
            egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                for c in clubs.iter().take(150) {
                    if ui.selectable_value(&mut state.club, Some(c.uid), format!("{} ({})", c.name, c.uid)).clicked() { ui.memory_mut(|m| m.close_popup()); }
                }
                if clubs.len() > 150 { ui.weak("Type a name or club UID to narrow the list."); }
            });
        });
        ui.label(ui.tr("Tactic:"));
        egui::ComboBox::from_id_salt("best_xi_formation").selected_text(FORMATIONS[state.formation].name).show_ui(ui, |ui| {
            for (i, f) in FORMATIONS.iter().enumerate() { ui.selectable_value(&mut state.formation, i, f.name); }
        });
        ui.toggle_value(&mut state.growth, &ui.tr("Room to grow")).on_hover_text("Off: strongest positional attributes. On: remaining PA − CA, then positional fit. Headroom estimates growth capacity, not development speed; late-bloomer status is unknown.");
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(ui.tr("Minimum familiarity:"));
        ui.add(egui::DragValue::new(&mut state.minimum).range(1..=20))
            .on_hover_text("15 = accomplished; 20 = natural");
        ui.checkbox(&mut state.bench_only, &ui.tr("Alternatives outside XI"));
    });
    if state.club.is_none() {
        if let Some(clubs) = &cache.my_clubs {
            ui.label(&clubs.label);
        } else {
            ui.label(ui.tr("Managed clubs unavailable. Select a club by name or UID."));
            return None;
        }
    }
    ui.weak(ui.tr("Cached squad snapshot")).on_hover_text("Current club membership, including youth. Away loans excluded. Reload Save to refresh. Fitness, injuries, suspensions and registration eligibility are not checked.");
    if state.lineup.is_none()
        || previous != (state.club, state.formation, state.growth, state.minimum)
    {
        state.lineup = Some(best_xi::calculate(
            schemas,
            cache,
            state.club,
            state.formation,
            state.growth,
            state.minimum,
        ));
    }
    let lineup = state.lineup.as_ref()?;
    let filled = lineup.selected.iter().flatten().count();
    let mode = if state.growth {
        ui.tr("Growth headroom")
    } else {
        ui.tr("Current strength")
    };
    ui.strong(format!(
        "{mode} · {filled}/11 {} · {} {}",
        ui.tr("positions filled"),
        lineup.pool_size,
        ui.tr("players in scope")
    )).on_hover_text("Each player is used once: maximize filled positions, then total score. Strength uses heuristic attribute weights × familiarity / 20, not FM match-engine ratings; foot and roles are not modeled. Growth uses PA − CA capacity, not a development rate.");
    let mut clicked = None;
    egui::ScrollArea::vertical()
        .id_salt("best_xi_content")
        .show(ui, |ui| {
            if filled < 11 {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    format!("{} ({filled}/11)", ui.tr("Incomplete XI")),
                );
            }
            if ui.available_width() >= 620.0 {
                pitch(ui, cache, lineup, state, &mut clicked);
            }
            ui.add_space(8.0);
            ui.heading(ui.tr("Position depth")).on_hover_text(
                "Alternatives are ranked independently per slot and can appear in several lists.",
            );
            let columns = if ui.available_width() >= 1000.0 {
                3
            } else if ui.available_width() >= 650.0 {
                2
            } else {
                1
            };
            for row in (0..11).collect::<Vec<_>>().chunks(columns) {
                ui.columns(columns, |cols| {
                    for (col, &slot) in row.iter().enumerate() {
                        egui::Frame::group(cols[col].style()).show(&mut cols[col], |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.strong(FORMATIONS[state.formation].slots[slot].label);
                            if let Some(c) = &lineup.selected[slot] {
                                player(
                                    ui,
                                    schemas,
                                    cache,
                                    c,
                                    state.growth,
                                    "First choice",
                                    &mut clicked,
                                );
                            } else {
                                ui.label(ui.tr("No eligible player"));
                            }
                            ui.separator();
                            let alternatives: Vec<_> = lineup.ranked[slot]
                                .iter()
                                .filter(|c| {
                                    lineup.selected[slot].is_none_or(|s| s.player != c.player)
                                        && (!state.bench_only
                                            || !lineup
                                                .selected
                                                .iter()
                                                .flatten()
                                                .any(|s| s.player == c.player))
                                })
                                .take(3)
                                .collect();
                            if alternatives.is_empty() {
                                ui.weak(ui.tr("No eligible alternatives"));
                            }
                            for (rank, c) in alternatives.iter().enumerate() {
                                let elsewhere = lineup
                                    .selected
                                    .iter()
                                    .position(|s| s.is_some_and(|s| s.player == c.player));
                                let label = elsewhere.map_or_else(
                                    || format!("{} {}", ui.tr("Cover"), rank + 1),
                                    |s| {
                                        format!(
                                            "{} {}",
                                            ui.tr("Starts at"),
                                            FORMATIONS[state.formation].slots[s].label
                                        )
                                    },
                                );
                                player(ui, schemas, cache, c, state.growth, &label, &mut clicked);
                            }
                        });
                    }
                });
                ui.add_space(6.0);
            }
        });
    clicked
}

/// Cached name, or the UID when the name could not be read.
fn display_name(p: &crate::cache::CachedPlayer) -> String {
    if p.name.is_empty() {
        format!("Player {}", p.uid)
    } else {
        p.name.clone()
    }
}

fn player(
    ui: &mut egui::Ui,
    schemas: &'static VersionedSchemas,
    cache: &EntityCache,
    c: &Candidate,
    growth: bool,
    label: &str,
    clicked: &mut Option<usize>,
) {
    let p = &cache.players[c.player];
    ui.weak(label);
    if ui.add(egui::Link::new(display_name(p))).clicked() {
        *clicked = Some(c.player);
    }
    ui.label(format!(
        "{} {:.1} · {} {:.1} · {} {}/20",
        ui.tr("Fit"),
        c.fit,
        ui.tr("attributes"),
        c.rating,
        ui.tr("familiarity"),
        c.familiarity
    ));
    let value = |key| best_xi::ability(schemas, p, key).map_or("?".into(), |v| v.to_string());
    let room = c.headroom.map_or("?".into(), |v| format!("+{v}"));
    ui.label(format!(
        "CA {} · PA {} · {} {room}",
        value("p_ca"),
        value("p_pa"),
        ui.tr("room")
    ));
    if growth {
        let trait_value =
            |key| best_xi::attribute(schemas, p, key).map_or("?".into(), |v| v.to_string());
        ui.label(format!("Pro {} · Det {} · Amb {}", trait_value("p_professionalism"), trait_value("determination"), trait_value("p_ambition")))
            .on_hover_text("Professionalism, determination and ambition (1–20). Context for development, not a growth forecast.");
        ui.weak(if p.dob_year > 0 {
            format!("Born {}", p.dob_year)
        } else {
            "Birth year unknown".into()
        });
    }
    ui.add_space(4.0);
}

fn pitch(
    ui: &mut egui::Ui,
    cache: &EntityCache,
    lineup: &Lineup,
    state: &BestXiState,
    clicked: &mut Option<usize>,
) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().min(1000.0), 600.0),
        egui::Sense::hover(),
    );
    let field = crate::ui::pitch::paint(ui.painter(), rect);
    for (i, slot) in FORMATIONS[state.formation].slots.iter().enumerate() {
        let center = crate::ui::pitch::point(field, slot.x, slot.y);
        let card = egui::Rect::from_center_size(center, egui::vec2(field.width() * 0.23, 72.0));
        let selected = lineup.selected[i];
        let color = if selected.is_none() {
            crate::ui::pitch::UNFILLED
        } else if slot.position == "pos_gk" {
            crate::ui::pitch::KEEPER
        } else {
            crate::ui::pitch::OUTFIELD
        };
        let kit =
            egui::Rect::from_center_size(center - egui::vec2(0.0, 20.0), egui::vec2(54.0, 30.0));
        crate::ui::pitch::shirt(ui.painter(), kit, slot.label, color);
        let score = selected.map_or_else(
            || "Unfilled".to_owned(),
            |c| {
                if state.growth {
                    c.headroom
                        .map_or("? room".into(), |room| format!("+{room} room"))
                } else {
                    format!("{:.1} {}", c.fit, ui.tr("fit"))
                }
            },
        );
        let score_rect = egui::Rect::from_center_size(
            center + egui::vec2(0.0, 4.0),
            egui::vec2(card.width(), 17.0),
        );
        ui.painter()
            .rect_filled(score_rect, 3.0, egui::Color32::from_rgb(19, 44, 33));
        ui.painter().text(
            score_rect.center(),
            egui::Align2::CENTER_CENTER,
            score,
            egui::FontId::proportional(12.0),
            egui::Color32::WHITE,
        );
        let name_rect = egui::Rect::from_center_size(
            center + egui::vec2(0.0, 24.0),
            egui::vec2(card.width(), 22.0),
        );
        if let Some(c) = selected {
            let name = display_name(&cache.players[c.player]);
            let response = ui
                .put(name_rect, egui::Button::new(&name).truncate())
                .on_hover_text(&name);
            let kit_response = ui
                .interact(kit, ui.id().with(("pitch_kit", i)), egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(&name);
            if response.clicked() || kit_response.clicked() {
                *clicked = Some(c.player);
            }
        } else {
            ui.put(
                name_rect,
                egui::Label::new(
                    egui::RichText::new("No eligible player").color(egui::Color32::WHITE),
                )
                .truncate(),
            );
        }
    }
    // Positioned widgets must not move the following depth cards into the pitch.
    ui.advance_cursor_after_rect(rect);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacing_or_removing_cache_discards_the_previous_solution() {
        let ctx = egui::Context::default();
        let s = crate::game_version::GameBuild::Fm27Steam.schemas();
        let empty = || {
            Arc::new(EntityCache {
                my_clubs: Some(crate::cache::MyClubs {
                    club_ids: Default::default(),
                    label: "Test club".into(),
                }),
                injuries: Default::default(),
                players: vec![],
                staff: vec![],
                clubs: vec![],
                club_names: Default::default(),
            })
        };
        let mut state = BestXiState::default();
        let first = empty();
        let second = empty();
        for cache in [Some(&first), Some(&second), None] {
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    show(ui, &mut state, s, cache);
                });
            });
            assert_eq!(state.source.is_some(), cache.is_some());
            if let Some(cache) = cache {
                assert!(Arc::ptr_eq(state.source.as_ref().unwrap(), cache));
            } else {
                assert!(state.lineup.is_none());
            }
        }
    }

    #[test]
    fn unsupported_builds_do_not_reuse_a_lineup() {
        let ctx = egui::Context::default();
        let cache = Arc::new(EntityCache {
            my_clubs: None,
            injuries: Default::default(),
            players: vec![],
            staff: vec![],
            clubs: vec![],
            club_names: Default::default(),
        });
        let mut state = BestXiState::default();
        state.source = Some(cache.clone());
        state.lineup = Some(Lineup {
            pool_size: 0,
            selected: vec![None; 11],
            ranked: vec![vec![]; 11],
        });
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                show(
                    ui,
                    &mut state,
                    crate::game_version::GameBuild::Fm27GamePass.schemas(),
                    Some(&cache),
                );
            });
        });
        assert!(state.source.is_none());
        assert!(state.lineup.is_none());
    }

    fn text_contents(shape: &egui::Shape, texts: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => texts.push(text.galley.text().to_owned()),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    text_contents(shape, texts);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn pitch_reserves_full_height_and_keeps_cards_separate_for_all_formations() {
        let cache = EntityCache {
            my_clubs: None,
            injuries: Default::default(),
            players: vec![],
            staff: vec![],
            clubs: vec![],
            club_names: Default::default(),
        };
        let lineup = Lineup {
            pool_size: 0,
            selected: vec![None; 11],
            ranked: vec![vec![]; 11],
        };
        for width in [636.0, 1200.0] {
            for formation in 0..FORMATIONS.len() {
                let ctx = egui::Context::default();
                let mut clicked = None;
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 800.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let start = ui.cursor().top();
                            let available = ui.available_rect_before_wrap();
                            pitch(
                                ui,
                                &cache,
                                &lineup,
                                &BestXiState {
                                    formation,
                                    ..Default::default()
                                },
                                &mut clicked,
                            );
                            assert!(ui.cursor().top() >= start + 600.0);
                            assert!(ui.min_rect().right() <= available.right());
                            let field = egui::Rect::from_min_size(
                                available.min,
                                egui::vec2(available.width().min(1000.0), 600.0),
                            )
                            .shrink(14.0);
                            let cards: Vec<_> = FORMATIONS[formation]
                                .slots
                                .iter()
                                .map(|slot| {
                                    egui::Rect::from_center_size(
                                        crate::ui::pitch::point(field, slot.x, slot.y),
                                        egui::vec2(field.width() * 0.23, 72.0),
                                    )
                                })
                                .collect();
                            for (i, card) in cards.iter().enumerate() {
                                assert!(field.contains_rect(*card));
                                assert!(
                                    cards
                                        .iter()
                                        .skip(i + 1)
                                        .all(|other| !card.intersects(*other))
                                );
                            }
                        });
                    },
                );
                let mut texts = Vec::new();
                for shape in output.shapes {
                    text_contents(&shape.shape, &mut texts);
                }
                assert_eq!(
                    texts
                        .iter()
                        .filter(|text| text.as_str() == "Unfilled")
                        .count(),
                    11
                );
                for slot in FORMATIONS[formation].slots {
                    assert!(texts.iter().any(|text| text == slot.label));
                }
                assert!(clicked.is_none());
            }
        }
    }

    #[test]
    fn selected_shirt_and_name_navigate_with_fit_and_growth_scores() {
        let schemas = crate::game_version::GameBuild::Fm27Steam.schemas();
        let mut raw = vec![0; schemas.player_struct_size];
        for field in schemas.player_fields {
            if let Some(crate::fields::FieldLocation::Direct { offset, encoding }) = field.offset {
                match encoding {
                    crate::fields::Encoding::Direct => raw[offset as usize] = 20,
                    crate::fields::Encoding::Encoded => raw[offset as usize] = 98,
                    _ => {}
                }
            }
        }
        for (key, value) in [("p_ca", 100u16), ("p_pa", 150u16)] {
            let loc = schemas.field(key).unwrap().offset.unwrap();
            let offset = loc.offset() as usize;
            raw[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        let name = "A very long footballer name that must not overflow adjacent positions";
        let cache = EntityCache {
            my_clubs: None,
            injuries: Default::default(),
            players: vec![crate::cache::CachedPlayer {
                addr: 0x20000,
                index: 7,
                uid: 42,
                name: name.into(),
                ca: 100,
                pa: 150,
                dob_year: 2000,
                raw,
                person_shift: 0,
                nation_id: None,
                club_id: Some(42),
                owning_club_id: Some(42),
                value: None,
            }],
            staff: vec![],
            clubs: vec![],
            club_names: Default::default(),
        };
        for width in [636.0, 1200.0] {
            for growth in [false, true] {
                let lineup = best_xi::calculate(schemas, &cache, Some(42), 0, growth, 15);
                let selected = lineup
                    .selected
                    .iter()
                    .position(Option::is_some)
                    .expect("fixture supplies a starter");
                let candidate = lineup.selected[selected].unwrap();
                let expected_score = if growth {
                    "+50 room".into()
                } else {
                    format!("{:.1} fit", candidate.fit)
                };
                for click_name in [false, true] {
                    let ctx = egui::Context::default();
                    let state = BestXiState {
                        growth,
                        ..Default::default()
                    };
                    let mut target = egui::Pos2::ZERO;
                    let mut clicked = None;
                    for frame in 0..3 {
                        let events = match frame {
                            1 => vec![
                                egui::Event::PointerMoved(target),
                                egui::Event::PointerButton {
                                    pos: target,
                                    button: egui::PointerButton::Primary,
                                    pressed: true,
                                    modifiers: Default::default(),
                                },
                            ],
                            2 => vec![egui::Event::PointerButton {
                                pos: target,
                                button: egui::PointerButton::Primary,
                                pressed: false,
                                modifiers: Default::default(),
                            }],
                            _ => vec![],
                        };
                        let output = ctx.run(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(width, 800.0),
                                )),
                                events,
                                ..Default::default()
                            },
                            |ctx| {
                                egui::CentralPanel::default().show(ctx, |ui| {
                                    let field = egui::Rect::from_min_size(
                                        ui.cursor().min,
                                        egui::vec2(ui.available_width().min(1000.0), 600.0),
                                    )
                                    .shrink(14.0);
                                    let slot = FORMATIONS[0].slots[selected];
                                    target = crate::ui::pitch::point(field, slot.x, slot.y)
                                        + egui::vec2(0.0, if click_name { 24.0 } else { -20.0 });
                                    pitch(ui, &cache, &lineup, &state, &mut clicked);
                                });
                            },
                        );
                        let mut texts = Vec::new();
                        for shape in output.shapes {
                            text_contents(&shape.shape, &mut texts);
                        }
                        assert!(texts.iter().any(|text| text == &expected_score));
                        assert!(texts.iter().any(|text| text == name));
                    }
                    assert_eq!(
                        clicked,
                        Some(0),
                        "navigate using cache index, not process index"
                    );
                }
            }
        }
    }

    #[test]
    fn narrow_best_xi_uses_stacked_depth_instead_of_pitch() {
        let ctx = egui::Context::default();
        let cache = Arc::new(EntityCache {
            my_clubs: Some(crate::cache::MyClubs {
                club_ids: Default::default(),
                label: "Test club".into(),
            }),
            injuries: Default::default(),
            players: vec![],
            staff: vec![],
            clubs: vec![],
            club_names: Default::default(),
        });
        let mut state = BestXiState::default();
        for width in [316.0, 600.0] {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 1000.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        assert!(
                            show(
                                ui,
                                &mut state,
                                crate::game_version::GameBuild::Fm27Steam.schemas(),
                                Some(&cache)
                            )
                            .is_none()
                        );
                    });
                },
            );
            let mut texts = Vec::new();
            for shape in output.shapes {
                text_contents(&shape.shape, &mut texts);
            }
            assert!(texts.iter().any(|text| text == "Position depth"));
            assert!(texts.iter().any(|text| text == "No eligible player"));
            assert!(!texts.iter().any(|text| text == "Unfilled"));
        }
    }
}
