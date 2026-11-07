//! Club detail panel: Overview, Facilities, Finance, Stadium tabs.

use crate::i18n::UiTr;
use eframe::egui;

use crate::bridge::ClubView;
use crate::fields::{self, FieldLocation};
use crate::ui::field_widget::{self, FieldEdit};
use crate::ui::{entity_header, navigation};

#[derive(PartialEq)]
pub enum ClubTab {
    Overview,
    Facilities,
    Finance,
    Stadium,
    ContractPolicy,
}

pub(crate) fn show(
    schemas: &'static crate::fields::VersionedSchemas,
    ui: &mut egui::Ui,
    club: &mut ClubView,
    tab: &mut ClubTab,
    policy_ui: &mut crate::ui::club_policy::PolicyUi,
    show_policy: bool,
    writable: bool,
) -> (Vec<FieldEdit>, Option<crate::ui::club_policy::Action>) {
    let mut edits = Vec::new();
    let mut policy_action = None;

    entity_header::show(
        ui,
        &club.full_name,
        "Club",
        club.uid,
        None,
        None,
        None,
        club.nation.as_deref(),
    );
    ui.horizontal(|ui| {
        ui.label(ui.tr("Ground:"));
        if let Some(name) = club
            .stadium_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
        {
            if ui
                .link(name)
                .on_hover_text("Open stadium details")
                .clicked()
            {
                *tab = ClubTab::Stadium;
            }
        } else {
            ui.label("Unknown");
        }
    });

    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(tab, ClubTab::Overview, &ui.tr("Overview"));
        ui.selectable_value(tab, ClubTab::Facilities, &ui.tr("Facilities"));
        ui.selectable_value(tab, ClubTab::Finance, &ui.tr("Finance"));
        ui.selectable_value(tab, ClubTab::Stadium, &ui.tr("Stadium (read-only)"));
        if show_policy {
            ui.selectable_value(tab, ClubTab::ContractPolicy, "Contract policy (trial)");
        }
    });
    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| match tab {
        ClubTab::Overview => show_overview(schemas, ui, club, &mut edits),
        ClubTab::Facilities => show_facilities(schemas, ui, club, &mut edits),
        ClubTab::Finance => show_finance(schemas, ui, club, &mut edits),
        ClubTab::Stadium => show_stadium(schemas, ui, club),
        ClubTab::ContractPolicy => {
            if show_policy {
                policy_action = policy_ui.show(
                    ui,
                    crate::club_policy::ClubIdentity {
                        address: club.addr,
                        index: club.index,
                        uid: club.uid,
                    },
                    &club.full_name,
                    writable,
                );
            } else {
                ui.label("Contract policy trial unsupported for this build.");
            }
        }
    });

    (edits, policy_action)
}

fn show_overview(
    schemas: &'static crate::fields::VersionedSchemas,
    ui: &mut egui::Ui,
    club: &mut ClubView,
    edits: &mut Vec<FieldEdit>,
) {
    ui.strong(ui.tr("Identity"));
    field_widget::info_label(ui, "Full Name", &club.full_name);
    field_widget::info_label(ui, "Short Name", &club.short_name);
    field_widget::info_label(ui, "UID", club.uid);
    if let Some(ref nick) = club.nickname {
        field_widget::info_label(ui, "Nickname", nick);
    }

    // Gender byte — direct club offset, read via c_gender field.
    let gender = schemas
        .field("c_gender")
        .and_then(|f| f.offset)
        .and_then(|l| club.raw.get(l.offset() as usize).copied());
    field_widget::info_label(
        ui,
        "Gender",
        match gender {
            Some(0) => "Male",
            Some(_) => "Female",
            None => "Unknown",
        },
    );
    // Reputation lives in a sub-record (FM27: the senior team object); edit
    // that record's bytes at its own address.
    if let Some(f) = schemas.field("c_reputation") {
        if let Some((container, _)) = sub_location(schemas, "c_reputation") {
            if let Some((buf, base)) = club.sub_record_mut(container) {
                edits.extend(field_widget::raw_u16_edit(ui, f, buf, 10000, base));
            }
        }
    }

    ui.add_space(8.0);
    ui.strong("Club Details");
    // Year founded is a u16; on FM27 the following u16 is not always zero.
    if let Some(v) = schemas
        .field("c_year_founded")
        .and_then(|f| fields::read_field_u16(club, &club.raw, &f))
    {
        field_widget::info_label(ui, "Year Founded", v);
    }
    if let Some(v) = schemas
        .field("c_owner_status")
        .and_then(|f| fields::read_field_u16(club, &club.raw, &f))
    {
        field_widget::info_label(ui, "Owner Status", v);
    }
    if let Some(v) = schemas
        .field("c_season_tickets")
        .and_then(|f| fields::read_field_u32(club, &club.raw, &f))
    {
        field_widget::info_label(ui, "Season Tickets", v);
    }
    if let Some(v) = schemas
        .field("c_social_media")
        .and_then(|f| fields::read_field_u32(club, &club.raw, &f))
    {
        field_widget::info_label(ui, "Social Media", v);
    }
}

fn show_facilities(
    schemas: &'static crate::fields::VersionedSchemas,
    ui: &mut egui::Ui,
    club: &mut ClubView,
    edits: &mut Vec<FieldEdit>,
) {
    ui.strong(ui.tr("Facilities"));
    // Facility rows edit whichever sub-record the schema puts them in. Rows
    // with a `<key>_fin` mirror (FM27 academy coaching / youth recruitment,
    // which FM's Facilities screen reads from the finance object) write the
    // same value to the mirror too, so the change shows in game.
    for (label, key) in [
        ("Training", "c_training"),
        ("Youth Facilities", "c_youth_fac"),
        ("Youth Improvement", "c_youth_improve"),
        ("Youth Coaching", "c_youth_coach"),
        ("Youth Recruitment", "c_youth_recruit"),
        ("Corporate", "c_corporate"),
    ] {
        let Some((container, offset)) = sub_location(schemas, key) else {
            continue;
        };
        let before = edits.len();
        if let Some((buf, base)) = club.sub_record_mut(container) {
            u8_drag_1_20(ui, &ui.tr(label), buf, offset, base, edits);
        }
        if edits.len() > before {
            let new_val = edits[edits.len() - 1].new_raw[0];
            if let Some(mut mirror) =
                mirror_u8_edit(schemas, club, &format!("{key}_fin"), &ui.tr(label), new_val)
            {
                group_facility_edits(club.addr, key, &mut edits[before], &mut mirror);
                edits.push(mirror);
            }
        }
    }

    if club.info_raw.is_some() {
        ui.add_space(8.0);
        ui.strong(ui.tr("Supporter Profile"));
        for (label, key) in [
            ("Hardcore %", "c_sup_hardcore"),
            ("Core %", "c_sup_core"),
            ("Family %", "c_sup_family"),
            ("Fair Weather %", "c_sup_fair_weather"),
            ("Casual %", "c_sup_casual"),
            ("Other %", "c_sup_other"),
        ] {
            if let Some(f) = schemas.field(key) {
                if let Some(v) = fields::read_field_u8(club, &club.raw, &f) {
                    field_widget::info_label(ui, label, format!("{v}%"));
                }
            }
        }

        ui.add_space(8.0);
        ui.strong(ui.tr("Supporter Attributes"));
        for (label, key) in [
            ("Loyalty", "c_sup_loyalty"),
            ("Passion", "c_sup_passion"),
            ("Patience", "c_sup_patience"),
            ("Affluence", "c_sup_affluence"),
            ("Temperament", "c_sup_temperament"),
            ("Expectations", "c_sup_expectations"),
        ] {
            if let Some(f) = schemas.field(key) {
                if let Some(v) = fields::read_field_u8(club, &club.raw, &f) {
                    field_widget::info_label(ui, label, v);
                }
            }
        }
    } else {
        ui.label("Info sub-record not available.");
    }
}

fn show_finance(
    schemas: &'static crate::fields::VersionedSchemas,
    ui: &mut egui::Ui,
    club: &mut ClubView,
    edits: &mut Vec<FieldEdit>,
) {
    ui.strong(ui.tr("Finance"));
    // Each row edits the sub-record its schema location names (FM26 keeps the
    // budgets in a finance → budget sub-record, FM27 in the finance object).
    let mut shown = false;
    let cur = crate::i18n::currency(ui.ctx());
    let period = crate::i18n::wage_period(ui.ctx());
    // Labels come from the schema. FM27 has no editable wage budget: its
    // budget and current wage total are shown read-only below.
    for key in [
        "c_balance",
        "c_transfer_budget",
        "c_transfer_remaining",
        "c_wage_budget",
    ] {
        let Some((container, offset)) = sub_location(schemas, key) else {
            continue;
        };
        let Some(field) = schemas.field(key) else {
            continue;
        };
        let wage_row = key == "c_wage_budget";
        let label = ui.tr(field.label);
        let factor = cur.factor() * if wage_row { period.factor() } else { 1.0 };
        if let Some((buf, base)) = club.sub_record_mut(container) {
            u32_drag(ui, &label, buf, offset, base, edits, factor);
            shown = true;
        }
    }
    for (label, key) in [
        ("Avg Ticket Price", "c_avg_ticket"),
        ("Avg Season Ticket", "c_avg_season_ticket"),
    ] {
        let Some((container, offset)) = sub_location(schemas, key) else {
            continue;
        };
        if let Some((buf, base)) = club.sub_record_mut(container) {
            f32_drag(ui, &ui.tr(label), buf, offset, base, edits, cur.factor());
            shown = true;
        }
    }
    // Derived totals: shown, not edited (FM recomputes them).
    for key in [
        "c_wage_total",
        "c_wage_spend",
        "c_youth_grant",
        "c_next_transfer_min",
    ] {
        let Some(field) = schemas.field(key) else {
            continue;
        };
        if let Some(v) = fields::read_field_u32(club, &club.raw, field) {
            let wage_row = matches!(key, "c_wage_total" | "c_wage_spend");
            let label = ui.tr(field.label);
            let factor = cur.factor() * if wage_row { period.factor() } else { 1.0 };
            let shown_v = (v as f64 * factor).round() as u64;
            field_widget::info_label(
                ui,
                &label,
                format!("{} {}", crate::i18n::thousands(shown_v), cur.symbol()),
            );
            shown = true;
        }
    }
    if !shown {
        ui.label(ui.tr("Finance data not available."));
    }
}

fn show_stadium(
    schemas: &'static crate::fields::VersionedSchemas,
    ui: &mut egui::Ui,
    club: &ClubView,
) {
    ui.strong(ui.tr("Attendance"));
    if club.stadium_raw.is_some() {
        for key in ["c_avg_attendance", "c_min_attendance", "c_max_attendance"] {
            if let Some(f) = schemas.field(key) {
                if let Some(v) = fields::read_field_u32(club, &club.raw, &f) {
                    field_widget::info_label(ui, f.label, v);
                }
            }
        }
    } else {
        ui.label(ui.tr("Attendance sub-record not available."));
    }

    if let Some(se) = &club.stadium_entity_raw {
        ui.separator();
        ui.strong(ui.tr("Stadium"));
        if let Some(name) = &club.stadium_name {
            field_widget::info_label(ui, "Name", name);
        }
        if let Some(name2) = &club.stadium_name2 {
            field_widget::info_label(ui, "Official Name", name2);
        }
        for key in [
            "st_capacity",
            "st_seated",
            "st_used",
            "st_all_seater",
            "st_expansion",
        ] {
            if let Some(f) = schemas.field(key) {
                if let Some(v) = fields::read_field_u32(&(), se, &f) {
                    field_widget::info_label(ui, f.label, v);
                }
            }
        }
        for key in ["st_pitch_length", "st_pitch_width"] {
            if let Some(f) = schemas.field(key) {
                if let Some(v) = fields::read_field_u16(&(), se, &f) {
                    field_widget::info_label(ui, f.label, format!("{:.1}m", v as f32 / 10.0));
                }
            }
        }
        for key in ["st_environment", "st_condition"] {
            if let Some(f) = schemas.field(key) {
                if let Some(v) = fields::read_field_u8(&(), se, &f) {
                    field_widget::info_label(ui, f.label, v);
                }
            }
        }
    }
}

/// Container and byte offset of a sub-record field, from its schema key.
fn sub_location(
    schemas: &'static crate::fields::VersionedSchemas,
    key: &str,
) -> Option<(fields::SubRecordId, usize)> {
    match schemas.field(key)?.offset? {
        FieldLocation::SubRecord {
            container, offset, ..
        } => Some((container, offset as usize)),
        FieldLocation::Direct { .. } => None,
    }
}

/// Edit writing `new_val` to the mirror field `key` (a second copy of a
/// facility rating that FM keeps in sync), or `None` when the schema has no
/// such mirror or its value already matches.
fn mirror_u8_edit(
    schemas: &'static crate::fields::VersionedSchemas,
    club: &mut ClubView,
    key: &str,
    label: &str,
    new_val: u8,
) -> Option<FieldEdit> {
    let (container, offset) = sub_location(schemas, key)?;
    let (buf, base) = club.sub_record_mut(container)?;
    let old = *buf.get(offset)?;
    if old == new_val {
        return None;
    }
    buf[offset] = new_val;
    Some(FieldEdit {
        addr: base + offset as u64,
        old_raw: vec![old],
        new_raw: vec![new_val],
        description: format!("{label} (finance copy): {old} \u{2192} {new_val}"),
        presentation: Some(crate::edit_history::EditPresentation::new(
            format!("{label} (finance copy)"),
            old,
            new_val,
        )),
    })
}

fn group_facility_edits(
    club_addr: u64,
    key: &str,
    primary: &mut FieldEdit,
    mirror: &mut FieldEdit,
) {
    if let (Some(p), Some(m)) = (&mut primary.presentation, &mut mirror.presentation) {
        let group = crate::edit_history::EditGroup::new(
            format!("facility:{club_addr:x}:{key}"),
            &p.field,
            &p.before,
            &p.after,
        );
        p.group = Some(group.clone());
        m.group = Some(group);
    }
}

// ---------------------------------------------------------------------------
// Sub-record edit helpers — offset is relative to the sub-record base, not
// the top-level club struct.
// ---------------------------------------------------------------------------

// Generic u32 words with the high bit set have no verified signed money
// interpretation. Keep their exact bits visible instead of asserting GBP units.
fn u32_value(label: &str, value: u32) -> String {
    if value & 0x8000_0000 != 0 {
        format!("raw u32 {value} (0x{value:08X}; signedness unverified)")
    } else {
        field_widget::money_value(label, value)
    }
}

/// Editable u32 field (money, counts, etc.). Double-click to type exact value.
fn u32_drag(
    ui: &mut egui::Ui,
    label: &str,
    raw: &mut [u8],
    offset: usize,
    base_addr: u64,
    edits: &mut Vec<FieldEdit>,
    factor: f64,
) {
    if offset + 4 > raw.len() {
        return;
    }
    let old_bytes: [u8; 4] = raw[offset..offset + 4].try_into().unwrap();
    let old_val = u32::from_le_bytes(old_bytes);
    let mut val = old_val as i64;
    let enabled = navigation::editing_enabled(ui);
    let mut changed = false;
    let cur = crate::i18n::currency(ui.ctx());
    // High-bit values have unverified signedness: render them raw and keep
    // the guarded history text instead of claiming a money conversion.
    let verified = old_val & 0x8000_0000 == 0;
    let fmt = |v: u32| {
        if v & 0x8000_0000 != 0 {
            u32_value(label, v)
        } else {
            format!(
                "{} {}",
                crate::i18n::thousands(((v as f64 * factor).round()) as u64),
                cur.symbol()
            )
        }
    };
    ui.horizontal(|ui| {
        ui.label(label);
        changed = ui
            .add_enabled_ui(enabled, |ui| {
                let drag = egui::DragValue::new(&mut val)
                    .update_while_editing(false)
                    .range(0i64..=4_294_967_295i64)
                    .speed(1.0);
                let drag = if verified {
                    drag.custom_formatter(crate::i18n::money_drag_formatter(
                        factor,
                        &format!(" {}", cur.symbol()),
                    ))
                    .custom_parser(crate::i18n::money_drag_parser(factor))
                } else {
                    drag
                };
                ui.add_sized([100.0, 22.0], drag).changed()
            })
            .inner;
    });
    if enabled && changed {
        let new_val = val.clamp(0, 4_294_967_295) as u32;
        if new_val != old_val {
            let new_bytes = new_val.to_le_bytes();
            raw[offset..offset + 4].copy_from_slice(&new_bytes);
            edits.push(FieldEdit {
                addr: base_addr + offset as u64,
                old_raw: old_bytes.to_vec(),
                new_raw: new_bytes.to_vec(),
                description: format!("{label}: {} \u{2192} {}", fmt(old_val), fmt(new_val)),
                presentation: Some(crate::edit_history::EditPresentation::new(
                    label,
                    fmt(old_val),
                    fmt(new_val),
                )),
            });
        }
    }
}

/// Editable f32 field (ticket prices, etc.).
fn f32_drag(
    ui: &mut egui::Ui,
    label: &str,
    raw: &mut [u8],
    offset: usize,
    base_addr: u64,
    edits: &mut Vec<FieldEdit>,
    factor: f64,
) {
    if offset + 4 > raw.len() {
        return;
    }
    let old_bytes: [u8; 4] = raw[offset..offset + 4].try_into().unwrap();
    let old_val = f32::from_le_bytes(old_bytes);
    if !old_val.is_finite() || !(0.0..=100_000.0).contains(&old_val) {
        let grey = egui::Color32::from_rgb(110, 110, 110);
        ui.horizontal(|ui| {
            ui.label(label);
            ui.label(egui::RichText::new(format!("raw={old_val}")).color(grey))
                .on_hover_text(
                    "Stored value is outside the supported 0..100000 range; shown read-only.",
                );
        });
        return;
    }
    let mut val = old_val;
    let enabled = navigation::editing_enabled(ui);
    let mut changed = false;
    let cur = crate::i18n::currency(ui.ctx());
    let fmt = |v: f32| {
        format!(
            "{} {}",
            crate::i18n::thousands(((v as f64 * factor).round() as i64).max(0) as u64),
            cur.symbol()
        )
    };
    ui.horizontal(|ui| {
        ui.label(label);
        changed = ui
            .add_enabled_ui(enabled, |ui| {
                ui.add_sized(
                    [100.0, 22.0],
                    egui::DragValue::new(&mut val)
                        .update_while_editing(false)
                        .range(0.0f32..=100_000.0f32)
                        .clamp_existing_to_range(false)
                        .speed(0.5)
                        .custom_formatter(crate::i18n::money_drag_formatter(
                            factor,
                            &format!(" {}", cur.symbol()),
                        ))
                        .custom_parser(crate::i18n::money_drag_parser(factor)),
                )
                .changed()
            })
            .inner;
    });
    if enabled && changed && (val - old_val).abs() > 0.001 {
        let new_bytes = val.to_le_bytes();
        raw[offset..offset + 4].copy_from_slice(&new_bytes);
        edits.push(FieldEdit {
            addr: base_addr + offset as u64,
            old_raw: old_bytes.to_vec(),
            new_raw: new_bytes.to_vec(),
            description: format!("{label}: {} \u{2192} {}", fmt(old_val), fmt(val)),
            presentation: Some(crate::edit_history::EditPresentation::new(
                label,
                fmt(old_val),
                fmt(val),
            )),
        });
    }
}

/// Editable 1–20 facility rating with FMLE-style color coding.
fn u8_drag_1_20(
    ui: &mut egui::Ui,
    label: &str,
    raw: &mut [u8],
    offset: usize,
    base_addr: u64,
    edits: &mut Vec<FieldEdit>,
) {
    if offset >= raw.len() {
        return;
    }
    let old = raw[offset];
    if !(1..=20).contains(&old) {
        let grey = egui::Color32::from_rgb(110, 110, 110);
        ui.horizontal(|ui| {
            ui.add_sized(
                [150.0, 22.0],
                egui::Label::new(egui::RichText::new(label).color(grey)),
            );
            ui.label(egui::RichText::new(format!("raw={old}")).color(grey));
        });
        return;
    }
    let color = field_widget::value_color(old);
    let mut val = old as i32;
    let enabled = navigation::editing_enabled(ui);
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add_sized([150.0, 22.0], egui::Label::new(label));
        ui.visuals_mut().override_text_color = Some(color);
        changed = ui
            .add_enabled_ui(enabled, |ui| {
                ui.add_sized(
                    [44.0, 22.0],
                    egui::DragValue::new(&mut val)
                        .update_while_editing(false)
                        .range(1..=20)
                        .speed(0.1),
                )
                .changed()
            })
            .inner;
    });
    if enabled && changed {
        let new_val = val.clamp(1, 20) as u8;
        if new_val != old {
            raw[offset] = new_val;
            edits.push(FieldEdit {
                addr: base_addr + offset as u64,
                old_raw: vec![old],
                new_raw: vec![new_val],
                description: format!("{label}: {old} \u{2192} {new_val}"),
                presentation: Some(crate::edit_history::EditPresentation::new(
                    label, old, new_val,
                )),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn u32_drag_high_bit_metadata_preserves_raw_bits_without_money_claim() {
        fn text_position(shapes: &[egui::epaint::ClippedShape], text: &str) -> egui::Pos2 {
            fn locate(shape: &egui::Shape, text: &str) -> Option<egui::Pos2> {
                match shape {
                    egui::Shape::Text(s) if s.galley.text() == text => {
                        Some(s.pos + s.galley.size() / 2.0)
                    }
                    egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| locate(s, text)),
                    _ => None,
                }
            }
            shapes.iter().find_map(|s| locate(&s.shape, text)).unwrap()
        }
        let ctx = egui::Context::default();
        navigation::set_editing_enabled(&ctx, true);
        let old = 0xFFFF_FF06u32;
        let new = 250u32;
        let mut raw = old.to_le_bytes();
        let mut edits = Vec::new();
        let mut frame = |events| {
            ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        u32_drag(ui, "Balance", &mut raw, 0, 0x20000, &mut edits, 1.0);
                    });
                },
            )
        };
        let output = frame(vec![]);
        // High-bit (unverified signedness) values stay raw, unformatted.
        let pos = text_position(&output.shapes, &old.to_string());
        frame(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        frame(vec![egui::Event::Text(new.to_string())]);
        frame(vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert_eq!(raw, new.to_le_bytes());
        assert_eq!(edits.len(), 1);
        let edit = &edits[0];
        assert_eq!(edit.old_raw, old.to_le_bytes());
        assert_eq!(edit.new_raw, new.to_le_bytes());
        let p = edit.presentation.as_ref().unwrap();
        assert_eq!(
            p.before,
            "raw u32 4294967046 (0xFFFFFF06; signedness unverified)"
        );
        assert!(!p.before.contains("GBP"));
        // factor 1.0 here: 250 internal → "250 £" with separators/symbol only.
        assert_eq!(p.after, "250 £");
        assert_eq!(u32_value("Wage Budget (p/w)", 500), "500 GBP/week");
        assert!(u32_value("Balance", 0x8000_0000).contains("signedness unverified"));
    }

    #[test]
    fn facility_mirror_group_preserves_each_physical_preimage() {
        let make = |addr, old, field| FieldEdit {
            addr,
            old_raw: vec![old],
            new_raw: vec![18],
            description: "unchanged".into(),
            presentation: Some(crate::edit_history::EditPresentation::new(field, old, 18)),
        };
        let mut primary = make(0x20000, 12, "Youth Coaching");
        let mut mirror = make(0x30000, 9, "Youth Coaching (finance copy)");
        group_facility_edits(0x10000, "c_youth_coach", &mut primary, &mut mirror);
        let p = primary.presentation.as_ref().unwrap();
        let m = mirror.presentation.as_ref().unwrap();
        assert_eq!(p.group, m.group);
        assert_eq!(p.group.as_ref().unwrap().before, "12");
        assert_eq!(p.group.as_ref().unwrap().after, "18");
        assert_eq!(m.before, "9");
        assert_eq!(primary.old_raw, [12]);
        assert_eq!(mirror.old_raw, [9]);
        assert_eq!(primary.description, "unchanged");
        let mut other = make(0x40000, 12, "Youth Coaching");
        let mut other_mirror = make(0x50000, 9, "Youth Coaching (finance copy)");
        group_facility_edits(0x60000, "c_youth_coach", &mut other, &mut other_mirror);
        assert_ne!(p.group, other.presentation.unwrap().group);
    }

    fn run_f32(value: f32, enabled: bool) -> (Vec<FieldEdit>, [u8; 4]) {
        let ctx = egui::Context::default();
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("editing_enabled"), enabled));
        let mut raw = value.to_le_bytes();
        let mut edits = Vec::new();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                f32_drag(ui, "Price", &mut raw, 0, 0x1000, &mut edits, 1.0);
            });
        });
        (edits, raw)
    }

    #[test]
    fn invalid_or_out_of_range_f32_is_not_mutated() {
        for v in [f32::NAN, f32::INFINITY, -5.0, 200_000.0] {
            for enabled in [true, false] {
                let (edits, raw) = run_f32(v, enabled);
                assert!(edits.is_empty());
                assert_eq!(raw, v.to_le_bytes());
            }
        }
    }

    #[test]
    fn disabled_in_range_f32_is_not_mutated() {
        let (edits, raw) = run_f32(50.0, false);
        assert!(edits.is_empty());
        assert_eq!(raw, 50.0f32.to_le_bytes());
    }
}
