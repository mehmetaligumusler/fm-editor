//! FM24 per-version field tables. Mirrors `fields::fm26`: each sub-module
//! owns one entity type's `FIELDS` const slice and this module collects them
//! into the `FM24` `VersionedSchemas`.
//!
//! FM24 has no `game_plugin.dll` — the game logic and every entity-array RVA
//! live in `fm.exe` itself (image base 0x140000000), so `gp_base` is the
//! fm.exe base on this version (see `GameVersion::module_name`).

mod club;
mod player;
mod stadium;
pub mod staff;

pub(crate) use club::CLUB_SUB_RECORDS;
pub(crate) use club::FIELDS as CLUB_FIELDS;
pub(crate) use player::FIELDS as PLAYER_FIELDS;
pub(crate) use player::PLAYER_SUB_RECORDS;
pub(crate) use stadium::FIELDS as STADIUM_FIELDS;
pub(crate) use staff::FIELDS as STAFF_FIELDS;

use crate::fields::VersionedSchemas;

/// FM24 schema — mapped 2026-10-22 against a fresh save (3 Jul 2023).
///
/// RVAs are relative to the fm.exe base. Each `(data, end)` pair is a
/// std::vector of object pointers in fm.exe's writable data:
///
/// - players `0x642EDD0` — 83,426 entries, 99.8% carry the player vtable
///   (fm.exe+0x5A4E958); contains Harry Kane. The neighbouring 116,856-entry
///   pair at 0x642EDB8 points 0x28 into a different object type.
/// - staff `0x642EDE8` — 33,835 entries (staff vtable fm.exe+0x5A4C2E8),
///   human manager last. The 24,775-entry pair at 0x642EDF0 is a subset.
/// - clubs `0x642EF58` — 36,725 entries (club vtable fm.exe+0x5A51F38); the
///   36,498-entry pair at 0x642EF88 is a strict subset.
///
/// The stadium array root is not located; stadiums are reached per club via
/// the `home_stadium` sub-record, so `rva_stadium_base` stays 0.
pub static FM24: VersionedSchemas = VersionedSchemas {
    player_fields: PLAYER_FIELDS,
    staff_fields: STAFF_FIELDS,
    club_fields: CLUB_FIELDS,
    stadium_fields: STADIUM_FIELDS,

    player_sub_records: PLAYER_SUB_RECORDS,
    staff_sub_records: &[],
    club_sub_records: CLUB_SUB_RECORDS,
    stadium_sub_records: &[],
    player_lists: None,
    morale_labels: player::MORALE_LABELS,
    person_profile: None,

    player_struct_size: 0x3E8, // pool stride
    staff_struct_size: 0x268,  // pool stride
    club_read_size: 0x200,     // furthest mapped club field is +0x150; objects are 0x170 apart
    stadium_entity_stride: 0xB0,

    human_manager_shift: staff::HUMAN_MANAGER_SHIFT,
    manager_shifted_fields: staff::MANAGER_SHIFTED_FIELDS,

    rva_humans_data: 0x642EDD0,
    rva_humans_end: 0x642EDD8,
    rva_staff_data: 0x642EDE8,
    rva_staff_end: 0x642EDF0,
    rva_clubs_data: 0x642EF58,
    rva_clubs_end: 0x642EF60,
    rva_stadium_base: 0,
    rva_current_date: 0,

    // Counts observed in the mapping save (used by scan_arrays.rs labels).
    fmrte_players: 83426,
    fmrte_staff: 33835,
    fmrte_clubs: 36725,
    fmrte_humans: 1,

    // FNV-1a-style hash (see `process::hash_dll_header`) of the first 64 KB
    // of fm.exe for the FM24 build this schema was verified against.
    known_dll_hash: 0xFB39_952A_2028_7907,
    full_finance_vtable_rva: 0,
    player_coach_vtable_rva: 0,
    player_person_offset: 0,
    player_coach_shift: 0,
    human_manager_vtable_rva: 0,
    age_reference_year: 2023,
};

/// Epic Windows read-only candidate layout. Array roots were observed live;
/// inherited field offsets are research leads, not verified editing support.
/// See docs/fm24/epic-preview.md. Never remove the build-level write lock
/// without independent in-game field witnesses and acknowledged edit/Undo tests.
pub static FM24_EPIC: VersionedSchemas = VersionedSchemas {
    player_fields: PLAYER_FIELDS,
    staff_fields: STAFF_FIELDS,
    club_fields: CLUB_FIELDS,
    stadium_fields: STADIUM_FIELDS,

    player_sub_records: PLAYER_SUB_RECORDS,
    staff_sub_records: &[],
    club_sub_records: CLUB_SUB_RECORDS,
    stadium_sub_records: &[],
    player_lists: None,
    morale_labels: player::MORALE_LABELS,
    person_profile: None,

    player_struct_size: 0x3E8, // pool stride
    staff_struct_size: 0x268,  // pool stride
    club_read_size: 0x200,     // furthest mapped club field is +0x150; objects are 0x170 apart
    stadium_entity_stride: 0xB0,

    human_manager_shift: staff::HUMAN_MANAGER_SHIFT,
    manager_shifted_fields: staff::MANAGER_SHIFTED_FIELDS,

    rva_humans_data: 0x642EDD0,
    rva_humans_end: 0x642EDD8,
    rva_staff_data: 0x642EDE8,
    rva_staff_end: 0x642EDF0,
    rva_clubs_data: 0x642EF58,
    rva_clubs_end: 0x642EF60,
    rva_stadium_base: 0,
    // Current in-game date, witnessed 2026-11-08 on the user's live career:
    // the packed DateDayYear pair for 21 June 2026 (day 172, year 2026) held
    // stably at module+0x642F458, inside the globals cluster just past the
    // entity-array roots. Two other stable copies live in string/buffer
    // regions (0x631D5BC, 0x6356C90) and were not chosen. Steam FM24 keeps
    // 0 (unwitnessed on that build).
    rva_current_date: 0x642F458,

    // Counts observed in the mapping save (used by scan_arrays.rs labels).
    fmrte_players: 75726,
    fmrte_staff: 38258,
    fmrte_clubs: 36046,
    fmrte_humans: 1,

    // FNV-1a-style hash (see `process::hash_dll_header`) of the first 64 KB
    // of fm.exe for the FM24 build this schema was verified against.
    known_dll_hash: 0x8FA1_13DB_9DCD_8883,
    full_finance_vtable_rva: 0,
    player_coach_vtable_rva: 0,
    player_person_offset: 0,
    player_coach_shift: 0,
    human_manager_vtable_rva: 0,
    age_reference_year: 2023,
};
