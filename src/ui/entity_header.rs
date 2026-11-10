//! Shared, non-scrolling identity and relationship header for detail panels.
use crate::fields::FieldLocation;
use crate::i18n::UiTr;
use eframe::egui;

pub fn show(
    ui: &mut egui::Ui,
    name: &str,
    kind: &str,
    uid: impl std::fmt::Display,
    age: Option<u8>,
    club: Option<&str>,
    club_addr: Option<u64>,
    nation: Option<&str>,
) {
    // A shallow strip, not a banner: every item participates in normal wrapping.
    ui.horizontal_wrapped(|ui| {
        ui.heading(name);
        ui.label(egui::RichText::new(ui.tr(kind)).strong());
        ui.label(egui::RichText::new(format!("UID: {uid}")).weak());
        if let Some(age) = age {
            ui.label(format!("{} {age}", ui.tr("Age:")));
        }
        if kind != "Club" {
            crate::ui::navigation::club_link(ui, club, club_addr);
        }
        let nation_text = nation
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| ui.tr("Unknown"));
        ui.label(format!("{} {}", ui.tr("Nation:"), nation_text))
            .on_hover_text("Nation pages are not yet supported.");
    });
    ui.add_space(4.0);
}

/// Age on the attached game's date, or the schema reference year if unavailable.
pub fn age(
    schemas: &'static crate::fields::VersionedSchemas,
    today: Option<crate::game_date::GameDate>,
    raw: &[u8],
    key: &str,
) -> Option<u8> {
    let FieldLocation::Direct { offset, .. } = schemas.field(key)?.offset? else {
        return None;
    };
    age_at(raw, offset as usize, today, schemas.age_reference_year)
}

fn age_at(
    raw: &[u8],
    offset: usize,
    today: Option<crate::game_date::GameDate>,
    reference_year: u16,
) -> Option<u8> {
    let bytes = raw.get(offset..offset.checked_add(4)?)?;
    let day = u16::from_le_bytes([bytes[0], bytes[1]]);
    let year = u16::from_le_bytes([bytes[2], bytes[3]]);
    crate::game_date::age_with_reference((day, year), today, reference_year)
}

/// Keep desktop density without asking egui for negative-width columns.
pub fn column_count(width: f32) -> usize {
    if width >= 960.0 {
        4
    } else if width >= 480.0 {
        2
    } else {
        1
    }
}

/// Render four conceptual groups in rows of four, two, or one.
pub fn columns(ui: &mut egui::Ui, mut render: impl FnMut(usize, &mut egui::Ui)) {
    let count = column_count(ui.available_width());
    for start in (0..4).step_by(count) {
        ui.columns(count, |cols| {
            for (index, col) in cols.iter_mut().enumerate() {
                egui::Frame::group(col.style())
                    .inner_margin(10.0)
                    .show(col, |ui| {
                        ui.set_width(ui.available_width());
                        render(start + index, ui);
                    });
            }
        });
        ui.add_space(12.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn collect_text(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(text) => out.push((
                text.galley.text().to_owned(),
                text.galley.rect.translate(text.pos.to_vec2()),
            )),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_text(shape, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn compact_summary_wraps_without_losing_identity_metadata() {
        for kind in ["Player", "Staff", "Human Manager"] {
            for width in [1400.0, 420.0, 320.0] {
                let ctx = egui::Context::default();
                let mut height = 0.0;
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 500.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let top = ui.cursor().top();
                            show(
                                ui,
                                "Layout Manager",
                                kind,
                                123456,
                                Some(46),
                                Some("Tottenham Hotspur"),
                                None,
                                Some("England"),
                            );
                            height = ui.cursor().top() - top;
                        });
                    },
                );
                let mut rendered = Vec::new();
                for shape in &output.shapes {
                    collect_text(&shape.shape, &mut rendered);
                }
                for expected in [
                    "Layout Manager",
                    kind,
                    "UID: 123456",
                    "Age: 46",
                    "Club:",
                    "Tottenham Hotspur",
                    "Nation: England",
                ] {
                    let (_, rect) = rendered
                        .iter()
                        .find(|(text, _)| text == expected)
                        .unwrap_or_else(|| panic!("missing {expected} at {width}"));
                    assert!(
                        rect.left() >= 0.0 && rect.right() <= width,
                        "{expected}: {rect:?}"
                    );
                }
                assert!(
                    height < if width > 1000.0 { 40.0 } else { 120.0 },
                    "header height {height} at {width}"
                );
                let name = rendered
                    .iter()
                    .find(|(text, _)| text == "Layout Manager")
                    .unwrap();
                assert!(name.1.height() >= 20.0, "name retains heading font size");
            }
        }
    }

    #[test]
    fn responsive_columns_preserve_dense_and_narrow_layouts() {
        for (width, expected) in [
            (0.0, 1),
            (20.0, 1),
            (479.0, 1),
            (480.0, 2),
            (959.0, 2),
            (960.0, 4),
            (1400.0, 4),
        ] {
            assert_eq!(column_count(width), expected);
        }
    }
    #[test]
    fn age_requires_complete_valid_dob() {
        assert_eq!(age_at(&[], 0, None, 2026), None);
        assert_eq!(age_at(&[1, 0, 208, 7], usize::MAX, None, 2026), None);
        assert_eq!(age_at(&[1, 0, 208, 7], 0, None, 2026), Some(26));
        assert_eq!(age_at(&[0, 0, 208, 7], 0, None, 2026), None);
        assert_eq!(age_at(&[1, 0, 255, 255], 0, None, 2026), None);
    }
}

#[cfg(test)]
mod game_date_tests {
    use super::*;
    use crate::game_date::GameDate;

    #[test]
    fn header_uses_full_flagged_dob_and_current_date() {
        let mut raw = Vec::new();
        raw.extend_from_slice(&(189u16 | 0x8000).to_le_bytes());
        raw.extend_from_slice(&1991u16.to_le_bytes());
        assert_eq!(age_at(&raw, 0, None, 2027), Some(36));
        for (day, expected) in [(188, 34), (189, 35), (195, 35)] {
            assert_eq!(
                age_at(&raw, 0, Some(GameDate { day, year: 2026 }), 2027),
                Some(expected)
            );
        }
    }
}
