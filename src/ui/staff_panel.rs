//! Staff detail panel with a persistent identity header and scrollable details.

use crate::i18n::UiTr;
use eframe::egui;

use crate::bridge::StaffView;
use crate::fields::{Category, Entity};
use crate::ui::entity_header;
use crate::ui::field_widget::{self, FieldEdit};

#[derive(PartialEq)]
pub enum StaffTab {
    Overview,
    Contract,
}

pub fn show(
    schemas: &'static crate::fields::VersionedSchemas,
    today: Option<crate::game_date::GameDate>,
    ui: &mut egui::Ui,
    staff: &mut StaffView,
    tab: &mut StaffTab,
    names: &mut crate::ui::name_picker::NamePicker,
) -> Vec<FieldEdit> {
    let mut edits = Vec::new();
    header(schemas, today, ui, staff, "Staff");

    let has_contract = staff.contract_raw.is_some();
    if *tab == StaffTab::Contract && !has_contract {
        *tab = StaffTab::Overview;
    }
    ui.horizontal(|ui| {
        ui.selectable_value(tab, StaffTab::Overview, &ui.tr("Overview"));
        if has_contract {
            ui.selectable_value(tab, StaffTab::Contract, &ui.tr("Contract"));
        }
    });
    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| match tab {
        StaffTab::Overview => show_overview(schemas, ui, staff, &mut edits, names),
        StaffTab::Contract => show_contract(schemas, ui, staff, &mut edits),
    });

    redirect_shifted_edits(staff, &mut edits);
    edits
}

/// Move edits to a human manager's identity fields (DOB, personality, ...)
/// to where the game stores them: `identity_shift` bytes past the schema
/// offset. A no-op for AI staff.
fn redirect_shifted_edits(staff: &StaffView, edits: &mut [FieldEdit]) {
    let shift = staff.record.identity_shift as u64;
    if shift == 0 {
        return;
    }
    let addr = staff.record.addr;
    let schema = staff.record.build.schemas();
    for edit in edits {
        let rel = edit.addr.wrapping_sub(addr);
        if schema.is_manager_shifted_offset(rel) {
            edit.addr = edit.addr.wrapping_add(shift);
        }
    }
}

fn header(
    schemas: &'static crate::fields::VersionedSchemas,
    today: Option<crate::game_date::GameDate>,
    ui: &mut egui::Ui,
    staff: &StaffView,
    kind: &str,
) {
    entity_header::show(
        ui,
        staff.name.as_deref().unwrap_or("Unknown"),
        kind,
        staff.record.id(),
        entity_header::age(schemas, today, &staff.record.raw, "s_dob"),
        staff.club_name.as_deref(),
        staff.club_addr,
        staff.nationality.as_deref(),
    );
}

fn identity(
    schemas: &'static crate::fields::VersionedSchemas,
    ui: &mut egui::Ui,
    staff: &mut StaffView,
    edits: &mut Vec<FieldEdit>,
    names: &mut crate::ui::name_picker::NamePicker,
) {
    ui.strong(ui.tr("Identity"));
    if let Some(dob) = schemas.field("s_dob") {
        edits.extend(field_widget::dob_edit(
            ui,
            &mut staff.record.raw,
            &dob,
            staff.record.addr,
        ));
    }
    if let Some(birthplace) = &staff.birthplace {
        field_widget::info_label(ui, "Place of Birth", birthplace);
    }
    profile_labels(schemas, ui, staff);
    let addr = staff.record.addr;
    edits.extend(crate::ui::name_picker::names_section(
        schemas,
        ui,
        names,
        &crate::person::STAFF_NAME_SLOTS,
        &mut staff.record.raw,
        addr,
    ));
    ui.add_space(4.0);
}

/// Read-only person-profile rows: languages, coaching licence and preferred
/// formations, when the attached build maps them.
fn profile_labels(
    schemas: &'static crate::fields::VersionedSchemas,
    ui: &mut egui::Ui,
    staff: &StaffView,
) {
    let Some(layout) = schemas.person_profile else {
        return;
    };
    if let Some(languages) = staff.languages.as_deref().filter(|l| !l.is_empty()) {
        field_widget::info_label_wrapped(
            ui,
            "Languages",
            crate::person::format_languages(languages),
        );
    }
    let byte = |key: &str| {
        schemas
            .offset_of(key)
            .and_then(|o| staff.record.raw.get(o).copied())
    };
    if let Some(code) = byte("s_licence") {
        field_widget::info_label(ui, "Coaching Licence", layout.licence_label(code));
    }
    // A zero ID means no preference (the human manager reads zero).
    let formations: Vec<String> = ["s_formation_in", "s_formation_out"]
        .iter()
        .filter_map(|key| byte(key))
        .filter(|&id| id != 0)
        .map(|id| layout.formation_label(id))
        .collect();
    if formations.len() == 2 {
        field_widget::info_label_wrapped(
            ui,
            "Formations",
            format!("{} (in) / {} (out)", formations[0], formations[1]),
        );
    }
}

fn categories(
    schemas: &'static crate::fields::VersionedSchemas,
    ui: &mut egui::Ui,
    staff: &mut StaffView,
    groups: &[(&str, Category)],
    edits: &mut Vec<FieldEdit>,
) {
    for &(heading, category) in groups {
        field_widget::category_column(
            schemas,
            ui,
            heading,
            Entity::Staff,
            category,
            &mut staff.record.raw,
            staff.record.addr,
            edits,
        );
        ui.add_space(4.0);
    }
}

fn roles(
    schemas: &'static crate::fields::VersionedSchemas,
    ui: &mut egui::Ui,
    staff: &StaffView,
    skip_zeros: bool,
) {
    if let Some(roles) = &staff.roles {
        field_widget::roles_column(schemas, ui, "Roles (read-only)", roles, skip_zeros);
    } else {
        ui.strong("Roles (read-only)");
        ui.label("No role sub-record.");
    }
}

fn show_overview(
    schemas: &'static crate::fields::VersionedSchemas,
    ui: &mut egui::Ui,
    staff: &mut StaffView,
    edits: &mut Vec<FieldEdit>,
    names: &mut crate::ui::name_picker::NamePicker,
) {
    let count = entity_header::column_count(ui.available_width());
    for chunk in [0, 1, 2].chunks(count) {
        ui.columns(chunk.len(), |cols| {
            for (col, group) in cols.iter_mut().zip(chunk) {
                match group {
                    0 => identity(schemas, col, staff, edits, names),
                    1 => categories(
                        schemas,
                        col,
                        staff,
                        &[("Ability", Category::Ability)],
                        edits,
                    ),
                    _ => categories(
                        schemas,
                        col,
                        staff,
                        &[("Reputation", Category::Reputation)],
                        edits,
                    ),
                }
            }
        });
    }
    ui.separator();

    let groups: &[&[(&str, Category)]] = &[
        &[
            ("Coaching", Category::Coaching),
            ("Medical", Category::Medical),
        ],
        &[
            ("Mental", Category::Mental),
            ("Tactical Style", Category::TacticalStyle),
        ],
        &[
            ("Knowledge", Category::Knowledge),
            ("Board Relations", Category::BoardRelations),
        ],
        &[("Personality", Category::Personality)],
    ];
    for chunk in [0, 1, 2, 3].chunks(count) {
        ui.columns(chunk.len(), |cols| {
            for (col, &group) in cols.iter_mut().zip(chunk) {
                categories(schemas, col, staff, groups[group], edits);
                if group == 3 {
                    roles(schemas, col, staff, true);
                }
            }
        });
    }
}

/// Single-page layout for the human manager.
///
/// The buffer is patched by `load_human_manager` to expose identity fields at
/// standard staff offsets. Redirect edits for those fields to their live,
/// shifted addresses after collecting the widgets' edits.
pub fn show_manager(
    schemas: &'static crate::fields::VersionedSchemas,
    today: Option<crate::game_date::GameDate>,
    ui: &mut egui::Ui,
    staff: &mut StaffView,
    names: &mut crate::ui::name_picker::NamePicker,
) -> Vec<FieldEdit> {
    let mut edits = Vec::new();
    header(schemas, today, ui, staff, "Human Manager");
    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
        let count = entity_header::column_count(ui.available_width());
        // Keep short field grids close together even on ultrawide viewports.
        let spacing = ui.spacing().item_spacing.x;
        ui.set_max_width(
            (count as f32 * 340.0 + (count - 1) as f32 * spacing).min(ui.available_width()),
        );
        let groups: &[&[(&str, Category)]] = &[
            &[
                ("Ability", Category::Ability),
                ("Reputation", Category::Reputation),
            ],
            &[
                ("Coaching", Category::Coaching),
                ("Personality", Category::Personality),
                ("Medical", Category::Medical),
            ],
            &[
                ("Mental", Category::Mental),
                ("Knowledge", Category::Knowledge),
            ],
            &[
                ("Tactical Style", Category::TacticalStyle),
                ("Board Relations", Category::BoardRelations),
            ],
        ];
        for chunk in [0, 1, 2, 3].chunks(count) {
            ui.columns(chunk.len(), |cols| {
                for (col, &group) in cols.iter_mut().zip(chunk) {
                    if group == 0 {
                        identity(schemas, col, staff, &mut edits, names);
                    }
                    categories(schemas, col, staff, groups[group], &mut edits);
                    if group == 0 {
                        if staff.contract_raw.is_some() {
                            show_contract(schemas, col, staff, &mut edits);
                            col.add_space(4.0);
                        }
                    }
                    if group == 2 {
                        // Keep all roles beneath Knowledge, including zero ratings.
                        roles(schemas, col, staff, false);
                    }
                }
            });
        }
    });

    // Redirect only identity fields stored at the human manager's shifted offsets.
    redirect_shifted_edits(staff, &mut edits);
    edits
}

fn show_contract(
    schemas: &'static crate::fields::VersionedSchemas,
    ui: &mut egui::Ui,
    staff: &mut StaffView,
    edits: &mut Vec<FieldEdit>,
) {
    ui.strong(ui.tr("Contract"));
    contract_rows(schemas, ui, staff, edits);
}

/// Job and club (read-only), wage and expiry (editable) from the staff
/// contract sub-record. Edits address the contract itself, so the human
/// manager's shifted-identity redirect in `show_manager` leaves them alone.
fn contract_rows(
    schemas: &'static crate::fields::VersionedSchemas,
    ui: &mut egui::Ui,
    staff: &mut StaffView,
    edits: &mut Vec<FieldEdit>,
) {
    let (Some(cr), Some(caddr)) = (staff.contract_raw.as_mut(), staff.contract_addr) else {
        return;
    };
    if let Some(job) = schemas.offset_of("s_job").and_then(|o| cr.get(o).copied()) {
        let text = match crate::staff::job_name(job) {
            Some(name) => format!("{name} ({job})"),
            None => format!("code {job}"),
        };
        field_widget::info_label(ui, "Job", text);
    }
    if let Some(club) = &staff.club_name {
        field_widget::info_label(ui, "Club", club);
    }
    if let Some(f) = schemas.field("s_weekly_wage") {
        edits.extend(field_widget::raw_u32_edit(ui, f, cr, caddr));
    }
    if let Some(f) = schemas.field("s_contract_expiry") {
        edits.extend(field_widget::date_edit(
            ui,
            "Expiry",
            cr,
            f,
            caddr,
            2000..=2060,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{game_version::GameBuild, staff::StaffRecord};

    fn manager(build: GameBuild) -> StaffView {
        let schemas = build.schemas();
        let mut raw = vec![0; schemas.staff_struct_size];
        let dob = schemas.offset_of("s_dob").unwrap();
        raw[dob..dob + 2].copy_from_slice(&1u16.to_le_bytes());
        raw[dob + 2..dob + 4].copy_from_slice(&1980u16.to_le_bytes());
        StaffView {
            record: StaffRecord {
                build,
                addr: 0x20000,
                index: 0,
                raw,
                identity_shift: schemas.human_manager_shift,
            },
            name: Some("Layout Manager".into()),
            roles: Some([0; 15]),
            role_ptr: 0,
            nationality: Some("England".into()),
            club_name: Some("Tottenham Hotspur".into()),
            club_addr: None,
            birthplace: Some("London".into()),
            languages: None,
            contract_raw: Some(vec![0; 1024]),
            contract_addr: Some(0x40000),
        }
    }

    fn texts(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(t) => out.push((
                t.galley.text().to_owned(),
                t.galley.rect.translate(t.pos.to_vec2()),
            )),
            egui::Shape::Vec(shapes) => {
                for s in shapes {
                    texts(s, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn manager_identity_is_in_bounded_body_columns_without_render_edits() {
        for build in [GameBuild::Fm24Steam, GameBuild::Fm27Steam] {
            for width in [2400.0, 1400.0, 700.0, 420.0, 320.0] {
                let mut staff = manager(build);
                let before = staff.record.raw.clone();
                let contract_before = staff.contract_raw.clone();
                let ctx = egui::Context::default();
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 6000.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let edits = show_manager(
                                build.schemas(),
                                None,
                                ui,
                                &mut staff,
                                &mut crate::ui::name_picker::NamePicker::default(),
                            );
                            assert!(edits.is_empty());
                        });
                    },
                );
                assert_eq!(staff.record.raw, before);
                assert_eq!(staff.contract_raw, contract_before);
                let mut rendered = Vec::new();
                for shape in &output.shapes {
                    texts(&shape.shape, &mut rendered);
                }
                let rect = |text: &str| rendered.iter().find(|(s, _)| s == text).unwrap().1;
                assert_eq!(rendered.iter().filter(|(s, _)| s == "Identity").count(), 1);
                let name = rect("Layout Manager");
                let identity = rect("Identity");
                let ability = rect("Ability");
                assert!(identity.top() > name.bottom());
                assert!(identity.top() < ability.top());
                assert!((identity.left() - ability.left()).abs() < 1.0);
                for label in [
                    "Reputation",
                    "Contract",
                    "Coaching",
                    "Medical",
                    "Mental",
                    "Knowledge",
                    "Tactical Style",
                    "Board Relations",
                    "Personality",
                    "Roles (read-only)",
                    "Place of Birth",
                ] {
                    assert!(
                        rect(label).top() > name.bottom(),
                        "{label} missing from body"
                    );
                }
                // Even zero-rated roles are present, not hidden or dropped.
                for field in build
                    .schemas()
                    .fields_for_category(Entity::Staff, Category::StaffRole)
                {
                    if field.offset.as_ref().is_some_and(|loc| loc.offset() < 15) {
                        assert!(
                            rendered.iter().any(|(text, _)| text == field.label),
                            "missing role {}",
                            field.label
                        );
                    }
                }
                if width > 1000.0 {
                    let coaching = rect("Coaching");
                    assert!((coaching.top() - identity.top()).abs() < 1.0);
                    assert!(coaching.left() > identity.left());
                    assert!(coaching.left() - identity.left() <= 350.0);
                    assert!(rect("Roles (read-only)").left() > coaching.left());
                    assert_eq!(rect("Roles (read-only)").left(), rect("Knowledge").left());
                    assert!(rect("Roles (read-only)").top() > rect("Knowledge").bottom());
                } else if width < 480.0 {
                    assert!(rect("Coaching").top() > rect("Contract").bottom());
                    assert!((identity.left() - name.left()).abs() < 1.0);
                }
            }
        }
    }
}
