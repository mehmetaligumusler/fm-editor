//! Advanced player search modal.
//!
//! Shares `PlayerSearchState` directly. The caller snapshots before opening and
//! restores the complete state on `ModalResult::Cancel`.

use crate::i18n::UiTr;
use eframe::egui;

use super::player_search::{POSITION_OFFSETS, PlayerSearchState};

#[derive(Default, PartialEq)]
pub enum ModalResult {
    #[default]
    None,
    Ok,
    Cancel,
    /// Reset every field displayed in the modal, keeping query and sorting.
    ClearAll,
}

/// Render a resizable window with scrolling inputs and a non-scrolling footer.
/// The caller is responsible for snapshotting and restoring state on Cancel.
pub fn show(ctx: &egui::Context, state: &mut PlayerSearchState, open: &mut bool) -> ModalResult {
    let mut result = ModalResult::None;
    let available = ctx.screen_rect().size();
    // Leave room for the title, window margins, validation and footer even on
    // short viewports. Content, rather than the whole window, scrolls.
    let content_height = (available.y - 150.0).clamp(40.0, 480.0);
    egui::Window::new(crate::i18n::tr(ctx, "Advanced Search"))
        .open(open)
        .resizable(true)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .min_width(200.0)
        .default_width((available.x - 32.0).clamp(200.0, 440.0))
        .max_width((available.x - 24.0).max(200.0))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("advanced_search_content")
                .max_height(content_height)
                .show(ui, |ui| {
                    show_general_tab(ui, state);
                    if let Some(error) = state.validation_error() {
                        ui.colored_label(ui.visuals().error_fg_color, error);
                    }
                });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if ui.button("Clear All").clicked() {
                    result = ModalResult::ClearAll;
                }
                if ui.button("Cancel").clicked() {
                    result = ModalResult::Cancel;
                }
                if ui
                    .add_enabled(
                        state.validation_error().is_none(),
                        egui::Button::new(ui.tr("OK")),
                    )
                    .clicked()
                {
                    result = ModalResult::Ok;
                }
            });
        });
    if result == ModalResult::Ok || result == ModalResult::Cancel {
        *open = false;
    }
    result
}

fn show_general_tab(ui: &mut egui::Ui, state: &mut PlayerSearchState) {
    // Stack sections so resizing never strands controls beyond the right edge.
    ui.horizontal_wrapped(|ui| {
        ui.label(ui.tr("Club ID:"));
        ui.add_enabled(
            !state.my_clubs,
            egui::TextEdit::singleline(&mut state.club_id)
                .desired_width(130.0)
                .hint_text("Exact current club UID"),
        )
        .on_hover_text(
            "Matches the current club UID only, without related clubs or outgoing loans.",
        );
    });
    ui.separator();
    section_title(ui, &ui.tr("Inclusive ranges"));
    for (label, min, max) in [
        ("Age", &mut state.age_min, &mut state.age_max),
        ("CA", &mut state.ca_min, &mut state.ca_max),
        ("PA", &mut state.pa_min, &mut state.pa_max),
    ] {
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("{label} ≥"));
            ui.add(
                egui::TextEdit::singleline(min)
                    .desired_width(44.0)
                    .hint_text("min"),
            );
            ui.label("≤");
            ui.add(
                egui::TextEdit::singleline(max)
                    .desired_width(44.0)
                    .hint_text("max"),
            );
        });
    }
    ui.separator();
    ui.label(egui::RichText::new(ui.tr("Positions")).small().strong())
        .on_hover_text("Any selected position (OR). Minimum familiarity: 1–20.\n1 = None · 20 = Natural. Default minimum: 2.");
    ui.add_space(2.0);
    for (bit, &(_, label)) in POSITION_OFFSETS.iter().enumerate() {
        ui.push_id(bit, |ui| {
            ui.horizontal_wrapped(|ui| {
                let mut selected = state.positions & (1 << bit) != 0;
                let name = if bit == 1 { "SW (Sweeper)" } else { label };
                ui.allocate_ui_with_layout(
                    egui::vec2(130.0, ui.spacing().interact_size.y),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_width(130.0);
                        if ui.checkbox(&mut selected, name).changed() {
                            if selected {
                                state.positions |= 1 << bit;
                            } else {
                                state.positions &= !(1 << bit);
                            }
                        }
                    },
                );
                ui.horizontal(|ui| {
                    ui.label("≥ min");
                    ui.add(
                        egui::DragValue::new(&mut state.position_minimums[bit])
                            .range(1..=20)
                            .speed(1),
                    );
                });
            });
        });
    }
}

fn section_title(ui: &mut egui::Ui, title: &str) {
    ui.label(egui::RichText::new(title).small().strong());
    ui.add_space(2.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn text_shapes(shape: &egui::epaint::Shape, texts: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::epaint::Shape::Text(text) => texts.push((
                text.galley.job.text.clone(),
                egui::Rect::from_min_size(text.pos, text.galley.size()),
            )),
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    text_shapes(shape, texts);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn position_minima_align_despite_different_checkbox_label_lengths() {
        let ctx = egui::Context::default();
        let mut state = PlayerSearchState::default();
        state.position_minimums = [20; 15];
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(500.0, 1000.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| show_general_tab(ui, &mut state));
            },
        );
        let mut texts = Vec::new();
        for clipped in output.shapes {
            text_shapes(&clipped.shape, &mut texts);
        }
        let numeric_x = |label: &str| {
            let (_, label_rect) = texts.iter().find(|(text, _)| text == label).unwrap();
            let (_, numeric_rect) = texts
                .iter()
                .find(|(text, rect)| {
                    text == "20" && (rect.center().y - label_rect.center().y).abs() < 2.0
                })
                .unwrap();
            numeric_rect.min.x
        };
        let gk_x = numeric_x("GK");
        for label in ["SW (Sweeper)", "AML"] {
            assert!(
                (numeric_x(label) - gk_x).abs() < 0.1,
                "{label} minimum must align with GK"
            );
        }
    }

    #[test]
    fn small_viewports_keep_footer_visible_and_all_positions_scroll_into_view() {
        for size in [egui::vec2(320.0, 300.0), egui::vec2(260.0, 360.0)] {
            let ctx = egui::Context::default();
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let mut state = PlayerSearchState::default();
            state.position_minimums = [20; 15];
            let mut open = true;
            let mut seen = HashSet::new();
            for frame in 0..40 {
                let mut input = egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 10.0),
                    ..Default::default()
                };
                input.events.push(egui::Event::PointerMoved(egui::pos2(
                    size.x / 2.0,
                    size.y / 2.0,
                )));
                if frame > 2 {
                    input.events.push(egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, -55.0),
                        modifiers: egui::Modifiers::default(),
                    });
                }
                let output = ctx.run(input, |ctx| {
                    show(ctx, &mut state, &mut open);
                });
                if frame < 2 {
                    continue;
                }
                let mut visible = HashSet::new();
                for clipped in output.shapes {
                    let mut texts = Vec::new();
                    text_shapes(&clipped.shape, &mut texts);
                    for (text, rect) in texts {
                        if screen.contains_rect(rect) && clipped.clip_rect.contains_rect(rect) {
                            visible.insert(text.clone());
                            seen.insert(text);
                        }
                    }
                }
                for footer in ["Clear All", "Cancel", "OK"] {
                    assert!(
                        visible.contains(footer),
                        "{footer} hidden at {size:?}, frame {frame}"
                    );
                }
            }
            assert_eq!(
                state.positions, 0,
                "Rendering minima must not select positions"
            );
            assert_eq!(state.position_minimums, [20; 15]);
            for (_, label) in POSITION_OFFSETS {
                let name = if label == "SW" { "SW (Sweeper)" } else { label };
                assert!(seen.contains(name), "{name} unreachable at {size:?}");
            }
        }
    }
}
