//! Read-only, decoded player snapshots. No source buffers are retained.

use crate::fields::{Category, Encoding, FieldLocation, HasSubRecords};
use crate::game_date::GameDate;
use crate::game_version::GameBuild;
use crate::i18n::UiTr;
use crate::player::PlayerRecord;
use eframe::egui;

const GROUPS: [(Category, &str); 5] = [
    (Category::Technical, "Technical"),
    (Category::SetPieces, "Set pieces"),
    (Category::Mental, "Mental"),
    (Category::Physical, "Physical"),
    (Category::Goalkeeping, "Goalkeeping"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Identity {
    build: GameBuild,
    addr: u64,
    uid: u32,
}

impl Identity {
    fn of(player: &PlayerRecord) -> Option<Self> {
        // UID is a four-byte identifier, including schemas whose legacy field
        // annotation is Raw (the profile and face lookup also read it as u32).
        let uid = player.field_u32("p_uid").filter(|uid| *uid != 0)?;
        (player.addr > 0x10000 && player.addr < 0x7FFF_FFFF_FFFF).then_some(Self {
            build: player.build,
            addr: player.addr,
            uid,
        })
    }
}

#[derive(Debug)]
struct Attribute {
    key: &'static str,
    label: &'static str,
    category: Category,
    value: Option<u8>,
}

#[derive(Debug)]
struct Snapshot {
    identity: Identity,
    name: String,
    club: Option<String>,
    today: Option<GameDate>,
    captured_at: std::time::Instant,
    age: Option<u8>,
    ca: Option<u32>,
    pa: Option<u32>,
    wage: Option<u32>,
    expiry: Option<String>,
    attributes: Vec<Attribute>,
}

impl Snapshot {
    fn capture(player: &PlayerRecord, identity: Identity, today: Option<GameDate>) -> Self {
        let schemas = player.build.schemas();
        let attributes = schemas
            .player_fields
            .iter()
            .filter(|f| GROUPS.iter().any(|(c, _)| *c == f.category))
            .map(|f| Attribute {
                key: f.key,
                label: f.label,
                category: f.category,
                value: crate::fields::read_field_u8(player, &player.raw, f)
                    .filter(|v| (1..=20).contains(v)),
            })
            .collect();
        Self {
            identity,
            name: player
                .name
                .clone()
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| "Name unavailable".into()),
            club: player.club_name.clone(),
            today,
            captured_at: std::time::Instant::now(),
            age: super::entity_header::age(schemas, today, &player.raw, "p_dob"),
            ca: player.field_int("p_ca").filter(|v| (1..=200).contains(v)),
            pa: player.field_int("p_pa").filter(|v| (1..=200).contains(v)),
            wage: contract_wage(player),
            expiry: date_field(player, "p_contract_expiry"),
            attributes,
        }
    }

    fn attribute(&self, key: &str) -> Option<u8> {
        self.attributes.iter().find(|a| a.key == key)?.value
    }
}

fn date_field(player: &PlayerRecord, key: &str) -> Option<String> {
    let location = player.build.schemas().field(key)?.offset?;
    let (raw, offset) = match location {
        FieldLocation::Direct { offset, .. } => (player.raw.as_slice(), offset),
        FieldLocation::SubRecord {
            container, offset, ..
        } => (player.sub_record(container)?, offset),
    };
    let offset = usize::try_from(offset).ok()?;
    GameDate::from_bytes(raw.get(offset..offset.checked_add(4)?)?)?;
    super::player_panel::fmt_date_at(raw, offset)
}

fn contract_wage(player: &PlayerRecord) -> Option<u32> {
    // Prefer the current contract. A legacy generic Raw field is not an
    // explicitly mapped monetary u32; reading it as u16 would truncate wages,
    // while reading four bytes would exceed its declared schema width.
    ["p_contract_wage", "p_weekly_wage"]
        .into_iter()
        .find_map(|key| {
            let location = player.build.schemas().field(key)?.offset?;
            matches!(location.encoding(), Encoding::RawU32)
                .then(|| player.field_u32(key))
                .flatten()
        })
}

/// Two independent decoded snapshots, never editable records or live readers.
#[derive(Default)]
pub struct PlayerComparison {
    slots: [Option<Snapshot>; 2],
    open: bool,
    message: Option<String>,
    profile: Option<Identity>,
}

impl PlayerComparison {
    /// Also closes the window; use on attachment/reload invalidation.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    fn capture(
        &mut self,
        slot: usize,
        player: &PlayerRecord,
        today: Option<GameDate>,
        enabled: bool,
    ) {
        if !enabled {
            return;
        }
        let Some(identity) = Identity::of(player) else {
            self.message = Some("Selection unavailable: player identity is missing.".into());
            return;
        };
        self.profile = Some(identity);
        if slot == 1 {
            let Some(selected) = &self.slots[0] else {
                return;
            };
            if selected.identity.build != identity.build {
                self.message = Some(format!(
                    "Cannot compare {} with {}: different builds. Select this player instead to start a new comparison.",
                    player.name.as_deref().unwrap_or("this player"),
                    selected.name
                ));
                return;
            }
            if selected.identity == identity {
                self.message = Some(format!(
                    "{} is already selected. Choose a different player.",
                    selected.name
                ));
                return;
            }
        }
        let snapshot = Snapshot::capture(player, identity, today);
        self.message = None;
        self.slots[slot] = Some(snapshot);
        if slot == 0 {
            self.slots[1] = None;
        } else {
            self.open = true;
        }
    }

    pub fn controls(
        &mut self,
        ui: &mut egui::Ui,
        player: &PlayerRecord,
        today: Option<GameDate>,
        enabled: bool,
    ) {
        let identity = Identity::of(player);
        if self.profile != identity {
            self.message = None;
            self.profile = identity;
        }
        ui.horizontal_wrapped(|ui| {
            let can_capture = enabled && identity.is_some();
            let selected_here = self.slots[0].as_ref().is_some_and(|s| Some(s.identity) == identity);
            if selected_here {
                ui.add_enabled(false, egui::Button::new("Selected for compare"));
                if ui.button(ui.tr("Clear selection")).clicked() {
                    self.clear();
                }
            } else {
                if self.slots[0].is_some()
                    && ui.add_enabled(can_capture, egui::Button::new(ui.tr("Compare with selected"))).clicked()
                {
                    self.capture(1, player, today, enabled);
                }
                if ui.add_enabled(can_capture, egui::Button::new(ui.tr("Select for compare"))).clicked() {
                    self.capture(0, player, today, enabled);
                }
            }
            if let Some(selected) = &self.slots[0] {
                ui.add(egui::Label::new(format!("Selected: {}", selected.name)).truncate())
                    .on_hover_text(&selected.name);
            }
            if !can_capture {
                ui.label("Selection unavailable: wait for pending changes, or load a valid player profile.");
            }
        });
        if let Some(message) = &self.message {
            ui.add(egui::Label::new(message).truncate())
                .on_hover_text(message);
        }
    }

    pub fn toolbar(&mut self, ui: &mut egui::Ui) {
        let count = self.slots.iter().flatten().count();
        if ui
            .button(format!("{} ({count}/2)", ui.tr("Compare players")))
            .clicked()
        {
            self.open = true;
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        let mut open = self.open;
        let screen = ctx.screen_rect();
        let width = (screen.width() - 32.0).clamp(100.0, 720.0);
        egui::Window::new("Player comparison")
            .id(egui::Id::new("player_comparison"))
            .open(&mut open)
            .default_width(width)
            .max_width(width)
            .min_width(0.0)
            .default_height((screen.height() - 64.0).clamp(100.0, 650.0))
            .max_height((screen.height() - 32.0).max(60.0))
            .vscroll(false)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button(ui.tr("Clear selection")).clicked() {
                        self.slots = [None, None];
                        self.message = None;
                    }
                    if ui.add_enabled(self.slots.iter().all(Option::is_some), egui::Button::new("Swap players")).clicked() {
                        self.slots.swap(0, 1);
                        self.message = None;
                    }
                    ui.label("Read-only snapshots").on_hover_text(
                        "Values do not auto-refresh. Reload a profile and select it again to update. No game memory is written. — means missing, invalid or unmapped data. Identity colors distinguish players, not better values. Age uses the captured game date, or the build's reference year."
                    );
                });
                if self.slots.iter().flatten().any(|s| s.identity.build == GameBuild::Fm27GamePass) {
                    ui.label(egui::RichText::new("Game Pass layout unverified").color(ui.visuals().warn_fg_color));
                }
                ui.separator();
                // Leave room for the scrollbar in both the pinned header and body.
                let widths = ColumnWidths::new((ui.available_width() - 16.0).max(0.0), ui.spacing().item_spacing.x);
                table_row(ui, widths, false, |ui, column| {
                    if column == 0 {
                        ui.strong("Attribute / field");
                    } else if let Some(snapshot) = &self.slots[column - 1] {
                        identity_ui(ui, snapshot, column - 1);
                    } else {
                        ui.label("No player selected")
                            .on_hover_text("Open a player profile and choose Select for compare.");
                    }
                });
                ui.separator();
                let height = ui.available_height().min(screen.bottom() - ui.cursor().top() - 24.0).max(0.0);
                egui::ScrollArea::vertical()
                    .id_salt(("comparison_rows", self.slots.each_ref().map(|s| s.as_ref().map(|s| (s.identity.addr, s.captured_at)))))
                    .auto_shrink([false, false])
                    .max_height(height)
                    .show(ui, |ui| {
                        egui::CollapsingHeader::new("Summary").default_open(true).show_unindented(ui, |ui| {
                            for (i, (label, a, b)) in self.summary().into_iter().enumerate() {
                                value_row(ui, widths, label, [&a, &b], [None, None], i % 2 == 0);
                            }
                        });
                        for (category, title) in GROUPS {
                            ui.add_space(6.0);
                            ui.strong(title);
                            ui.separator();
                            // Union by stable schema key, not field position or label.
                            let mut keys = Vec::new();
                            for snapshot in self.slots.iter().flatten() {
                                for attribute in snapshot.attributes.iter().filter(|a| a.category == category) {
                                    if !keys.contains(&attribute.key) { keys.push(attribute.key); }
                                }
                            }
                            for (i, key) in keys.into_iter().enumerate() {
                                let label = self.slots.iter().flatten().flat_map(|s| &s.attributes)
                                    .find(|a| a.key == key).map(|a| a.label).unwrap_or(key);
                                let values = self.slots.each_ref().map(|s| s.as_ref().and_then(|s| s.attribute(key)));
                                value_row(ui, widths, label, [&number(values[0]), &number(values[1])], values, i % 2 == 0);
                            }
                        }
                    });
            });
        self.open = open;
    }

    fn summary(&self) -> Vec<(&'static str, String, String)> {
        let values = |s: Option<&Snapshot>| -> [String; 5] {
            match s {
                Some(s) => [
                    number(s.age),
                    number(s.ca),
                    number(s.pa),
                    number(s.wage),
                    s.expiry.clone().unwrap_or_else(|| "—".into()),
                ],
                None => std::array::from_fn(|_| "—".into()),
            }
        };
        let a = values(self.slots[0].as_ref());
        let b = values(self.slots[1].as_ref());
        ["Age", "CA", "PA", "Wage (GBP/week)", "Contract expiry"]
            .into_iter()
            .zip(a)
            .zip(b)
            .map(|((label, a), b)| (label, a, b))
            .collect()
    }
}

fn number(value: Option<impl std::fmt::Display>) -> String {
    value.map(|v| v.to_string()).unwrap_or_else(|| "—".into())
}

fn identity_color(ui: &egui::Ui, column: usize) -> egui::Color32 {
    match (ui.visuals().dark_mode, column) {
        (true, 0) => egui::Color32::from_rgb(115, 185, 255),
        (true, _) => egui::Color32::from_rgb(240, 200, 100),
        (false, 0) => egui::Color32::from_rgb(25, 85, 160),
        (false, _) => egui::Color32::from_rgb(125, 85, 0),
    }
}

fn identity_ui(ui: &mut egui::Ui, snapshot: &Snapshot, column: usize) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(&snapshot.name)
                .strong()
                .color(identity_color(ui, column)),
        )
        .truncate(),
    )
    .on_hover_text(format!(
        "{}\nUID {} · {}\nGame date: {}\nCaptured {} seconds ago",
        snapshot.name,
        snapshot.identity.uid,
        snapshot.identity.build.display_name(),
        snapshot
            .today
            .map(|d| format!("day {} of {}", d.day, d.year))
            .unwrap_or_else(|| "unavailable".into()),
        snapshot.captured_at.elapsed().as_secs()
    ));
    if let Some(club) = &snapshot.club {
        ui.add(egui::Label::new(egui::RichText::new(club).small()).truncate())
            .on_hover_text(club);
    }
}

#[derive(Clone, Copy)]
struct ColumnWidths([f32; 3]);

impl ColumnWidths {
    fn new(available: f32, spacing: f32) -> Self {
        let usable = (available - 2.0 * spacing).max(0.0);
        Self([usable * 0.42, usable * 0.29, usable * 0.29])
    }
}

// A shared width calculation and explicit minimum cell widths pin every value
// beneath its player header, even when labels are short or truncated.
fn table_row(
    ui: &mut egui::Ui,
    widths: ColumnWidths,
    striped: bool,
    mut cell: impl FnMut(&mut egui::Ui, usize),
) {
    let start = ui.cursor().min;
    let fill = if striped {
        ui.visuals().faint_bg_color
    } else {
        egui::Color32::TRANSPARENT
    };
    let background = ui.painter().add(egui::Shape::Noop);
    let row = ui.horizontal_top(|ui| {
        for (column, width) in widths.0.into_iter().enumerate() {
            ui.allocate_ui_with_layout(
                egui::vec2(width, 0.0),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    ui.set_min_width(width);
                    ui.set_max_width(width);
                    cell(ui, column);
                },
            );
        }
    });
    ui.painter().set(
        background,
        egui::Shape::rect_filled(
            egui::Rect::from_min_max(start, row.response.rect.max),
            2.0,
            fill,
        ),
    );
}

fn value_row(
    ui: &mut egui::Ui,
    widths: ColumnWidths,
    label: &str,
    values: [&str; 2],
    attributes: [Option<u8>; 2],
    striped: bool,
) {
    table_row(ui, widths, striped, |ui, column| {
        if column == 0 {
            ui.add(egui::Label::new(label).truncate())
                .on_hover_text(label);
        } else {
            let value = values[column - 1];
            let mut text = egui::RichText::new(value).strong();
            if let Some(attribute) = attributes[column - 1] {
                let color = super::field_widget::attr_color(attribute, ui);
                // Preserve the shared attribute bands, with opaque darker ink
                // on light backgrounds (the lime band otherwise lacks contrast).
                let color = if ui.visuals().dark_mode {
                    color
                } else {
                    egui::Color32::from_rgb(
                        (color.r() as f32 * 0.5) as u8,
                        (color.g() as f32 * 0.5) as u8,
                        (color.b() as f32 * 0.5) as u8,
                    )
                };
                text = text.color(color);
            }
            let response = ui.add(egui::Label::new(text).truncate());
            if value == "—" {
                response.on_hover_text("Unavailable: missing, invalid or unmapped data.");
            } else {
                response.on_hover_text(value);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fields::Encoding;
    use crate::player::ListState;

    fn fixture(build: GameBuild, uid: u32) -> PlayerRecord {
        let mut p = PlayerRecord {
            build,
            addr: 0x20000 + u64::from(uid) * 0x1000,
            index: 0,
            name: Some("A very long player name ".repeat(16)),
            raw: vec![0; build.schemas().player_struct_size],
            person_shift: 0,
            contract_raw: Some(vec![0; 256]),
            contract_addr: None,
            club_name: Some("A long club name ".repeat(20)),
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
        set(&mut p, "p_uid", &uid.to_le_bytes());
        p
    }

    fn set(p: &mut PlayerRecord, key: &str, bytes: &[u8]) {
        let location = p.build.schemas().field(key).unwrap().offset.unwrap();
        let (raw, offset) = match location {
            FieldLocation::Direct { offset, .. } => (&mut p.raw, offset as usize),
            FieldLocation::SubRecord { offset, .. } => {
                (p.contract_raw.as_mut().unwrap(), offset as usize)
            }
        };
        raw[offset..offset + bytes.len()].copy_from_slice(bytes);
    }

    fn set_attribute(p: &mut PlayerRecord, key: &str, value: u8) {
        let encoding = p
            .build
            .schemas()
            .field(key)
            .unwrap()
            .offset
            .unwrap()
            .encoding();
        set(
            p,
            key,
            &[if encoding == Encoding::Encoded {
                value * 5
            } else {
                value
            }],
        );
    }

    fn first_key(p: &PlayerRecord) -> &'static str {
        p.build
            .schemas()
            .player_fields
            .iter()
            .find(|f| f.category == Category::Technical && f.offset.is_some())
            .unwrap()
            .key
    }

    #[test]
    fn snapshots_decode_each_build_schema_and_remain_immutable() {
        for build in GameBuild::ALL {
            let mut p = fixture(build, 42);
            let key = first_key(&p);
            set_attribute(&mut p, key, 17);
            set(&mut p, "p_ca", &137u16.to_le_bytes());
            set(&mut p, "p_pa", &181u16.to_le_bytes());
            set(&mut p, "p_contract_wage", &123456u32.to_le_bytes());
            // A neighbouring-byte pattern in a legacy direct field must not
            // override the explicitly typed current-contract wage.
            if build
                .schemas()
                .field("p_weekly_wage")
                .is_some_and(|f| f.offset.is_some())
            {
                set(&mut p, "p_weekly_wage", &[0x39, 0x30, 0xAB, 0xCD]);
            }
            set(
                &mut p,
                "p_contract_expiry",
                &[181u16.to_le_bytes(), 2027u16.to_le_bytes()].concat(),
            );
            set(
                &mut p,
                "p_dob",
                &[1u16.to_le_bytes(), 2000u16.to_le_bytes()].concat(),
            );
            let mut comparison = PlayerComparison::default();
            comparison.capture(
                0,
                &p,
                Some(GameDate {
                    day: 100,
                    year: 2026,
                }),
                true,
            );
            let snapshot = comparison.slots[0].as_ref().unwrap();
            assert_eq!(snapshot.identity.uid, 42);
            assert_eq!(snapshot.attribute(key), Some(17));
            assert_eq!(
                (snapshot.ca, snapshot.pa, snapshot.wage, snapshot.age),
                (Some(137), Some(181), Some(123456), Some(26))
            );
            assert!(snapshot.expiry.as_ref().unwrap().contains("2027"));
            assert!(
                snapshot
                    .attributes
                    .iter()
                    .all(|a| GROUPS.iter().any(|(c, _)| *c == a.category))
            );
            p.raw.fill(0);
            p.contract_raw.as_mut().unwrap().fill(0);
            assert_eq!(
                comparison.slots[0].as_ref().unwrap().attribute(key),
                Some(17)
            );
        }
    }

    #[test]
    fn legacy_wage_width_is_not_mistaken_for_a_monetary_mapping() {
        let mut p = fixture(GameBuild::Fm26Steam, 42);
        set(&mut p, "p_weekly_wage", &[0x39, 0x30, 0xAB, 0xCD]);
        set(&mut p, "p_contract_wage", &222086u32.to_le_bytes());
        assert_eq!(contract_wage(&p), Some(222086));
        p.contract_raw = None;
        assert_eq!(contract_wage(&p), None);
    }

    #[test]
    fn unavailable_truncated_and_invalid_data_stay_missing() {
        for build in GameBuild::ALL {
            let mut p = fixture(build, 42);
            let key = first_key(&p);
            for invalid in [0, 21, 50] {
                set_attribute(&mut p, key, invalid);
                let s = Snapshot::capture(&p, Identity::of(&p).unwrap(), None);
                assert_eq!(s.attribute(key), None);
                assert_eq!(number(s.attribute(key)), "—");
            }
            for invalid in [0u16, 201, u16::MAX] {
                set(&mut p, "p_ca", &invalid.to_le_bytes());
                set(&mut p, "p_pa", &invalid.to_le_bytes());
                let s = Snapshot::capture(&p, Identity::of(&p).unwrap(), None);
                assert_eq!((s.ca, s.pa), (None, None));
            }
            let identity = Identity::of(&p).unwrap();
            p.raw.clear();
            p.contract_raw = Some(vec![0]);
            let s = Snapshot::capture(&p, identity, None);
            assert_eq!((s.age, s.ca, s.pa, s.wage), (None, None, None, None));
            assert!(s.expiry.is_none());
            assert!(s.attributes.iter().all(|a| a.value.is_none()));
            assert_eq!(s.attribute("unmapped_key"), None);
            assert_eq!(number(s.attribute("unmapped_key")), "—");
        }
    }

    #[test]
    fn identity_build_constraints_swap_and_clear() {
        let a = fixture(GameBuild::Fm24Steam, 42);
        let b = fixture(GameBuild::Fm24Steam, 43);
        let foreign = fixture(GameBuild::Fm27Steam, 44);
        let mut comparison = PlayerComparison::default();
        comparison.capture(0, &a, None, true);
        comparison.capture(1, &a, None, true);
        assert!(comparison.slots[1].is_none());
        assert!(comparison.message.as_ref().unwrap().contains("already"));
        comparison.capture(1, &foreign, None, true);
        assert!(comparison.slots[1].is_none());
        assert!(
            comparison
                .message
                .as_ref()
                .unwrap()
                .contains("different builds")
        );
        comparison.capture(1, &b, None, true);
        comparison.slots.swap(0, 1);
        assert_eq!(comparison.slots[0].as_ref().unwrap().identity.uid, 43);
        assert_eq!(comparison.slots[1].as_ref().unwrap().identity.uid, 42);
        let mut missing = a.clone();
        missing.raw.clear();
        comparison.capture(0, &missing, None, true);
        assert_eq!(comparison.slots[0].as_ref().unwrap().identity.uid, 43);
        comparison.open = true;
        comparison.clear();
        assert!(comparison.slots.iter().all(Option::is_none));
        assert!(!comparison.open);
        assert!(comparison.message.is_none());
    }

    #[test]
    fn capture_refusal_does_not_follow_another_player_profile() {
        let a = fixture(GameBuild::Fm24Steam, 42);
        let b = fixture(GameBuild::Fm24Steam, 43);
        let ctx = egui::Context::default();
        let mut comparison = PlayerComparison::default();
        comparison.capture(0, &a, None, true);
        comparison.capture(1, &a, None, true);
        assert!(comparison.message.as_ref().unwrap().contains("already"));
        frame(&ctx, &mut comparison, &b, true, 800.0, vec![]);
        assert!(comparison.message.is_none());
        assert_eq!(comparison.slots[0].as_ref().unwrap().identity.uid, 42);
        assert!(comparison.slots[1].is_none());
    }

    #[test]
    fn select_visit_compare_opens_and_reselection_replaces_both_snapshots() {
        let mut first = fixture(GameBuild::Fm24Steam, 42);
        first.name = Some("First Player".into());
        let mut second = fixture(first.build, 43);
        second.name = Some("Second Player".into());
        let foreign = fixture(GameBuild::Fm27Steam, 44);
        let ctx = egui::Context::default();
        let mut comparison = PlayerComparison::default();
        click(&ctx, &mut comparison, &first, true, "Select for compare");
        assert!(!comparison.open);
        assert_eq!(comparison.slots[0].as_ref().unwrap().name, "First Player");
        assert!(comparison.message.is_none());
        frame(&ctx, &mut comparison, &second, true, 800.0, vec![]);
        assert!(comparison.message.is_none());
        click(
            &ctx,
            &mut comparison,
            &second,
            true,
            "Compare with selected",
        );
        assert!(comparison.open);
        assert_eq!(comparison.slots[1].as_ref().unwrap().identity.uid, 43);
        assert_eq!(comparison.slots[1].as_ref().unwrap().name, "Second Player");
        assert!(comparison.message.is_none());
        // Close the floating window before interacting with the profile beneath it.
        comparison.open = false;
        click(&ctx, &mut comparison, &second, true, "Select for compare");
        assert_eq!(comparison.slots[0].as_ref().unwrap().identity.uid, 43);
        assert!(comparison.slots[1].is_none());
        // Starting a new selection safely replaces even a different build.
        click(&ctx, &mut comparison, &foreign, true, "Select for compare");
        assert_eq!(
            comparison.slots[0].as_ref().unwrap().identity.build,
            foreign.build
        );
        assert!(comparison.slots[1].is_none());
        click(&ctx, &mut comparison, &foreign, true, "Clear selection");
        assert!(comparison.slots.iter().all(Option::is_none));
    }

    fn frame(
        ctx: &egui::Context,
        comparison: &mut PlayerComparison,
        p: &PlayerRecord,
        enabled: bool,
        width: f32,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
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
                    comparison.controls(ui, p, None, enabled);
                    comparison.toolbar(ui);
                });
                comparison.show(ctx);
            },
        )
    }

    fn click(
        ctx: &egui::Context,
        comparison: &mut PlayerComparison,
        p: &PlayerRecord,
        enabled: bool,
        label: &str,
    ) {
        let output = frame(ctx, comparison, p, enabled, 800.0, vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(t) if t.galley.job.text == label => {
                    Some(t.pos + t.galley.size() * 0.5)
                }
                _ => None,
            })
            .expect("button label");
        for pressed in [true, false] {
            frame(
                ctx,
                comparison,
                p,
                enabled,
                800.0,
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

    #[test]
    fn disabled_and_missing_identity_clicks_do_not_capture_but_enabled_click_does() {
        for missing in [false, true] {
            let mut p = fixture(GameBuild::Fm27Steam, 42);
            if missing {
                p.raw.clear();
            }
            let ctx = egui::Context::default();
            let mut comparison = PlayerComparison::default();
            click(&ctx, &mut comparison, &p, missing, "Select for compare");
            assert!(comparison.slots.iter().all(Option::is_none));
        }
        let p = fixture(GameBuild::Fm27Steam, 42);
        let ctx = egui::Context::default();
        let mut comparison = PlayerComparison::default();
        click(&ctx, &mut comparison, &p, true, "Select for compare");
        assert!(comparison.slots[0].is_some());
    }

    #[test]
    fn comparison_columns_align_across_different_label_lengths() {
        for width in [640.0, 320.0] {
            let ctx = egui::Context::default();
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
                        let widths =
                            ColumnWidths::new(ui.available_width(), ui.spacing().item_spacing.x);
                        table_row(ui, widths, false, |ui, column| {
                            ui.label(["Attribute", "First name", "Second name"][column]);
                        });
                        value_row(
                            ui,
                            widths,
                            "Short",
                            ["11", "12"],
                            [Some(11), Some(12)],
                            false,
                        );
                        value_row(
                            ui,
                            widths,
                            "A much longer attribute label",
                            ["13", "14"],
                            [Some(13), Some(14)],
                            true,
                        );
                    });
                },
            );
            let x = |label: &str| {
                output
                    .shapes
                    .iter()
                    .find_map(|s| match &s.shape {
                        egui::Shape::Text(t) if t.galley.text() == label => Some(t.pos.x),
                        _ => None,
                    })
                    .expect("cell text visible")
            };
            assert!((x("11") - x("13")).abs() < 0.1);
            assert!((x("12") - x("14")).abs() < 0.1);
            assert!((x("First name") - x("13")).abs() < 0.1);
            assert!((x("Second name") - x("14")).abs() < 0.1);
            assert!(x("14") < width);
            let colored = output.shapes.iter().any(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == "13" => t
                    .galley
                    .job
                    .sections
                    .iter()
                    .any(|s| s.format.color == super::super::field_widget::value_color(13)),
                _ => false,
            });
            assert!(colored, "attribute text uses the existing value palette");
        }
    }

    #[test]
    fn named_headers_stay_pinned_while_body_scrolls_and_new_pair_starts_at_top() {
        for (width, height) in [(1280.0, 700.0), (700.0, 500.0), (400.0, 350.0)] {
            let ctx = egui::Context::default();
            let mut first = fixture(GameBuild::Fm24Steam, 42);
            first.name = Some("First Player".into());
            let mut second = fixture(first.build, 43);
            second.name = Some("Second Player".into());
            let mut comparison = PlayerComparison::default();
            comparison.capture(0, &first, None, true);
            comparison.capture(1, &second, None, true);
            let render = |comparison: &mut PlayerComparison, events| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, height),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| comparison.show(ctx),
                )
            };
            for _ in 0..3 {
                render(&mut comparison, vec![]);
            }
            let before = render(&mut comparison, vec![]);
            let find = |output: &egui::FullOutput, text: &str| {
                output
                    .shapes
                    .iter()
                    .find_map(|s| match &s.shape {
                        egui::Shape::Text(t) if t.galley.job.text == text => {
                            Some((t.pos, s.clip_rect))
                        }
                        _ => None,
                    })
                    .expect(text)
            };
            let header = find(&before, "First Player");
            let other = find(&before, "Second Player");
            assert!(header.1.contains(header.0));
            assert!(other.1.contains(other.0));
            let body = find(&before, "Technical").0;
            let pointer = egui::pos2(header.0.x, (body.y + 60.0).min(height - 45.0));
            for _ in 0..6 {
                render(
                    &mut comparison,
                    vec![
                        egui::Event::PointerMoved(pointer),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, -100.0),
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            let after = render(&mut comparison, vec![]);
            assert_eq!(find(&after, "First Player").0, header.0);
            assert_eq!(find(&after, "Second Player").0, other.0);
            assert!(
                after
                    .shapes
                    .iter()
                    .find_map(|s| match &s.shape {
                        egui::Shape::Text(t) if t.galley.job.text == "Technical" => Some(t.pos.y),
                        _ => None,
                    })
                    .is_none_or(|y| y < body.y),
                "body must scroll at {width}px"
            );
            for shape in &after.shapes {
                if let egui::Shape::Text(t) = &shape.shape {
                    assert!(
                        !["A", "B", "B − A", "B-A", "Swap A / B"]
                            .contains(&t.galley.job.text.as_str())
                    );
                }
            }
            comparison.capture(1, &second, None, true);
            let reset = render(&mut comparison, vec![]);
            assert!((find(&reset, "Technical").0.y - body.y).abs() < 0.1);
            let rect = ctx
                .memory(|m| m.area_rect(egui::Id::new("player_comparison")))
                .unwrap();
            assert!(
                rect.width() <= width && rect.height() <= height,
                "window fits {width}x{height}: {rect:?}"
            );
        }
    }

    #[test]
    fn toolbar_opens_empty_window_and_responsive_render_preserves_buffers() {
        let p = fixture(GameBuild::Fm27Steam, 42);
        let raw = p.raw.clone();
        let contract = p.contract_raw.clone();
        let ctx = egui::Context::default();
        let mut comparison = PlayerComparison::default();
        click(&ctx, &mut comparison, &p, true, "Compare players (0/2)");
        assert!(comparison.open);
        comparison.capture(0, &p, None, true);
        comparison.capture(1, &fixture(p.build, 43), None, true);
        for width in [1280.0, 700.0, 400.0] {
            let ctx = egui::Context::default();
            for _ in 0..3 {
                let output = frame(&ctx, &mut comparison, &p, true, width, vec![]);
                assert!(!output.shapes.is_empty());
            }
            let rect = ctx
                .memory(|m| m.area_rect(egui::Id::new("player_comparison")))
                .unwrap();
            assert!(
                rect.width() <= width,
                "window width {} exceeds viewport {width}",
                rect.width()
            );
        }
        assert_eq!(p.raw, raw);
        assert_eq!(p.contract_raw, contract);
    }
}
