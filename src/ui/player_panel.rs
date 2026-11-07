//! Player detail panel with identity, attributes, positions, traits and contract data.

use crate::i18n::UiTr;
use eframe::egui;

use crate::fields::{self, Category, Entity};
use crate::player::{self, Absence, GameDate, Injury, ListState, PlayerRecord};
use crate::ratings;
use crate::settings::{WagePeriod, format_wage};
use crate::ui::field_widget::{self, FieldEdit};

#[derive(PartialEq)]
pub enum PlayerTab {
    Overview,
    PositionsTraits,
    Contract,
}

pub fn show(
    schemas: &'static crate::fields::VersionedSchemas,
    today: Option<crate::game_date::GameDate>,
    ui: &mut egui::Ui,
    player: &mut PlayerRecord,
    tab: &mut PlayerTab,
    addr: u64,
    wage_period: WagePeriod,
    face_pack_path: Option<&str>,
    names: &mut crate::ui::name_picker::NamePicker,
) -> Vec<FieldEdit> {
    let mut edits = Vec::new();
    crate::ui::entity_header::show(
        ui,
        player.name.as_deref().unwrap_or("Unknown"),
        "Player",
        player
            .field_u32("p_uid")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "Unknown".into()),
        crate::ui::entity_header::age(schemas, today, &player.raw, "p_dob"),
        player.club_name.as_deref(),
        player.club_addr,
        player.nationality.as_deref(),
    );
    // Sub-tab bar
    ui.horizontal(|ui| {
        ui.selectable_value(tab, PlayerTab::Overview, &ui.tr("Overview"));
        ui.selectable_value(
            tab,
            PlayerTab::PositionsTraits,
            &ui.tr("Positions & Traits"),
        );
        ui.selectable_value(tab, PlayerTab::Contract, &ui.tr("Contract"));
    });
    ui.separator();

    match tab {
        PlayerTab::PositionsTraits => show_positions_traits(schemas, ui, player, addr, &mut edits),
        PlayerTab::Overview => {
            egui::ScrollArea::vertical()
                .id_salt("player_overview")
                .show(ui, |ui| {
                    show_overview(
                        schemas,
                        ui,
                        player,
                        addr,
                        &mut edits,
                        wage_period,
                        face_pack_path,
                        names,
                    );
                });
        }
        PlayerTab::Contract => {
            egui::ScrollArea::vertical()
                .id_salt("player_contract")
                .show(ui, |ui| {
                    show_contract(schemas, ui, player, &mut edits);
                });
        }
    }

    // Widgets address edits as `addr + schema offset`. A player-coach's person
    // block sits `person_shift` bytes later in memory, so move edits that land
    // in it (DOB, traits, ...) to where the game stores them.
    if player.person_shift != 0 {
        let size = schemas.player_struct_size as u64;
        for edit in &mut edits {
            let rel = edit.addr.wrapping_sub(addr);
            if rel < size {
                edit.addr = addr + player::player_phys_offset(schemas, rel, player.person_shift);
            }
        }
    }

    edits
}

// ---------------------------------------------------------------------------
// Overview
// ---------------------------------------------------------------------------

fn show_overview(
    schemas: &'static fields::VersionedSchemas,
    ui: &mut egui::Ui,
    player: &mut PlayerRecord,
    addr: u64,
    edits: &mut Vec<FieldEdit>,
    wage_period: WagePeriod,
    face_pack_path: Option<&str>,
    names: &mut crate::ui::name_picker::NamePicker,
) {
    let id = potential_attributes_id(player, addr);
    let mut preview = ui.data_mut(|data| data.get_temp::<bool>(id).unwrap_or(false));
    if ui
        .checkbox(&mut preview, &ui.tr("Show potential attributes"))
        .on_hover_text("Estimated attributes at PA; read-only, not a growth guarantee.")
        .changed()
    {
        ui.data_mut(|data| data.insert_temp(id, preview));
    }
    // A previous row can expand the scroll area's minimum width. The viewport,
    // not that minimum, determines the number and bounded width of these stacks.
    let width = ui
        .available_width()
        .min((ui.clip_rect().right() - ui.cursor().left()).max(0.0))
        .min(1450.0);
    let count = if width >= 1200.0 {
        5
    } else if width >= 880.0 {
        4
    } else if width >= 720.0 {
        3
    } else if width >= 600.0 {
        2
    } else {
        1
    };
    let assignments: &[&[usize]] = match count {
        5 => &[&[0], &[1], &[2], &[3], &[4]],
        4 => &[&[0], &[1], &[2], &[3, 4]],
        3 => &[&[0], &[1, 3], &[2, 4]],
        2 => &[&[0, 2], &[1, 3, 4]],
        _ => &[&[0, 1, 2, 3, 4]],
    };
    let mut projected = preview.then(|| potential_attribute_raw(player));
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(width);
            ui.columns(count, |cols| {
                for (col, stacks) in cols.iter_mut().zip(assignments) {
                    for &stack in *stacks {
                        if stack == 0 {
                            egui::Frame::group(col.style())
                                .inner_margin(8.0)
                                .show(col, |ui| {
                                    show_personal_details(
                                        schemas,
                                        ui,
                                        player,
                                        addr,
                                        edits,
                                        wage_period,
                                        face_pack_path,
                                        names,
                                    );
                                    show_nationality_ties(schemas, ui, player);
                                });
                            continue;
                        }
                        let categories: &[(&str, Category)] = match stack {
                            1 => &[
                                ("Technical", Category::Technical),
                                ("Set Pieces", Category::SetPieces),
                            ],
                            2 => &[
                                ("Mental", Category::Mental),
                                ("Personality", Category::Personality),
                            ],
                            3 => &[
                                ("Physical", Category::Physical),
                                ("Hidden", Category::Hidden),
                            ],
                            _ => &[
                                ("Goalkeeping", Category::Goalkeeping),
                                ("Footedness", Category::Footedness),
                            ],
                        };
                        for &(heading, category) in categories {
                            egui::Frame::group(col.style())
                                .inner_margin(8.0)
                                .show(col, |ui| {
                                    ui.set_width(ui.available_width());
                                    if let Some(raw) = projected.as_mut() {
                                        ui.add_enabled_ui(false, |ui| {
                                            field_widget::category_column(
                                                schemas,
                                                ui,
                                                heading,
                                                Entity::Player,
                                                category,
                                                raw,
                                                addr,
                                                &mut Vec::new(),
                                            );
                                        });
                                    } else {
                                        field_widget::category_column(
                                            schemas,
                                            ui,
                                            heading,
                                            Entity::Player,
                                            category,
                                            &mut player.raw,
                                            addr,
                                            edits,
                                        );
                                    }
                                });
                            col.add_space(8.0);
                        }
                        if stack == 1 {
                            egui::Frame::group(col.style())
                                .inner_margin(8.0)
                                .show(col, |ui| {
                                    show_reputation(schemas, ui, player, addr, edits);
                                });
                        }
                    }
                }
            });
        },
    );
}

fn show_personal_details(
    schemas: &'static fields::VersionedSchemas,
    ui: &mut egui::Ui,
    player: &mut PlayerRecord,
    addr: u64,
    edits: &mut Vec<FieldEdit>,
    wage_period: WagePeriod,
    face_pack_path: Option<&str>,
    names: &mut crate::ui::name_picker::NamePicker,
) {
    // Look up fields once from the schema; passing them by reference into
    // the widget helpers avoids every panel line hardcoding an offset.
    let p_dob = schemas.field("p_dob").unwrap();
    let p_ca = schemas.field("p_ca").unwrap();
    let p_pa = schemas.field("p_pa").unwrap();

    // Face image — shown when a face pack path is configured and the UID is known.
    let face_uid = player.field_u32("p_uid").filter(|&uid| uid > 0);
    if let (Some(path), Some(uid)) = (face_pack_path, face_uid) {
        let img_path = std::path::Path::new(path).join(format!("{uid}.png"));
        let uri = format!("file:///{}", img_path.to_string_lossy().replace('\\', "/"));
        ui.add(egui::Image::new(uri).max_size(egui::vec2(80.0, 80.0)));
        ui.add_space(4.0);
    }

    // Personal Details
    ui.vertical(|ui| {
        ui.strong(ui.tr("Identity"));
        edits.extend(field_widget::dob_edit(ui, &mut player.raw, &p_dob, addr));
        if let Some(f) = schemas.field("p_height") {
            edits.extend(field_widget::raw_u16_edit(
                ui,
                &f,
                &mut player.raw,
                250,
                addr,
            ));
        }
        if let Some(f) = schemas.field("p_weight") {
            edits.extend(field_widget::raw_u16_edit(
                ui,
                &f,
                &mut player.raw,
                250,
                addr,
            ));
        }
        if let Some(birthplace) = &player.birthplace {
            field_widget::info_label(ui, "Place of Birth", birthplace);
        }
        if let Some(languages) = player.languages.as_deref().filter(|l| !l.is_empty()) {
            field_widget::info_label_wrapped(
                ui,
                "Languages",
                crate::person::format_languages(languages),
            );
        }
        edits.extend(crate::ui::name_picker::names_section(
            schemas,
            ui,
            names,
            &crate::person::PLAYER_NAME_SLOTS,
            &mut player.raw,
            addr,
        ));
    });

    // Status belongs to the person, immediately after identity and names.
    show_player_status(schemas, ui, player, addr, edits);

    // Appearance (shown in the 3D match), when the schema maps it.
    if schemas.offset_of("p_skin_tone").is_some() {
        ui.separator();
        ui.vertical(|ui| {
            ui.strong("Appearance");
            if let Some(f) = schemas.field("p_ethnicity") {
                edits.extend(field_widget::choice_u8_edit(
                    ui,
                    "Ethnicity",
                    &mut player.raw,
                    &f,
                    addr,
                    ETHNICITIES,
                ));
            }
            for (key, max) in [
                ("p_skin_tone", 19),
                ("p_hair_colour", 18),
                ("p_hair_length", 6),
            ] {
                if let Some(f) = schemas.field(key) {
                    edits.extend(field_widget::raw_u8_edit(
                        ui,
                        &f,
                        &mut player.raw,
                        max,
                        addr,
                    ));
                }
            }
        });
    }

    ui.separator();

    // Ability
    ui.vertical(|ui| {
        ui.strong(ui.tr("Ability"));
        edits.extend(field_widget::ca_pa_edit(
            ui,
            &mut player.raw,
            &p_ca,
            &p_pa,
            addr,
        ));
    });

    // Stats
    ui.vertical(|ui| {
        ui.strong(ui.tr("Stats"));
        // Each row shows only when the record's schema maps the field.
        if let Some(apps) = player.field_u16("p_senior_apps") {
            field_widget::info_label(ui, "Snr Apps", apps);
        }
        if let (Some(caps), Some(goals)) = (
            player.field_int("p_intl_caps"),
            player.field_int("p_intl_goals"),
        ) {
            field_widget::info_label(ui, "Intl Caps / Goals", format!("{caps} / {goals}"));
        }
        if let (Some(caps), Some(goals)) = (
            player.field_int("p_u21_caps"),
            player.field_int("p_u21_goals"),
        ) {
            field_widget::info_label(ui, "U21 Caps / Goals", format!("{caps} / {goals}"));
        }
        // FM27 has no struct-level weekly wage; fall back to the contract's.
        if let Some(wage) = player
            .field_u32("p_weekly_wage")
            .or_else(|| player.field_u32("p_contract_wage"))
        {
            field_widget::info_label(
                ui,
                "Wage",
                format_wage(wage, wage_period, crate::i18n::language(ui.ctx())),
            );
        }
    });
}

fn show_reputation(
    schemas: &'static fields::VersionedSchemas,
    ui: &mut egui::Ui,
    player: &mut PlayerRecord,
    addr: u64,
    edits: &mut Vec<FieldEdit>,
) {
    let p_current_rep = schemas.field("p_current_rep").unwrap();
    let p_home_rep = schemas.field("p_home_rep").unwrap();
    let p_world_rep = schemas.field("p_world_rep").unwrap();
    // Reputation
    ui.vertical(|ui| {
        ui.strong(ui.tr("Reputation"));
        if let Some(e) =
            field_widget::raw_u16_edit(ui, &p_current_rep, &mut player.raw, 10000, addr)
        {
            edits.push(e);
        }
        if let Some(e) = field_widget::raw_u16_edit(ui, &p_home_rep, &mut player.raw, 10000, addr) {
            edits.push(e);
        }
        if let Some(e) = field_widget::raw_u16_edit(ui, &p_world_rep, &mut player.raw, 10000, addr)
        {
            edits.push(e);
        }
    });
}

fn potential_attributes_id(player: &PlayerRecord, addr: u64) -> egui::Id {
    egui::Id::new((
        "potential_attributes",
        format!("{:?}", player.build),
        addr,
        player.addr,
        player.field_u32("p_uid"),
    ))
}

/// Projections are encoded into a disposable copy, never the editable record.
fn potential_attribute_raw(player: &PlayerRecord) -> Vec<u8> {
    let mut raw = player.raw.clone();
    for (key, _, projected) in ratings::extrapolate_player(player) {
        let Some(location @ fields::FieldLocation::Direct { .. }) =
            player.build.schemas().field(key).and_then(|f| f.offset)
        else {
            continue;
        };
        if let Ok(offset) = usize::try_from(location.offset()) {
            if let Some(byte) = raw.get_mut(offset) {
                *byte = location.encode_u8(projected);
            }
        }
    }
    raw
}

// ---------------------------------------------------------------------------
// Positions & Traits
// ---------------------------------------------------------------------------

fn show_positions_traits(
    schemas: &'static crate::fields::VersionedSchemas,
    ui: &mut egui::Ui,
    player: &mut PlayerRecord,
    addr: u64,
    edits: &mut Vec<FieldEdit>,
) {
    let height = ui.available_height().max(0.0);
    if ui.available_width() >= 820.0 {
        let pitch_width = (ui.available_width() * 0.4).min(440.0);
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(pitch_width, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(pitch_width);
                    // Only genuinely short viewports need pitch scrolling.
                    egui::ScrollArea::vertical()
                        .id_salt("player_pitch")
                        .max_height(height)
                        .show(ui, |ui| {
                            let heading_height = if height >= 414.0 {
                                ui.strong(ui.tr("Positions"));
                                24.0
                            } else {
                                0.0
                            };
                            show_positions_formation_with_height(
                                schemas,
                                ui,
                                player,
                                addr,
                                edits,
                                (height - heading_height).clamp(340.0, 500.0),
                            );
                        });
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("player_traits")
                        .max_height(height)
                        .show(ui, |ui| {
                            show_traits(schemas, ui, player, addr, edits);
                        });
                },
            );
        });
    } else {
        // Capture the finite viewport before entering the scrolling content.
        egui::ScrollArea::vertical()
            .id_salt("player_positions_stacked")
            .max_height(height)
            .show(ui, |ui| {
                ui.strong(ui.tr("Positions"));
                show_positions_formation_with_height(
                    schemas,
                    ui,
                    player,
                    addr,
                    edits,
                    (height - 24.0).clamp(390.0, 500.0),
                );
                ui.add_space(12.0);
                show_traits(schemas, ui, player, addr, edits);
            });
    }
}

fn show_traits(
    schemas: &'static fields::VersionedSchemas,
    ui: &mut egui::Ui,
    player: &mut PlayerRecord,
    addr: u64,
    edits: &mut Vec<FieldEdit>,
) {
    ui.strong(ui.tr("Traits"));
    show_traits_checkboxes(schemas, ui, player, addr, edits);
}

/// Position familiarities on a responsive pitch, attacking towards the top.
#[cfg(test)]
fn show_positions_formation(
    schemas: &'static crate::fields::VersionedSchemas,
    ui: &mut egui::Ui,
    player: &mut PlayerRecord,
    addr: u64,
    edits: &mut Vec<FieldEdit>,
) {
    let height = ui.available_height().clamp(390.0, 500.0);
    show_positions_formation_with_height(schemas, ui, player, addr, edits, height);
}

fn show_positions_formation_with_height(
    schemas: &'static fields::VersionedSchemas,
    ui: &mut egui::Ui,
    player: &mut PlayerRecord,
    addr: u64,
    edits: &mut Vec<FieldEdit>,
    height: f32,
) {
    // Rows of (left, center, right) position keys. None = empty cell.
    let rows: &[(Option<&str>, Option<&str>, Option<&str>)] = &[
        (None, Some("pos_st"), None),
        (Some("pos_aml"), Some("pos_amc"), Some("pos_amr")),
        (Some("pos_ml"), Some("pos_mc"), Some("pos_mr")),
        (Some("pos_wbl"), Some("pos_dm"), Some("pos_wbr")),
        (Some("pos_dl"), Some("pos_dc"), Some("pos_dr")),
        (None, Some("pos_sw"), None),
        (None, Some("pos_gk"), None),
    ];

    let width = ui.available_width().min(440.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let field = crate::ui::pitch::paint(ui.painter(), rect);
    // On scaled desktops retain the 22px editor, reducing only shirt/gap space.
    let compact = height < 390.0;
    let shirt_height = if height < 360.0 {
        20.0
    } else if compact {
        22.0
    } else {
        24.0
    };
    let cell_height = shirt_height + if compact { 2.0 } else { 4.0 } + 22.0;
    for (row, keys) in rows.iter().enumerate() {
        for (column, key) in [keys.0, keys.1, keys.2].into_iter().enumerate() {
            let Some(key) = key else { continue };
            let center = crate::ui::pitch::point(
                field,
                0.15 + column as f32 * 0.35,
                (cell_height / 2.0 + row as f32 * (field.height() - cell_height) / 6.0)
                    / field.height(),
            );
            let cell = egui::Rect::from_center_size(center, egui::vec2(56.0, cell_height));
            ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(cell)
                    .layout(egui::Layout::top_down(egui::Align::Center)),
            )
            .scope(|ui| {
                ui.spacing_mut().item_spacing.y = if compact { 2.0 } else { 4.0 };
                ui.push_id(key, |ui| {
                    pos_cell(schemas, ui, Some(key), player, addr, edits, shirt_height)
                });
            });
        }
    }
}

fn pos_cell(
    schemas: &'static crate::fields::VersionedSchemas,
    ui: &mut egui::Ui,
    key: Option<&str>,
    player: &mut PlayerRecord,
    addr: u64,
    edits: &mut Vec<FieldEdit>,
    shirt_height: f32,
) {
    match key {
        Some(k) => {
            if let Some(f) = schemas.field(k) {
                let (kit, _) =
                    ui.allocate_exact_size(egui::vec2(46.0, shirt_height), egui::Sense::hover());
                let color = if k == "pos_gk" {
                    crate::ui::pitch::KEEPER
                } else {
                    crate::ui::pitch::OUTFIELD
                };
                crate::ui::pitch::shirt(ui.painter(), kit, f.label, color);
                // The shirt supplies the label; retain the shared guarded editor.
                if let Some(edit) = field_widget::attr_value_compact(ui, &f, &mut player.raw, addr)
                {
                    edits.push(edit);
                }
            }
        }
        None => {
            // Empty cell — reserve space so the grid stays aligned.
            ui.allocate_space(egui::vec2(56.0, 38.0));
        }
    }
}

/// Render traits as checkbox groups arranged in 4 columns to mirror the FM26 UI.
fn show_traits_checkboxes(
    schemas: &'static crate::fields::VersionedSchemas,
    ui: &mut egui::Ui,
    player: &mut PlayerRecord,
    addr: u64,
    edits: &mut Vec<FieldEdit>,
) {
    let location = schemas.field("p_traits").and_then(|field| field.offset);
    let Some((_, old_mask)) = trait_value(&player.raw, location) else {
        ui.label(ui.tr("Traits unavailable for this record or game version."));
        return;
    };
    let mut new_mask = old_mask;

    // Column 1
    const COL1: &[(&str, &[u8])] = &[
        ("General", &[25, 14, 18, 17, 42, 45, 19]),
        ("Defending", &[16, 38, 39, 34, 61, 33]),
        ("Ball Control", &[35, 31, 12, 15, 23, 27]),
    ];
    // Column 2
    const COL2: &[(&str, &[u8])] = &[
        ("Shooting", &[10, 9, 11, 43, 22, 24, 57]),
        ("Movement", &[5, 4, 6, 32, 20, 41, 55]),
        ("Set Pieces", &[51, 56, 37, 46, 62, 63]),
    ];
    // Column 3
    const COL3: &[(&str, &[u8])] = &[(
        "Dribbling / Running",
        &[52, 53, 2, 1, 3, 28, 36, 47, 29, 30, 40, 59, 58, 13],
    )];
    // Column 4
    const COL4: &[(&str, &[u8])] = &[("Passing", &[7, 26, 8, 48, 21, 44, 60, 64])];

    let groups = [COL1, COL2, COL3, COL4];
    crate::ui::entity_header::columns(ui, |index, col| {
        render_trait_column(col, groups[index], &mut new_mask);
    });

    if crate::ui::navigation::editing_enabled(ui) && new_mask != old_mask {
        // Re-resolve the mapping and validate the whole range before copying bytes.
        let location = schemas.field("p_traits").and_then(|field| field.offset);
        let Some((traits_offset, current_mask)) = trait_value(&player.raw, location) else {
            return;
        };
        if current_mask != old_mask {
            return;
        }
        let old_bytes = old_mask.to_le_bytes();
        let new_bytes = new_mask.to_le_bytes();
        let Some(bytes) = player.raw.get_mut(traits_offset..traits_offset + 8) else {
            return;
        };
        bytes.copy_from_slice(&new_bytes);

        let mut added = Vec::new();
        let mut removed = Vec::new();
        for bit in 0..64u8 {
            let o = (old_mask >> bit) & 1;
            let n = (new_mask >> bit) & 1;
            if o != n {
                let name = player::ppm_name(bit + 1);
                if n == 1 {
                    added.push(name);
                } else {
                    removed.push(name);
                }
            }
        }
        let desc = match (added.as_slice(), removed.as_slice()) {
            ([a], []) => format!("Added trait: {a}"),
            ([], [r]) => format!("Removed trait: {r}"),
            (a, r) => format!("Traits: +{} -{}", a.len(), r.len()),
        };

        edits.push(FieldEdit {
            addr: addr + traits_offset as u64,
            old_raw: old_bytes.to_vec(),
            new_raw: new_bytes.to_vec(),
            description: desc,
            presentation: Some(crate::edit_history::EditPresentation::new(
                "Traits",
                trait_names(old_mask),
                trait_names(new_mask),
            )),
        });
    }
}

fn trait_names(mask: u64) -> String {
    let names: Vec<_> = (0..64u8)
        .filter(|bit| mask & (1u64 << bit) != 0)
        .map(|bit| player::ppm_name(bit + 1))
        .collect();
    if names.is_empty() {
        "None".into()
    } else {
        names.join(", ")
    }
}

fn render_trait_column(ui: &mut egui::Ui, groups: &[(&str, &[u8])], mask: &mut u64) {
    for (heading, ppm_ids) in groups {
        ui.strong(ui.tr(heading));
        for &id in *ppm_ids {
            let bit = 1u64 << (id - 1);
            let mut checked = (*mask & bit) != 0;
            if ui
                .add_enabled(
                    crate::ui::navigation::editing_enabled(ui),
                    egui::Checkbox::new(&mut checked, ui.tr(player::ppm_name(id))),
                )
                .changed()
            {
                if checked {
                    *mask |= bit;
                } else {
                    *mask &= !bit;
                }
            }
        }
        ui.add_space(6.0);
    }
}

// ---------------------------------------------------------------------------
// Contract / Relations (placeholder)
// ---------------------------------------------------------------------------

fn show_contract(
    schemas: &'static crate::fields::VersionedSchemas,
    ui: &mut egui::Ui,
    player: &mut PlayerRecord,
    edits: &mut Vec<FieldEdit>,
) {
    ui.strong(ui.tr("Current contract"));
    if player.build.version() == crate::game_version::GameVersion::Fm27 {
        show_unverified_contract_type(ui);
    }

    // Contract fields live in the contract sub-record: edit its bytes and
    // address. Asking price is in the player struct itself.
    if let (Some(cr), Some(caddr)) = (player.contract_raw.as_mut(), player.contract_addr) {
        if let Some(mut f) = schemas.field("p_contract_wage").cloned() {
            f.label = "Wage (GBP/week)";
            edits.extend(field_widget::raw_u32_edit(ui, &f, cr, caddr));
        }
        if let Some(f) = schemas.field("p_contract_expiry") {
            edits.extend(field_widget::date_edit(
                ui,
                "Expiry",
                cr,
                &f,
                caddr,
                2000..=2060,
            ));
        }
        if let Some(f) = schemas.field("p_squad_status") {
            let code = schemas
                .offset_of("p_squad_status")
                .and_then(|o| cr.get(o).copied());
            match code {
                // FM's own statuses (Out On Loan, ...) are shown, not offered.
                Some(c) if !SQUAD_STATUS_OFFERED.iter().any(|(o, _)| *o == c) => {
                    let name =
                        squad_status_name(c).map_or(format!("code {c}"), |n| format!("{n} ({c})"));
                    field_widget::info_label(ui, "Squad Status", name);
                }
                _ => edits.extend(field_widget::choice_u8_edit(
                    ui,
                    "Squad Status",
                    cr,
                    &f,
                    caddr,
                    SQUAD_STATUS_OFFERED,
                )),
            }
        }
        if let Some(f) = schemas.field("p_squad_number") {
            edits.extend(field_widget::optional_u8_edit(
                ui,
                "Squad Number",
                cr,
                &f,
                caddr,
                99,
                255,
            ));
        }
        show_listing(schemas, ui, cr, caddr, player.game_date, edits);
        if let Some(s) = schemas
            .offset_of("p_contract_signed")
            .and_then(|o| fmt_date_at(cr, o))
        {
            field_widget::info_label(ui, "Signed", s);
        }
    } else {
        ui.label("Contract sub-record unavailable.");
    }
    if let Some(f) = schemas.field("p_asking_price") {
        let addr = player.addr;
        edits.extend(field_widget::optional_money_edit(
            ui,
            "Asking Price",
            &mut player.raw,
            &f,
            addr,
        ));
    }

    ui.add_space(4.0);

    if let Some(apps) = player.field_u16("p_senior_apps") {
        field_widget::info_label(ui, "Senior Apps", apps);
    }

    for (key, label) in [
        ("p_contract_start", "Contract Start"),
        ("p_join_date", "Joined Club"),
        ("p_debut_date", "Senior Debut"),
    ] {
        if let Some(s) = fmt_date_field(schemas, &player.raw, key) {
            field_widget::info_label(ui, label, s);
        }
    }
    show_clauses(schemas, ui, player, edits);
}

/// No record, retained selection or edit sink: this unsupported control cannot
/// default an unknown byte to a known type or emit a proposed write.
fn show_unverified_contract_type(ui: &mut egui::Ui) {
    ui.label("Contract type: Unknown (read-only)").on_hover_text(
        "Type conversion is unverified. Only the current contract is mapped, not pending offers or future contracts. Wage and expiry edits do not change type.",
    );
}

/// Medical/list data is loaded by the bridge, never inferred from a cached search row.
fn show_player_status(
    schemas: &'static fields::VersionedSchemas,
    ui: &mut egui::Ui,
    player: &mut PlayerRecord,
    addr: u64,
    edits: &mut Vec<FieldEdit>,
) {
    // Only expose morale on builds with witnessed mappings; use the schema's
    // numeric encoding, not the display-only descriptions of morale levels.
    let layout = schemas.player_lists;
    let morale = schemas
        .field("p_morale")
        .filter(|f| f.offset.is_some() && !schemas.morale_labels.is_empty());
    if layout.is_none() && morale.is_none() {
        return;
    }
    ui.strong(ui.tr("Status"));
    if let Some(field) = morale {
        if let Some(location) = field.offset.as_ref() {
            if let Some(&raw) = player.raw.get(location.offset() as usize) {
                if (1..=20).contains(&location.decode_u8(raw)) {
                    edits.extend(field_widget::attr_edit(ui, field, &mut player.raw, addr));
                } else {
                    field_widget::info_label(ui, "Morale", format!("raw={raw}"));
                }
            }
        }
    }
    if let Some(off) = layout.and_then(|_| schemas.offset_of("p_value")) {
        if let Some(bytes) = player.raw.get(off..off.saturating_add(4)) {
            if let (Ok(bytes), Some(at)) = (bytes.try_into(), addr.checked_add(off as u64)) {
                if let Some(edit) = field_widget::money_edit_at(
                    ui,
                    "Transfer Value",
                    i32::from_le_bytes(bytes),
                    at,
                    false,
                ) {
                    player.raw[off..off + 4].copy_from_slice(&edit.new_raw);
                    edits.push(edit);
                }
            }
        }
    }
    let Some(layout) = layout else {
        return;
    };
    ui.strong("Injuries");
    match &player.injuries {
        ListState::Unmapped => {
            ui.label("Not loaded.");
        }
        ListState::Unreadable => {
            ui.label("Injury data unavailable (read-only).");
        }
        ListState::Items(items) if items.is_empty() => {
            ui.label(ui.tr("None"));
        }
        ListState::Items(items) => {
            for injury in items {
                ui.push_id(("injury", injury.addr), |ui| {
                    ui.label(&injury.name);
                    field_widget::info_label(ui, "Injured on", injury.start.display());
                    field_widget::info_label(
                        ui,
                        "Back in light training",
                        injury.start.add_days(injury.light_days).display(),
                    );
                    field_widget::info_label(
                        ui,
                        "Back in full training",
                        injury.start.add_days(injury.full_days).display(),
                    );
                    let edit = heal_edit(injury, player.game_date);
                    if ui
                        .add_enabled(can_edit(ui) && edit.is_some(), egui::Button::new("Heal"))
                        .clicked()
                    {
                        edits.extend(edit);
                    }
                });
            }
        }
    }
    ui.strong("Unavailability");
    match &player.absences {
        ListState::Unmapped => {
            ui.label("Not loaded.");
        }
        ListState::Unreadable => {
            ui.label("Unavailability data unavailable (read-only).");
        }
        ListState::Items(items) if items.is_empty() => {
            ui.label(ui.tr("None"));
        }
        ListState::Items(items) => {
            for absence in items {
                ui.push_id(("absence", absence.addr), |ui| {
                    let until = (absence.days != u16::MAX)
                        .then(|| absence.start.add_days(absence.days).display());
                    if absence.kind == layout.absence_ban {
                        ui.label(until.map_or_else(
                            || "Suspended (not date-based)".into(),
                            |end| format!("Suspended from {} until {end}", absence.start.display()),
                        ));
                        let edit = lift_ban_edit(absence, player.game_date);
                        if ui
                            .add_enabled(
                                can_edit(ui) && edit.is_some(),
                                egui::Button::new("Lift ban"),
                            )
                            .clicked()
                        {
                            edits.extend(edit);
                        }
                    } else if absence.kind == layout.absence_holiday {
                        ui.label(until.map_or_else(
                            || "On holiday (not date-based)".into(),
                            |end| format!("On holiday until {end}"),
                        ));
                    } else {
                        let label = layout
                            .absence_kinds
                            .iter()
                            .find(|(kind, _)| *kind == absence.kind)
                            .map_or_else(
                                || format!("Unavailable (code {})", absence.kind),
                                |(_, name)| name.to_string(),
                            );
                        ui.label(until.map_or_else(
                            || format!("{label} (not date-based)"),
                            |end| format!("{label} from {} until {end}", absence.start.display()),
                        ));
                    }
                });
            }
        }
    }
}

fn can_edit(ui: &egui::Ui) -> bool {
    ui.is_enabled() && crate::ui::navigation::editing_enabled(ui)
}

fn heal_edit(injury: &Injury, date: Option<GameDate>) -> Option<FieldEdit> {
    let elapsed = date?.elapsed_since(injury.start);
    if elapsed >= injury.full_days {
        return None;
    }
    Some(FieldEdit {
        addr: injury.days_addr,
        old_raw: [
            injury.full_days.to_le_bytes(),
            injury.light_days.to_le_bytes(),
        ]
        .concat(),
        new_raw: [
            elapsed.to_le_bytes(),
            injury.light_days.min(elapsed).to_le_bytes(),
        ]
        .concat(),
        description: format!("Heal injury: {}", injury.name),
        presentation: Some(crate::edit_history::EditPresentation::new(
            format!("Heal injury: {}", injury.name),
            format!(
                "{} full / {} light days remaining",
                injury.full_days - elapsed,
                injury.light_days.saturating_sub(elapsed)
            ),
            "0 full / 0 light days remaining",
        )),
    })
}

fn lift_ban_edit(absence: &Absence, date: Option<GameDate>) -> Option<FieldEdit> {
    if absence.days == u16::MAX {
        return None;
    }
    let elapsed = date?.elapsed_since(absence.start);
    if elapsed >= absence.days {
        return None;
    }
    Some(FieldEdit {
        addr: absence.days_addr,
        old_raw: absence.days.to_le_bytes().to_vec(),
        new_raw: elapsed.to_le_bytes().to_vec(),
        description: "Lift ban".into(),
        presentation: Some(crate::edit_history::EditPresentation::new(
            "Ban",
            format!("{} days remaining", absence.days - elapsed),
            "0 days remaining",
        )),
    })
}

fn show_nationality_ties(
    schemas: &'static fields::VersionedSchemas,
    ui: &mut egui::Ui,
    player: &PlayerRecord,
) {
    let Some(layout) = schemas.player_lists else {
        return;
    };
    ui.separator();
    ui.strong("Nationality & ties");
    match &player.relations {
        ListState::Unmapped => {
            ui.label("Not loaded.");
        }
        ListState::Unreadable => {
            ui.label("Relations unavailable (read-only).");
        }
        ListState::Items(items) => {
            let nations: Vec<_> = items
                .iter()
                .filter(|r| {
                    r.kind == layout.kind_nation && r.relation_type == layout.type_other_nationality
                })
                .map(|r| r.name.as_str())
                .collect();
            field_widget::info_label(
                ui,
                "Other nationalities",
                if nations.is_empty() {
                    "None".into()
                } else {
                    nations.join(", ")
                },
            );
            let clubs: Vec<_> = items
                .iter()
                .filter(|r| {
                    r.kind == layout.kind_club && r.relation_type == layout.type_favoured_club
                })
                .collect();
            if clubs.is_empty() {
                field_widget::info_label(ui, "Favoured clubs", "None");
            } else {
                ui.label("Favoured clubs:");
                for club in clubs {
                    let reason = layout
                        .favoured_club_reasons
                        .iter()
                        .find(|(code, _)| *code == club.reason)
                        .map_or_else(
                            || format!("Reason {}", club.reason),
                            |(_, name)| name.to_string(),
                        );
                    ui.push_id(("favoured_club", club.addr), |ui| {
                        ui.horizontal_wrapped(|ui| {
                            crate::ui::navigation::club_link(
                                ui,
                                Some(&club.name),
                                Some(club.target_addr),
                            );
                            ui.label(format!("({reason})"))
                                .on_hover_text(format!("Strength: {}", club.strength));
                        });
                    });
                }
            }
        }
    }
}

fn contract_offset(schemas: &'static fields::VersionedSchemas, key: &str) -> Option<usize> {
    match schemas.field(key)?.offset? {
        fields::FieldLocation::SubRecord {
            container: "contract",
            offset,
            ..
        } => usize::try_from(offset).ok(),
        _ => None,
    }
}

fn transfer_listing_edit(
    schemas: &'static fields::VersionedSchemas,
    raw: &[u8],
    base: u64,
    listed: bool,
    date: Option<GameDate>,
) -> Option<FieldEdit> {
    let date = date?;
    let start = contract_offset(schemas, "p_list_date")?;
    let flags = contract_offset(schemas, "p_list_flags")?;
    let code = contract_offset(schemas, "p_list_code")?;
    let end = code.checked_add(1)?;
    if flags < start.checked_add(4)? || flags >= code {
        return None;
    }
    let old = raw.get(start..end)?;
    if (raw[flags] & 1 != 0) == listed {
        return None;
    }
    let mut new = old.to_vec();
    new[..4].copy_from_slice(&if listed { date.raw } else { 0x076C0001u32 }.to_le_bytes());
    new[flags - start] = if listed {
        raw[flags] | 1
    } else {
        raw[flags] & !1
    };
    new[code - start] = if listed { 5 } else { 0 };
    Some(FieldEdit {
        addr: base.checked_add(start as u64)?,
        old_raw: old.to_vec(),
        new_raw: new,
        description: format!("Transfer listed: {listed}"),
        presentation: Some(crate::edit_history::EditPresentation::new(
            "Transfer listed",
            raw[flags] & 1 != 0,
            listed,
        )),
    })
}

fn show_listing(
    schemas: &'static fields::VersionedSchemas,
    ui: &mut egui::Ui,
    raw: &mut [u8],
    base: u64,
    date: Option<GameDate>,
    edits: &mut Vec<FieldEdit>,
) {
    let Some(flags_off) = contract_offset(schemas, "p_list_flags") else {
        return;
    };
    let Some(&flags) = raw.get(flags_off) else {
        return;
    };
    // The additional date/code fields distinguish verified editable listings
    // from older schemas whose flag meanings have not been verified.
    if contract_offset(schemas, "p_list_date").is_none()
        || contract_offset(schemas, "p_list_code").is_none()
    {
        return;
    }
    let mut transfer = flags & 1 != 0;
    if ui
        .add_enabled(
            can_edit(ui) && date.is_some(),
            egui::Checkbox::new(&mut transfer, "Transfer listed"),
        )
        .changed()
    {
        if let Some(edit) = transfer_listing_edit(schemas, raw, base, transfer, date) {
            let start = (edit.addr - base) as usize;
            raw[start..start + edit.new_raw.len()].copy_from_slice(&edit.new_raw);
            edits.push(edit);
        }
    }
    let flags = raw[flags_off];
    let mut loan = flags & 2 != 0;
    if ui
        .add_enabled(can_edit(ui), egui::Checkbox::new(&mut loan, "Loan listed"))
        .changed()
    {
        let new = if loan { flags | 2 } else { flags & !2 };
        if let Some(addr) = base.checked_add(flags_off as u64) {
            raw[flags_off] = new;
            edits.push(FieldEdit {
                addr,
                old_raw: vec![flags],
                new_raw: vec![new],
                description: format!("Loan listed: {loan}"),
                presentation: Some(crate::edit_history::EditPresentation::new(
                    "Loan listed",
                    flags & 2 != 0,
                    loan,
                )),
            });
        }
    }
    if raw[flags_off] & 1 != 0 {
        if let Some(date) =
            contract_offset(schemas, "p_list_date").and_then(|off| fmt_date_at(raw, off))
        {
            field_widget::info_label(ui, "Listed since", date);
        }
    }
}

fn show_clauses(
    schemas: &'static fields::VersionedSchemas,
    ui: &mut egui::Ui,
    player: &PlayerRecord,
    edits: &mut Vec<FieldEdit>,
) {
    let Some(layout) = schemas.player_lists else {
        return;
    };
    ui.separator();
    ui.strong("Bonuses & clauses (GBP/event)")
        .on_hover_text("Monetary bonuses use GBP per event; no currency conversion is applied.");
    match &player.clauses {
        ListState::Unmapped => {
            ui.label("Not loaded.");
        }
        ListState::Unreadable => {
            ui.label("Clause data unavailable (read-only).");
        }
        ListState::Items(items) if items.is_empty() => {
            ui.label(ui.tr("None"));
        }
        ListState::Items(items) => {
            for clause in items {
                let label = layout
                    .clause_types
                    .iter()
                    .find(|(code, _)| *code == clause.clause_type)
                    .map_or_else(
                        || format!("Clause type 0x{:02X}", clause.clause_type),
                        |(_, name)| name.to_string(),
                    );
                if layout.clause_amount_types.contains(&clause.clause_type) {
                    ui.push_id(("clause", clause.addr), |ui| {
                        edits.extend(field_widget::money_edit_at(
                            ui,
                            &label,
                            clause.amount,
                            clause.amount_addr,
                            false,
                        ));
                    });
                } else {
                    let value = if layout.clause_percent_types.contains(&clause.clause_type) {
                        format!("{}%", clause.value)
                    } else if layout.clause_year_types.contains(&clause.clause_type) {
                        format!(
                            "{} {}",
                            clause.value,
                            if clause.value == 1 { "year" } else { "years" }
                        )
                    } else {
                        format!("amount={}, value={}", clause.amount, clause.value)
                    };
                    field_widget::info_label(ui, &label, value);
                }
            }
        }
    }
}

/// Squad statuses offered for editing: one code per distinct label. Codes
/// 6, 8, 19 and 21 duplicate a label; 0 ("-"), 23 (Scouting Required) and
/// 24 (Out On Loan) are FM's own and not offered. A stored code outside
/// this list is shown read-only.
const SQUAD_STATUS_OFFERED: &[(u8, &str)] = &[
    (1, "Star Player"),
    (2, "Important Player"),
    (3, "Regular Starter"),
    (4, "Squad Player"),
    (5, "Impact Sub"),
    (7, "Fringe Player"),
    (9, "Emergency Backup"),
    (10, "Breakthrough Prospect"),
    (11, "Future Prospect"),
    (12, "Hot Prospect"),
    (13, "Youngster"),
    (14, "Regular"),
    (15, "First-Choice Goalkeeper"),
    (16, "Cup Goalkeeper"),
    (17, "Domestic Cup Goalkeeper"),
    (18, "Continental Cup Goalkeeper"),
    (20, "Backup"),
    (22, "Surplus to Requirements"),
];

/// FM's ethnicity codes (`p_ethnicity`).
const ETHNICITIES: &[(u8, &str)] = &[
    (0, "Northern European"),
    (1, "Mediterranean/Hispanic"),
    (2, "North African/Middle Eastern"),
    (3, "African/Caribbean"),
    (4, "Asian"),
    (5, "South East Asian"),
    (6, "Pacific Islander"),
    (7, "Native American"),
    (8, "Native Australian"),
    (9, "Mixed Race"),
    (10, "East Asian"),
];

/// FM's label for a squad-status code (contract "agreed playing time").
/// Codes from FM's Squad Status list; several codes share a label.
fn squad_status_name(code: u8) -> Option<&'static str> {
    Some(match code {
        1 => "Star Player",
        2 => "Important Player",
        3 => "Regular Starter",
        4 | 6 => "Squad Player",
        5 => "Impact Sub",
        7 | 8 => "Fringe Player",
        9 | 21 => "Emergency Backup",
        10 => "Breakthrough Prospect",
        11 => "Future Prospect",
        12 => "Hot Prospect",
        13 => "Youngster",
        14 => "Regular",
        15 => "First-Choice Goalkeeper",
        16 | 19 => "Cup Goalkeeper",
        17 => "Domestic Cup Goalkeeper",
        18 => "Continental Cup Goalkeeper",
        20 => "Backup",
        22 => "Surplus to Requirements",
        23 => "Scouting Required",
        24 => "Out On Loan",
        _ => return None,
    })
}

/// Read a DateDayYear field from `raw` by schema key and format as "DD Mon YYYY".
fn fmt_date_field(
    schemas: &'static crate::fields::VersionedSchemas,
    raw: &[u8],
    key: &str,
) -> Option<String> {
    let off = schemas.field(key)?.offset?.offset() as usize;
    fmt_date_at(raw, off)
}

/// Format a DateDayYear value at byte offset `off` in `buf` as "DD Mon YYYY".
pub(crate) fn fmt_date_at(buf: &[u8], off: usize) -> Option<String> {
    if off + 4 > buf.len() {
        return None;
    }
    // Low 9 bits hold the day; FM keeps flags above them.
    let doy = u16::from_le_bytes(buf[off..off + 2].try_into().ok()?) & 0x1FF;
    let year = u16::from_le_bytes(buf[off + 2..off + 4].try_into().ok()?);
    if year == 0 || doy == 0 || doy > 366 || !(1900..=2050).contains(&year) {
        return None;
    }
    let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
    let mdays: [u16; 12] = if is_leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mnames = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut rem = doy;
    for (i, &d) in mdays.iter().enumerate() {
        if rem <= d {
            return Some(format!("{rem} {} {year}", mnames[i]));
        }
        rem -= d;
    }
    None
}

/// Trait masks must be mapped directly into this buffer, never into a sub-record.
fn trait_value(raw: &[u8], location: Option<fields::FieldLocation>) -> Option<(usize, u64)> {
    let fields::FieldLocation::Direct { offset, .. } = location? else {
        return None;
    };
    let offset = usize::try_from(offset).ok()?;
    let bytes = raw.get(offset..offset.checked_add(8)?)?;
    Some((offset, u64::from_le_bytes(bytes.try_into().ok()?)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_and_expanded_columns_keep_long_personal_values_clear_of_attributes() {
        for width in [916.0, 966.0, 1220.0] {
            let ctx = egui::Context::default();
            let mut player = growing_player(crate::game_version::GameBuild::Fm27Steam);
            let schemas = player.build.schemas();
            let ethnicity = schemas.field("p_ethnicity").unwrap().offset.unwrap();
            player.raw[ethnicity.offset() as usize] = ethnicity.encode_u8(2);
            let dob = schemas.offset_of("p_dob").unwrap();
            player.raw[dob..dob + 2].copy_from_slice(&324u16.to_le_bytes());
            player.raw[dob + 2..dob + 4].copy_from_slice(&1990u16.to_le_bytes());
            let output = profile_frame(
                &ctx,
                &mut player,
                &mut PlayerTab::Overview,
                egui::vec2(width, 5000.0),
                vec![],
            );
            let text = painted_text(&output);
            let heading = |label: &str| text.iter().find(|(s, _, _)| s == label).unwrap().1;
            let identity = heading("Identity");
            let technical = heading("Technical");
            assert!(technical.left() > identity.right());
            for (label, rect, _) in &text {
                if rect.top() >= identity.top() && rect.left() < technical.left() {
                    assert!(
                        rect.right() < technical.left(),
                        "personal {label:?} overlaps attributes at {width}: {rect:?}"
                    );
                }
            }
        }
    }

    fn show_overview_attributes(
        schemas: &'static fields::VersionedSchemas,
        ui: &mut egui::Ui,
        player: &mut PlayerRecord,
        addr: u64,
        edits: &mut Vec<FieldEdit>,
    ) {
        show_overview(
            schemas,
            ui,
            player,
            addr,
            edits,
            WagePeriod::Weekly,
            None,
            &mut crate::ui::name_picker::NamePicker::default(),
        );
    }
    fn fixture_player(build: crate::game_version::GameBuild) -> PlayerRecord {
        PlayerRecord {
            build,
            addr: 0x20000,
            index: 0,
            name: None,
            raw: vec![0; build.schemas().player_struct_size],
            person_shift: 0,
            contract_raw: None,
            contract_addr: None,
            club_name: None,
            club_addr: None,
            nationality: None,
            birthplace: None,
            languages: None,
            game_date: None,
            injuries: ListState::Unmapped,
            absences: ListState::Unmapped,
            relations: ListState::Unmapped,
            clauses: ListState::Unmapped,
        }
    }

    #[test]
    fn trait_metadata_uses_actual_trait_names() {
        assert_eq!(trait_names(0), "None");
        assert_eq!(trait_names(1), player::ppm_name(1));
        assert_eq!(
            trait_names(3),
            format!("{}, {}", player::ppm_name(1), player::ppm_name(2))
        );
        assert_eq!(trait_names(1u64 << 63), player::ppm_name(64));
    }
    #[test]
    fn heal_and_lift_ban_only_shorten_durations() {
        let start = GameDate::from_raw((2025u32 << 16) | 131).unwrap(); // 11 May
        let now = GameDate::from_raw(0x07EA1AC2).unwrap(); // 13 Jul 2026, flags retained
        let mut injury = Injury {
            addr: 0x30000,
            name: "a patella dislocation".into(),
            start,
            full_days: 498,
            light_days: 468,
            days_addr: 0x30028,
        };
        let edit = heal_edit(&injury, Some(now)).unwrap();
        assert_eq!(edit.addr, injury.days_addr);
        let presentation = edit.presentation.as_ref().unwrap();
        assert_eq!(presentation.before, "70 full / 40 light days remaining");
        assert_eq!(presentation.after, "0 full / 0 light days remaining");
        assert!(presentation.group.is_none());
        assert_eq!(
            edit.old_raw,
            [498u16.to_le_bytes(), 468u16.to_le_bytes()].concat()
        );
        assert_eq!(
            edit.new_raw,
            [428u16.to_le_bytes(), 428u16.to_le_bytes()].concat()
        );
        injury.light_days = 200;
        assert_eq!(
            heal_edit(&injury, Some(now)).unwrap().new_raw,
            [428u16.to_le_bytes(), 200u16.to_le_bytes()].concat()
        );
        assert!(heal_edit(&injury, None).is_none());
        injury.full_days = 428;
        assert!(heal_edit(&injury, Some(now)).is_none());
        injury.full_days = 498;
        assert_eq!(
            heal_edit(
                &injury,
                Some(GameDate::from_raw((2024u32 << 16) | 366).unwrap())
            )
            .unwrap()
            .new_raw,
            [0; 4]
        );
        let mut absence = Absence {
            addr: 0x40000,
            kind: 0x19,
            start,
            days: 595,
            days_addr: 0x40016,
        };
        assert_eq!(
            lift_ban_edit(&absence, Some(now)).unwrap().new_raw,
            428u16.to_le_bytes()
        );
        assert!(lift_ban_edit(&absence, None).is_none());
        absence.days = 428;
        assert!(lift_ban_edit(&absence, Some(now)).is_none());
        absence.days = u16::MAX;
        assert!(lift_ban_edit(&absence, Some(now)).is_none());
    }

    #[test]
    fn transfer_listing_bytes_preserve_flags_padding_and_raw_game_date() {
        let schemas = crate::game_version::GameBuild::Fm27Steam.schemas();
        let now = GameDate::from_raw(0x07EA1AC2).unwrap();
        let date = schemas.offset_of("p_list_date").unwrap();
        let flags = schemas.offset_of("p_list_flags").unwrap();
        let code = schemas.offset_of("p_list_code").unwrap();
        let mut raw = vec![0xA5; 0xC4];
        raw[flags] = 0xA2;
        let edit = transfer_listing_edit(schemas, &raw, 0x50000, true, Some(now)).unwrap();
        assert_eq!(edit.addr, 0x50000 + date as u64);
        let presentation = edit.presentation.as_ref().unwrap();
        assert_eq!(presentation.field, "Transfer listed");
        assert_eq!(presentation.before, "false");
        assert_eq!(presentation.after, "true");
        assert!(presentation.group.is_none());
        assert_eq!(edit.new_raw.len(), code + 1 - date);
        assert_eq!(&edit.new_raw[..4], &now.raw.to_le_bytes());
        assert_eq!(&edit.new_raw[4..flags - date], &raw[date + 4..flags]);
        assert_eq!(edit.new_raw[flags - date], 0xA3);
        assert_eq!(edit.new_raw[code - date], 5);
        raw[date..code + 1].copy_from_slice(&edit.new_raw);
        assert!(transfer_listing_edit(schemas, &raw, 0x50000, true, Some(now)).is_none());
        let off = transfer_listing_edit(schemas, &raw, 0x50000, false, Some(now)).unwrap();
        assert_eq!(&off.new_raw[..4], &0x076C0001u32.to_le_bytes());
        assert_eq!(off.new_raw[flags - date], 0xA2);
        assert_eq!(off.new_raw[code - date], 0);
        assert!(transfer_listing_edit(schemas, &raw, 0x50000, false, None).is_none());
        assert!(transfer_listing_edit(schemas, &raw[..code], 0x50000, false, Some(now)).is_none());
        assert!(transfer_listing_edit(schemas, &raw, u64::MAX, false, Some(now)).is_none());
    }

    fn collect_text(shape: &egui::Shape, text: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(t) => text.push(t.galley.text().to_owned()),
            egui::Shape::Vec(shapes) => {
                for s in shapes {
                    collect_text(s, text);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn fm24_shows_only_morale_and_fm26_renders_no_new_player_sections() {
        use crate::game_version::GameBuild;
        for build in [GameBuild::Fm24Steam, GameBuild::Fm26Steam] {
            let ctx = egui::Context::default();
            let mut player = fixture_player(build);
            let mut edits = vec![];
            let mut raw = vec![0; 0xC4];
            let output = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    show_player_status(build.schemas(), ui, &mut player, 0x20000, &mut edits);
                    show_nationality_ties(build.schemas(), ui, &player);
                    show_clauses(build.schemas(), ui, &player, &mut edits);
                    show_listing(build.schemas(), ui, &mut raw, 0x30000, None, &mut edits);
                });
            });
            let mut text = vec![];
            for s in output.shapes {
                collect_text(&s.shape, &mut text);
            }
            if build == GameBuild::Fm24Steam {
                // FM24 shows only the witnessed morale level: no transfer
                // value, injury or absence rows, and no writes from rendering.
                assert!(text.iter().any(|t| t.contains("Morale")), "{text:?}");
                for absent in ["Transfer Value", "Injuries", "Unavailability"] {
                    assert!(
                        !text.iter().any(|t| t.contains(absent)),
                        "{absent}: {text:?}"
                    );
                }
            } else {
                assert!(text.is_empty(), "{build:?}: {text:?}");
            }
            assert!(edits.is_empty());
        }
    }

    #[test]
    fn numeric_morale_edits_preserve_schema_bytes_and_undo_metadata() {
        fn frame(
            ctx: &egui::Context,
            player: &mut PlayerRecord,
            events: Vec<egui::Event>,
        ) -> (egui::FullOutput, Vec<FieldEdit>) {
            let mut edits = Vec::new();
            let output = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        show_player_status(player.build.schemas(), ui, player, 0x20000, &mut edits);
                    });
                },
            );
            (output, edits)
        }
        for build in [
            crate::game_version::GameBuild::Fm24Steam,
            crate::game_version::GameBuild::Fm27Steam,
        ] {
            let ctx = egui::Context::default();
            crate::ui::navigation::set_editing_enabled(&ctx, true);
            let mut player = fixture_player(build);
            let location = build.schemas().field("p_morale").unwrap().offset.unwrap();
            player.raw[location.offset() as usize] = location.encode_u8(10);
            let (output, edits) = frame(&ctx, &mut player, vec![]);
            assert!(edits.is_empty());
            let value = painted_text(&output)
                .into_iter()
                .find(|(s, _, _)| s == "10")
                .unwrap()
                .1
                .center();
            for pressed in [true, false, true, false] {
                let (_, edits) = frame(
                    &ctx,
                    &mut player,
                    vec![
                        egui::Event::PointerMoved(value),
                        egui::Event::PointerButton {
                            pos: value,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
                assert!(edits.is_empty());
            }
            frame(
                &ctx,
                &mut player,
                vec![
                    egui::Event::Key {
                        key: egui::Key::A,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers {
                            ctrl: true,
                            command: true,
                            ..Default::default()
                        },
                    },
                    egui::Event::Text("17".into()),
                ],
            );
            let (_, edits) = frame(
                &ctx,
                &mut player,
                vec![egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert_eq!(edits.len(), 1);
            let edit = &edits[0];
            assert_eq!(edit.addr, 0x20000 + location.offset());
            assert_eq!(edit.old_raw, vec![location.encode_u8(10)]);
            assert_eq!(edit.new_raw, vec![location.encode_u8(17)]);
            assert_eq!(
                player.raw[location.offset() as usize],
                location.encode_u8(17)
            );
            let presentation = edit.presentation.as_ref().unwrap();
            assert_eq!(presentation.field, "Morale");
            assert_eq!(presentation.before, "10");
            assert_eq!(presentation.after, "17");
        }
    }

    #[test]
    fn morale_is_numeric_schema_encoded_and_invalid_values_never_render_clamp() {
        use crate::game_version::GameBuild;
        for build in [GameBuild::Fm24Steam, GameBuild::Fm27Steam] {
            let location = build.schemas().field("p_morale").unwrap().offset.unwrap();
            for value in [0, 1, 10, 20, 255] {
                let ctx = egui::Context::default();
                let mut player = fixture_player(build);
                player.raw[location.offset() as usize] = value;
                let before = player.raw.clone();
                let mut edits = Vec::new();
                let output = ctx.run(Default::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        show_player_status(build.schemas(), ui, &mut player, 0x20000, &mut edits);
                    });
                });
                assert_eq!(player.raw, before);
                assert!(edits.is_empty());
                let text = painted_text(&output);
                let expected = if (1..=20).contains(&location.decode_u8(value)) {
                    location.decode_u8(value).to_string()
                } else {
                    format!("raw={value}")
                };
                assert!(
                    text.iter().any(|(s, _, _)| s == &expected),
                    "{build:?}: {text:?}"
                );
                assert!(!text.iter().any(|(s, _, _)| s == "Okay"
                    || s.contains("Superb")
                    || s.contains("Very Good")));
            }
        }
    }

    #[test]
    fn status_is_in_personal_column_immediately_after_identity() {
        for width in [966.0, 1220.0, 280.0] {
            let ctx = egui::Context::default();
            let mut player = growing_player(crate::game_version::GameBuild::Fm27Steam);
            let output = profile_frame(
                &ctx,
                &mut player,
                &mut PlayerTab::Overview,
                egui::vec2(width, 5000.0),
                vec![],
            );
            let text = painted_text(&output);
            let rect = |label: &str| text.iter().find(|(s, _, _)| s == label).unwrap().1;
            assert_eq!(rect("Status").left(), rect("Identity").left());
            assert!(rect("Status").top() > rect("Identity").bottom());
            assert!(rect("Status").bottom() < rect("Appearance").top());
            assert!(rect("Status").bottom() < rect("Ability").top());
            assert_eq!(text.iter().filter(|(s, _, _)| s == "Status").count(), 1);
        }
    }

    #[test]
    fn fm27_player_sections_distinguish_unloaded_unreadable_and_empty_without_writes() {
        use crate::game_version::GameBuild;
        for width in [1200.0, 280.0] {
            for enabled in [false, true] {
                for state in [0, 1, 2] {
                    let ctx = egui::Context::default();
                    crate::ui::navigation::set_editing_enabled(&ctx, enabled);
                    let mut player = fixture_player(GameBuild::Fm27Steam);
                    player.raw[player.build.schemas().offset_of("p_morale").unwrap()] = 15;
                    player.injuries = match state {
                        0 => ListState::Unmapped,
                        1 => ListState::Unreadable,
                        _ => ListState::Items(vec![]),
                    };
                    player.absences = match state {
                        0 => ListState::Unmapped,
                        1 => ListState::Unreadable,
                        _ => ListState::Items(vec![]),
                    };
                    let original = player.raw.clone();
                    let mut edits = vec![];
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 700.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                show_player_status(
                                    player.build.schemas(),
                                    ui,
                                    &mut player,
                                    0x20000,
                                    &mut edits,
                                );
                            });
                        },
                    );
                    let mut text = vec![];
                    for s in output.shapes {
                        collect_text(&s.shape, &mut text);
                    }
                    let text = text.join(" ");
                    assert!(text.contains("Morale 15"), "{text}");
                    assert!(!text.contains("Level 15"), "{text}");
                    assert!(
                        text.contains(match state {
                            0 => "Not loaded",
                            1 => "unavailable",
                            _ => "None",
                        }),
                        "{text}"
                    );
                    assert!(edits.is_empty());
                    assert_eq!(player.raw, original);
                }
            }
        }
    }

    fn direct(offset: u64) -> Option<fields::FieldLocation> {
        Some(fields::FieldLocation::Direct {
            offset,
            encoding: fields::Encoding::Raw,
        })
    }
    #[test]
    fn formation_fits_desktop_and_narrow_and_rendering_preserves_values() {
        fn text_centers(
            shape: &egui::Shape,
            centers: &mut std::collections::HashMap<String, egui::Pos2>,
        ) {
            match shape {
                egui::Shape::Text(text) => {
                    centers.insert(
                        text.galley.text().to_owned(),
                        text.galley.rect.translate(text.pos.to_vec2()).center(),
                    );
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        text_centers(shape, centers);
                    }
                }
                _ => {}
            }
        }

        let build = crate::game_version::GameBuild::Fm26Steam;
        let schemas = build.schemas();
        for invalid_right_cell in [false, true] {
            for width in [1200.0, 280.0] {
                let ctx = egui::Context::default();
                crate::ui::navigation::set_editing_enabled(&ctx, true);
                let mut player = PlayerRecord {
                    build,
                    addr: 0x20000,
                    index: 0,
                    name: None,
                    raw: vec![0; 1024],
                    person_shift: 0,
                    contract_raw: None,
                    contract_addr: None,
                    club_name: None,
                    club_addr: None,
                    nationality: None,
                    birthplace: None,
                    languages: None,
                    game_date: None,
                    injuries: ListState::Unmapped,
                    absences: ListState::Unmapped,
                    relations: ListState::Unmapped,
                    clauses: ListState::Unmapped,
                };
                for field in schemas.fields_for_category(Entity::Player, Category::Positions) {
                    let loc = field.offset.as_ref().unwrap();
                    player.raw[loc.offset() as usize] = loc.encode_u8(10);
                }
                if invalid_right_cell {
                    let loc = schemas.field("pos_amr").unwrap().offset.unwrap();
                    player.raw[loc.offset() as usize] = 255;
                }
                let original = player.raw.clone();
                let mut centers = std::collections::HashMap::new();
                // Check first and retained layouts, including an invalid raw value.
                for _ in 0..3 {
                    let mut edits = Vec::new();
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 700.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                show_positions_formation(
                                    schemas,
                                    ui,
                                    &mut player,
                                    0x20000,
                                    &mut edits,
                                );
                            });
                        },
                    );
                    assert!(edits.is_empty());
                    assert_eq!(player.raw, original);
                    centers.clear();
                    for shape in &output.shapes {
                        text_centers(&shape.shape, &mut centers);
                    }
                    let aml = centers["AML"];
                    let amc = centers["AMC"];
                    let amr = centers["AMR"];
                    // Text origins are pixel-rounded independently, so their
                    // painted centers can differ by one pixel.
                    let close = |a: f32, b: f32| assert!((a - b).abs() <= 1.0, "{a} != {b}");
                    let pitch_width = (width - 16.0_f32).min(440.0);
                    let column_gap = (pitch_width - 28.0) * 0.35;
                    close(amc.x - aml.x, column_gap);
                    close(amr.x - amc.x, column_gap);
                    close(aml.y, amc.y);
                    close(amc.y, amr.y);
                    assert!(aml.x > 28.0, "left position must fit inside pitch");
                    assert!(amr.x + 28.0 < width, "right control must fit narrow panels");
                    let dc = centers["DC"];
                    let sw = centers["SW"];
                    let gk = centers["GK"];
                    close(sw.x, dc.x);
                    close(sw.x, gk.x);
                    assert!(dc.y < sw.y && sw.y < gk.y);
                    assert!(!centers.contains_key("Sweeper:"));
                    if invalid_right_cell {
                        close(centers["r255"].x, amr.x);
                    }
                }
            }
        }
    }

    fn painted_text(output: &egui::FullOutput) -> Vec<(String, egui::Rect, egui::Rect)> {
        fn collect(
            shape: &egui::Shape,
            clip: egui::Rect,
            out: &mut Vec<(String, egui::Rect, egui::Rect)>,
        ) {
            match shape {
                egui::Shape::Text(t) => out.push((
                    t.galley.text().to_owned(),
                    t.galley.rect.translate(t.pos.to_vec2()),
                    clip,
                )),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, clip, out);
                    }
                }
                _ => {}
            }
        }
        let mut text = Vec::new();
        for s in &output.shapes {
            collect(&s.shape, s.clip_rect, &mut text);
        }
        text
    }

    fn profile_frame(
        ctx: &egui::Context,
        player: &mut PlayerRecord,
        tab: &mut PlayerTab,
        size: egui::Vec2,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let before = player.raw.clone();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add_space(80.0); // The real profile sits below workspace controls.
                    let edits = show(
                        player.build.schemas(),
                        None,
                        ui,
                        player,
                        tab,
                        0x20000,
                        WagePeriod::Weekly,
                        None,
                        &mut crate::ui::name_picker::NamePicker::default(),
                    );
                    assert!(edits.is_empty());
                });
            },
        );
        assert_eq!(player.raw, before);
        output
    }

    #[test]
    fn potential_preview_leaves_ordinary_overview_fields_unchanged() {
        let ctx = egui::Context::default();
        let mut player = growing_player(crate::game_version::GameBuild::Fm27Steam);
        let before = player.raw.clone();
        let mut ordinary = Vec::new();
        for preview in [false, true] {
            ctx.data_mut(|data| {
                data.insert_temp(potential_attributes_id(&player, player.addr), preview)
            });
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1440.0, 4000.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let mut edits = Vec::new();
                        show_overview_attributes(
                            player.build.schemas(),
                            ui,
                            &mut player,
                            0x20000,
                            &mut edits,
                        );
                        assert!(edits.is_empty());
                    });
                },
            );
            let text = painted_text(&output);
            let technical_left = text
                .iter()
                .find(|(s, _, _)| s == "Technical")
                .unwrap()
                .1
                .left();
            ordinary.push(
                text.into_iter()
                    .filter(|(_, r, _)| r.right() < technical_left)
                    .map(|(s, _, _)| s)
                    .collect::<Vec<_>>(),
            );
            assert_eq!(player.raw, before);
        }
        assert_eq!(ordinary[0], ordinary[1]);
        assert!(ordinary[0].iter().any(|s| s == "Ability"));
        assert!(ordinary[0].iter().any(|s| s == "Appearance"));
    }

    #[test]
    fn narrow_overview_scroll_reaches_every_mapped_attribute_and_personal_section() {
        for width in [280.0, 400.0] {
            let ctx = egui::Context::default();
            let mut player = growing_player(crate::game_version::GameBuild::Fm27Steam);
            let mut tab = PlayerTab::Overview;
            let size = egui::vec2(width, 700.0);
            let mut visible = std::collections::HashSet::new();
            for frame in 0..100 {
                let output = profile_frame(
                    &ctx,
                    &mut player,
                    &mut tab,
                    size,
                    if frame == 0 {
                        vec![]
                    } else {
                        scroll_events(egui::pos2(width - 40.0, 600.0), -80.0)
                    },
                );
                for (text, rect, clip) in painted_text(&output) {
                    if clip.contains_rect(rect) {
                        visible.insert(text);
                    }
                }
            }
            for heading in [
                "Identity",
                "Ability",
                "Appearance",
                "Stats",
                "Reputation",
                "Status",
                "Nationality & ties",
                "Technical",
                "Mental",
                "Physical",
                "Set Pieces",
                "Personality",
                "Goalkeeping",
                "Hidden",
                "Footedness",
                "Show potential attributes",
            ] {
                assert!(
                    visible.contains(heading),
                    "unreachable {heading} at {width}"
                );
            }
            for category in [
                Category::Technical,
                Category::Mental,
                Category::Physical,
                Category::SetPieces,
                Category::Personality,
                Category::Goalkeeping,
                Category::Hidden,
                Category::Footedness,
            ] {
                for field in player
                    .build
                    .schemas()
                    .fields_for_category(Entity::Player, category)
                {
                    if field.offset.is_some() {
                        assert!(
                            visible.contains(field.label),
                            "unreachable {} at {width}",
                            field.label
                        );
                    }
                }
            }
        }
    }

    fn populated_positions(invalid: bool) -> PlayerRecord {
        let mut player = fixture_player(crate::game_version::GameBuild::Fm26Steam);
        for (index, field) in player
            .build
            .schemas()
            .fields_for_category(Entity::Player, Category::Positions)
            .iter()
            .enumerate()
        {
            let loc = field.offset.as_ref().unwrap();
            player.raw[loc.offset() as usize] = loc.encode_u8(index as u8 + 1);
        }
        if invalid {
            let loc = player
                .build
                .schemas()
                .field("pos_amr")
                .unwrap()
                .offset
                .unwrap();
            player.raw[loc.offset() as usize] = 255;
        }
        player
    }

    #[test]
    fn full_profile_pitch_fits_bounded_desktop_and_scaled_workspace() {
        for size in [
            egui::vec2(1280.0, 800.0),
            egui::vec2(1000.0, 640.0),
            egui::vec2(1000.0, 590.0),
            egui::vec2(1000.0, 580.0), // ~344px body fits the complete 340px pitch.
        ] {
            for enabled in [false, true] {
                for invalid in [false, true] {
                    let ctx = egui::Context::default();
                    crate::ui::navigation::set_editing_enabled(&ctx, enabled);
                    let mut player = populated_positions(invalid);
                    let mut tab = PlayerTab::PositionsTraits;
                    for _ in 0..3 {
                        let output = profile_frame(&ctx, &mut player, &mut tab, size, vec![]);
                        let text = painted_text(&output);
                        fn pitch_rect(shape: &egui::Shape) -> Option<egui::Rect> {
                            match shape {
                                egui::Shape::Rect(r)
                                    if r.fill == egui::Color32::from_rgb(26, 80, 49) =>
                                {
                                    Some(r.rect)
                                }
                                egui::Shape::Vec(shapes) => shapes.iter().find_map(pitch_rect),
                                _ => None,
                            }
                        }
                        let (pitch, pitch_clip) = output
                            .shapes
                            .iter()
                            .find_map(|s| pitch_rect(&s.shape).map(|rect| (rect, s.clip_rect)))
                            .unwrap();
                        // Check the whole artwork, not just the final numeric box:
                        // the bottom goal/margin was clipped in the real 125% view.
                        assert!(
                            pitch_clip.expand(1.0).contains_rect(pitch),
                            "complete pitch {pitch:?} outside {pitch_clip:?} at {size:?}"
                        );
                        let compact = pitch.height() < 390.0;
                        let shirt_height = if pitch.height() < 360.0 {
                            20.0
                        } else if compact {
                            22.0
                        } else {
                            24.0
                        };
                        let cell_height = shirt_height + if compact { 2.0 } else { 4.0 } + 22.0;
                        let mut cells = Vec::new();
                        for field in player
                            .build
                            .schemas()
                            .fields_for_category(Entity::Player, Category::Positions)
                        {
                            let labels: Vec<_> =
                                text.iter().filter(|(s, _, _)| s == field.label).collect();
                            assert_eq!(labels.len(), 1, "{} must appear once", field.label);
                            let (_, shirt_text, clip) = labels[0];
                            // Artwork anchors the label at 62% of the shirt height.
                            let top = shirt_text.center().y - shirt_height * 0.62;
                            let cell = egui::Rect::from_min_size(
                                egui::pos2(shirt_text.center().x - 28.0, top),
                                egui::vec2(56.0, cell_height),
                            );
                            assert!(
                                clip.expand(1.0).contains_rect(cell),
                                "{} cell {cell:?} outside {clip:?} at {size:?}",
                                field.label
                            );
                            assert!(cell.bottom() <= size.y - 7.0);
                            assert!(
                                pitch.shrink(14.0).expand(1.0).contains_rect(cell),
                                "{} outside playable pitch: {cell:?} / {pitch:?}",
                                field.label
                            );
                            let raw = player.raw[field.offset.as_ref().unwrap().offset() as usize];
                            let display = field.offset.as_ref().unwrap().decode_u8(raw);
                            let value = if display > 20 {
                                format!("r{raw}")
                            } else {
                                display.to_string()
                            };
                            assert!(
                                text.iter().any(|(s, rect, clip)| s == &value
                                    && cell.expand(1.0).contains_rect(*rect)
                                    && clip.expand(1.0).contains_rect(*rect)),
                                "missing value for {} in {cell:?}",
                                field.label
                            );
                            cells.push(cell);
                        }
                        for (i, cell) in cells.iter().enumerate() {
                            for other in &cells[i + 1..] {
                                assert!(!cell.intersects(*other), "overlapping pitch cells");
                            }
                        }
                        let pitch_right = cells.iter().map(|r| r.right()).fold(0.0, f32::max);
                        let traits = text.iter().find(|(s, _, _)| s == "Traits").unwrap();
                        assert!(
                            traits.1.left() > pitch_right,
                            "traits must use the right pane"
                        );
                    }
                }
            }
        }
    }

    fn growing_player(build: crate::game_version::GameBuild) -> PlayerRecord {
        let mut player = fixture_player(build);
        for category in [
            Category::Technical,
            Category::SetPieces,
            Category::Mental,
            Category::Physical,
            Category::Goalkeeping,
            Category::Hidden,
            Category::Personality,
            Category::Footedness,
        ] {
            for field in build
                .schemas()
                .fields_for_category(Entity::Player, category)
            {
                if let Some(loc @ fields::FieldLocation::Direct { .. }) = field.offset {
                    player.raw[loc.offset() as usize] = loc.encode_u8(10);
                }
            }
        }
        for (key, value) in [("p_ca", 100u16), ("p_pa", 150)] {
            let off = build.schemas().offset_of(key).unwrap();
            player.raw[off..off + 2].copy_from_slice(&value.to_le_bytes());
        }
        player
    }

    #[test]
    fn potential_copy_uses_build_encoding_and_preserves_invalid_and_no_growth() {
        use crate::game_version::GameBuild;
        for build in [
            GameBuild::Fm24Steam,
            GameBuild::Fm26Steam,
            GameBuild::Fm27Steam,
        ] {
            let mut player = growing_player(build);
            let original = player.raw.clone();
            let projection = ratings::extrapolate_player(&player);
            assert!(!projection.is_empty());
            let raw = potential_attribute_raw(&player);
            assert_ne!(raw, original);
            for (key, _, new) in projection {
                let loc = build.schemas().field(key).unwrap().offset.unwrap();
                assert_eq!(loc.decode_u8(raw[loc.offset() as usize]), new);
            }
            assert_eq!(player.raw, original);
            let loc = build.schemas().field("passing").unwrap().offset.unwrap();
            player.raw[loc.offset() as usize] = 255;
            assert_eq!(potential_attribute_raw(&player)[loc.offset() as usize], 255);
            for (ca, pa) in [(150u16, 150u16), (160, 100), (0, 150)] {
                for (key, value) in [("p_ca", ca), ("p_pa", pa)] {
                    let off = build.schemas().offset_of(key).unwrap();
                    player.raw[off..off + 2].copy_from_slice(&value.to_le_bytes());
                }
                assert_eq!(potential_attribute_raw(&player), player.raw);
            }
            player.raw.clear();
            assert!(potential_attribute_raw(&player).is_empty());
        }
    }

    #[test]
    fn potential_state_is_bound_to_build_address_and_uid() {
        use crate::game_version::GameBuild;
        let mut player = growing_player(GameBuild::Fm26Steam);
        let id = potential_attributes_id(&player, player.addr);
        assert_ne!(id, potential_attributes_id(&player, player.addr + 8));
        player.addr += 8;
        assert_ne!(id, potential_attributes_id(&player, 0x20000));
        player.addr = 0x20000;
        let off = player.build.schemas().offset_of("p_uid").unwrap();
        player.raw[off..off + 4].copy_from_slice(&123u32.to_le_bytes());
        assert_ne!(id, potential_attributes_id(&player, player.addr));
        player.build = GameBuild::Fm27Steam;
        assert_ne!(id, potential_attributes_id(&player, player.addr));
    }

    #[test]
    fn overview_potential_toggle_replaces_values_without_edits_at_desktop_and_narrow_sizes() {
        for width in [1200.0, 280.0] {
            for enabled in [false, true] {
                let ctx = egui::Context::default();
                crate::ui::navigation::set_editing_enabled(&ctx, enabled);
                let mut player = growing_player(crate::game_version::GameBuild::Fm26Steam);
                let original = player.raw.clone();
                let projected = ratings::extrapolate_player(&player);
                let (key, old, new) = projected
                    .iter()
                    .find(|(key, _, _)| *key == "passing")
                    .copied()
                    .unwrap();
                assert_ne!(old, new);
                let label = player.build.schemas().field(key).unwrap().label;
                let id = potential_attributes_id(&player, player.addr);
                for preview in [false, true, false] {
                    ctx.data_mut(|data| data.insert_temp(id, preview));
                    let mut edits = Vec::new();
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 4000.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                show_overview_attributes(
                                    player.build.schemas(),
                                    ui,
                                    &mut player,
                                    0x20000,
                                    &mut edits,
                                );
                                assert!(
                                    ui.is_enabled(),
                                    "preview must not disable the rest of Overview"
                                );
                            });
                        },
                    );
                    assert!(edits.is_empty());
                    assert_eq!(player.raw, original);
                    let text = painted_text(&output);
                    let toggle = text
                        .iter()
                        .find(|(s, _, _)| s == "Show potential attributes")
                        .unwrap();
                    assert!(toggle.2.contains_rect(toggle.1));
                    let heading = text.iter().find(|(s, _, _)| s == "Technical").unwrap();
                    assert!(toggle.1.bottom() < heading.1.top());
                    let row = text.iter().find(|(s, _, _)| s == label).unwrap();
                    let expected = if preview { new } else { old }.to_string();
                    assert!(
                        text.iter().any(|(s, r, _)| s == &expected
                            && (r.center().y - row.1.center().y).abs() < 2.0
                            && r.left() > row.1.left()),
                        "missing {label}={expected}"
                    );
                    for category in [
                        "Technical",
                        "Set Pieces",
                        "Mental",
                        "Footedness",
                        "Physical",
                        "Personality",
                        "Goalkeeping",
                        "Hidden",
                    ] {
                        assert_eq!(text.iter().filter(|(s, _, _)| s == category).count(), 1);
                    }
                }
            }
        }
    }

    #[test]
    fn overview_toggle_is_reachable_in_full_profile_but_absent_from_positions() {
        for size in [egui::vec2(1200.0, 800.0), egui::vec2(280.0, 700.0)] {
            let ctx = egui::Context::default();
            let mut player = growing_player(crate::game_version::GameBuild::Fm26Steam);
            let mut tab = PlayerTab::Overview;
            let mut reachable = false;
            for _ in 0..25 {
                let output = profile_frame(
                    &ctx,
                    &mut player,
                    &mut tab,
                    size,
                    scroll_events(egui::pos2(size.x - 40.0, size.y - 50.0), -90.0),
                );
                reachable |= painted_text(&output)
                    .iter()
                    .any(|(s, r, c)| s == "Show potential attributes" && c.contains_rect(*r));
            }
            assert!(reachable, "Overview toggle unreachable at {size:?}");
            tab = PlayerTab::PositionsTraits;
            let output = profile_frame(&ctx, &mut player, &mut tab, size, vec![]);
            assert!(
                !painted_text(&output)
                    .iter()
                    .any(|(s, _, _)| s.contains("potential attributes")
                        || s.contains("extrapolation"))
            );
        }
    }

    #[test]
    fn potential_checkbox_and_disabled_grid_ignore_edit_input() {
        fn frame(
            ctx: &egui::Context,
            player: &mut PlayerRecord,
            width: f32,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            let before = player.raw.clone();
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 4000.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let mut edits = Vec::new();
                        show_overview_attributes(
                            player.build.schemas(),
                            ui,
                            player,
                            0x20000,
                            &mut edits,
                        );
                        assert!(edits.is_empty());
                    });
                },
            );
            assert_eq!(player.raw, before);
            output
        }
        fn click(ctx: &egui::Context, player: &mut PlayerRecord, width: f32, pos: egui::Pos2) {
            for pressed in [true, false] {
                frame(
                    ctx,
                    player,
                    width,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
        }
        for width in [1200.0, 280.0] {
            let ctx = egui::Context::default();
            crate::ui::navigation::set_editing_enabled(&ctx, true);
            let mut player = growing_player(crate::game_version::GameBuild::Fm26Steam);
            let id = potential_attributes_id(&player, player.addr);
            let output = frame(&ctx, &mut player, width, vec![]);
            let toggle = painted_text(&output)
                .into_iter()
                .find(|(s, _, _)| s == "Show potential attributes")
                .unwrap()
                .1
                .center();
            click(&ctx, &mut player, width, toggle);
            assert_eq!(ctx.data_mut(|data| data.get_temp::<bool>(id)), Some(true));
            let output = frame(&ctx, &mut player, width, vec![]);
            let text = painted_text(&output);
            let label = text.iter().find(|(s, _, _)| s == "Passing").unwrap().1;
            let value = text
                .iter()
                .find(|(s, rect, _)| {
                    s.parse::<u8>().is_ok()
                        && (rect.center().y - label.center().y).abs() < 2.0
                        && rect.left() > label.right()
                })
                .unwrap()
                .1
                .center();
            click(&ctx, &mut player, width, value);
            click(&ctx, &mut player, width, value);
            frame(
                &ctx,
                &mut player,
                width,
                vec![egui::Event::Text("1".into())],
            );
            frame(
                &ctx,
                &mut player,
                width,
                vec![
                    egui::Event::PointerMoved(value),
                    egui::Event::PointerButton {
                        pos: value,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            frame(
                &ctx,
                &mut player,
                width,
                vec![egui::Event::PointerMoved(value + egui::vec2(30.0, 0.0))],
            );
            frame(
                &ctx,
                &mut player,
                width,
                vec![egui::Event::PointerButton {
                    pos: value + egui::vec2(30.0, 0.0),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            click(&ctx, &mut player, width, toggle);
            assert_eq!(ctx.data_mut(|data| data.get_temp::<bool>(id)), Some(false));
        }
    }

    #[test]
    fn sidebar_sized_overview_keeps_long_ethnicity_in_a_single_stack() {
        let ctx = egui::Context::default();
        let mut player = growing_player(crate::game_version::GameBuild::Fm27Steam);
        let ethnicity = player
            .build
            .schemas()
            .field("p_ethnicity")
            .unwrap()
            .offset
            .unwrap();
        player.raw[ethnicity.offset() as usize] = ethnicity.encode_u8(2);
        let before = player.raw.clone();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(548.0, 5000.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut edits = Vec::new();
                    show_overview_attributes(
                        player.build.schemas(),
                        ui,
                        &mut player,
                        0x20000,
                        &mut edits,
                    );
                    assert!(edits.is_empty());
                });
            },
        );
        assert_eq!(player.raw, before);
        let text = painted_text(&output);
        let rect = |label: &str| text.iter().find(|(s, _, _)| s == label).unwrap().1;
        assert_eq!(rect("Identity").left(), rect("Technical").left());
        assert!(rect("Technical").top() > rect("Appearance").bottom());
        assert!(text.iter().any(|(s, _, _)| s.contains("North African")));
        for (_, rect, _) in text {
            assert!(rect.right() <= 541.0, "narrow column overflow: {rect:?}");
        }
    }

    #[test]
    fn overview_cards_flow_independently_in_balanced_stacks() {
        for width in [
            1440.0, 2160.0, 1220.0, 1200.0, 966.0, 760.0, 700.0, 548.0, 280.0, 400.0,
        ] {
            let ctx = egui::Context::default();
            let mut player = populated_positions(false);
            let before = player.raw.clone();
            let mut edits = vec![];
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 3000.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        show_overview_attributes(
                            player.build.schemas(),
                            ui,
                            &mut player,
                            0x20000,
                            &mut edits,
                        );
                    });
                },
            );
            assert!(edits.is_empty());
            assert_eq!(player.raw, before);
            let text = painted_text(&output);
            let heading = |name: &str| text.iter().find(|(s, _, _)| s == name).unwrap().1;
            let tech = heading("Technical");
            let mental = heading("Mental");
            let physical = heading("Physical");
            let set = heading("Set Pieces");
            let feet = heading("Footedness");
            let personality = heading("Personality");
            if width >= 892.0 {
                let identity = heading("Identity");
                for attribute in [tech, mental, physical] {
                    assert!(
                        (attribute.top() - identity.top()).abs() < 4.0,
                        "attributes must start beside personal details: {attribute:?}, {identity:?}"
                    );
                    assert!(attribute.left() > identity.right());
                }
                assert_eq!(tech.left(), set.left());
                assert_eq!(mental.left(), personality.left());
                assert_eq!(physical.left(), heading("Hidden").left());
                assert!(
                    heading("Hidden").top() + 40.0 < set.top(),
                    "physical stack should reclaim its empty space"
                );
                assert!(heading("Goalkeeping").right() < 1600.0);
                let goalkeeping = heading("Goalkeeping");
                if width >= 1212.0 {
                    assert!(goalkeeping.left() > physical.left());
                    assert!((goalkeeping.top() - physical.top()).abs() < 1.0);
                } else {
                    assert_eq!(goalkeeping.left(), physical.left());
                    assert!(goalkeeping.top() > heading("Hidden").bottom());
                }
            } else if width >= 732.0 {
                assert!(tech.left() > heading("Identity").left());
                assert!(mental.left() > tech.left());
                assert_eq!(physical.left(), tech.left());
                assert_eq!(heading("Goalkeeping").left(), mental.left());
            } else if width >= 612.0 {
                assert_eq!(tech.left(), physical.left());
                assert_eq!(mental.left(), heading("Identity").left());
                assert!(physical.top() > set.top());
            } else {
                assert!(mental.top() > set.top());
                assert!(physical.top() > personality.top());
                assert!(feet.top() > heading("Goalkeeping").top());
                for (_, rect, _) in &text {
                    assert!(rect.right() <= width - 7.0, "horizontal overflow: {rect:?}");
                }
            }
        }
    }

    #[test]
    fn overview_stacks_use_viewport_width_after_an_earlier_row_expands_content() {
        for width in [400.0, 700.0] {
            let ctx = egui::Context::default();
            let mut player = populated_positions(false);
            let mut edits = vec![];
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 3000.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.allocate_space(egui::vec2(1600.0, 10.0));
                        show_overview_attributes(
                            player.build.schemas(),
                            ui,
                            &mut player,
                            0x20000,
                            &mut edits,
                        );
                    });
                },
            );
            let text = painted_text(&output);
            let heading = |label: &str| text.iter().find(|(s, _, _)| s == label).unwrap().1;
            if width == 400.0 {
                assert_eq!(heading("Technical").left(), heading("Mental").left());
                assert!(heading("Mental").top() > heading("Set Pieces").top());
            } else {
                assert!(heading("Technical").left() > heading("Mental").left());
                assert_eq!(heading("Technical").left(), heading("Goalkeeping").left());
            }
            for (_, rect, _) in text {
                assert!(rect.right() <= width - 7.0, "card overflow: {rect:?}");
            }
            assert!(edits.is_empty());
        }
    }

    fn scroll_events(pos: egui::Pos2, delta: f32) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, delta),
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }

    #[test]
    fn narrow_and_short_profiles_scroll_to_all_positions_and_traits_without_overflow() {
        for size in [
            egui::vec2(280.0, 700.0),
            egui::vec2(400.0, 700.0),
            egui::vec2(400.0, 480.0),
            egui::vec2(1000.0, 480.0),
            egui::vec2(1000.0, 570.0), // Below the readable pitch minimum: scrolling is required.
        ] {
            for enabled in [false, true] {
                let ctx = egui::Context::default();
                crate::ui::navigation::set_editing_enabled(&ctx, enabled);
                let mut player = populated_positions(true);
                let mut tab = PlayerTab::PositionsTraits;
                let mut visible_positions = std::collections::HashSet::new();
                let mut traits_reachable = false;
                for frame in 0..85 {
                    let x = if size.x >= 820.0 && frame < 25 {
                        180.0
                    } else {
                        size.x - 40.0
                    };
                    let events = if frame == 0 {
                        vec![]
                    } else {
                        scroll_events(egui::pos2(x, size.y - 50.0), -90.0)
                    };
                    let output = profile_frame(&ctx, &mut player, &mut tab, size, events);
                    for (s, rect, clip) in painted_text(&output) {
                        if clip.contains_rect(rect) {
                            assert!(
                                rect.left() >= 0.0 && rect.right() <= size.x - 6.0,
                                "horizontal overflow at {size:?}: {s}: {rect:?}"
                            );
                            if s == "Traits" {
                                traits_reachable = true;
                            }
                            for field in player
                                .build
                                .schemas()
                                .fields_for_category(Entity::Player, Category::Positions)
                            {
                                if s == field.label {
                                    visible_positions.insert(s.clone());
                                }
                            }
                        }
                    }
                }
                assert_eq!(
                    visible_positions.len(),
                    15,
                    "positions unreachable at {size:?}: {visible_positions:?}"
                );
                assert!(traits_reachable, "Traits unreachable at {size:?}");
            }
        }
    }

    #[test]
    fn traits_and_other_tabs_do_not_scroll_the_wide_pitch() {
        let ctx = egui::Context::default();
        let mut player = populated_positions(false);
        let size = egui::vec2(1000.0, 640.0);
        let mut tab = PlayerTab::PositionsTraits;
        let output = profile_frame(&ctx, &mut player, &mut tab, size, vec![]);
        let initial = painted_text(&output)
            .into_iter()
            .find(|(s, _, _)| s == "GK")
            .unwrap()
            .1;
        let mut traits_reachable = false;
        for _ in 0..20 {
            let output = profile_frame(
                &ctx,
                &mut player,
                &mut tab,
                size,
                scroll_events(egui::pos2(850.0, 570.0), -500.0),
            );
            let text = painted_text(&output);
            assert_eq!(text.iter().find(|(s, _, _)| s == "GK").unwrap().1, initial);
            traits_reachable |= text
                .iter()
                .any(|(s, r, c)| s == "Traits" && c.contains_rect(*r));
        }
        assert!(traits_reachable);
        tab = PlayerTab::Overview;
        for _ in 0..10 {
            profile_frame(
                &ctx,
                &mut player,
                &mut tab,
                size,
                scroll_events(egui::pos2(850.0, 570.0), -500.0),
            );
        }
        tab = PlayerTab::PositionsTraits;
        let output = profile_frame(&ctx, &mut player, &mut tab, size, vec![]);
        let text = painted_text(&output);
        assert_eq!(text.iter().find(|(s, _, _)| s == "GK").unwrap().1, initial);
    }

    #[test]
    fn traits_require_mapping_and_complete_buffer() {
        assert_eq!(trait_value(&[0; 16], None), None);
        assert_eq!(trait_value(&[0; 7], direct(0)), None);
        assert_eq!(trait_value(&[0; 16], direct(9)), None);
        assert_eq!(trait_value(&[0; 16], direct(u64::MAX)), None);
        assert_eq!(
            trait_value(
                &[0; 16],
                Some(fields::FieldLocation::SubRecord {
                    container: "contract",
                    offset: 0,
                    encoding: fields::Encoding::Raw,
                })
            ),
            None
        );
        assert_eq!(
            trait_value(&[1, 0, 0, 0, 0, 0, 0, 0], direct(0)),
            Some((0, 1))
        );
        assert_eq!(trait_value(&[0; 16], direct(8)), Some((8, 0)));
    }

    #[test]
    fn fmt_date_masks_flag_bits() {
        // Day word 7019 = flags | day 363 → 29 Dec 2025.
        let buf = [7019u16.to_le_bytes(), 2025u16.to_le_bytes()].concat();
        assert_eq!(fmt_date_at(&buf, 0).as_deref(), Some("29 Dec 2025"));
    }

    #[test]
    fn fm27_contract_render_has_no_unsupported_apply_or_edits() {
        for build in [
            crate::game_version::GameBuild::Fm27Steam,
            crate::game_version::GameBuild::Fm27GamePass,
        ] {
            for width in [1200.0, 280.0] {
                for length in [None, Some(0), Some(7), Some(196)] {
                    let mut player = fixture_player(build);
                    player.contract_raw = length.map(|n| vec![0xFF; n]);
                    player.contract_addr = length.map(|_| 0x30000);
                    let before = player.contract_raw.clone();
                    let before_player = player.raw.clone();
                    let ctx = egui::Context::default();
                    crate::ui::navigation::set_editing_enabled(&ctx, true);
                    let mut tab = PlayerTab::Contract;
                    for _ in 0..3 {
                        let mut edits = Vec::new();
                        let output = ctx.run(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(width, 900.0),
                                )),
                                ..Default::default()
                            },
                            |ctx| {
                                egui::CentralPanel::default().show(ctx, |ui| {
                                    edits = show(
                                        build.schemas(),
                                        None,
                                        ui,
                                        &mut player,
                                        &mut tab,
                                        0x20000,
                                        WagePeriod::Weekly,
                                        None,
                                        &mut crate::ui::name_picker::NamePicker::default(),
                                    );
                                });
                            },
                        );
                        let text = painted_text(&output);
                        assert!(
                            text.iter()
                                .any(|(s, _, _)| s == "Contract type: Unknown (read-only)")
                        );
                        assert!(
                            !text
                                .iter()
                                .any(|(s, _, _)| s.contains("Apply Contract Type"))
                        );
                        assert!(matches!(tab, PlayerTab::Contract));
                        assert!(edits.is_empty());
                        assert_eq!(player.contract_raw, before);
                        assert_eq!(player.raw, before_player);
                    }
                }
            }
        }
    }
}
