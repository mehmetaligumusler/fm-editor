//! FM27 per-version field tables, same layout as `fields::fm26`.
//!
//! Fields not listed as verified in docs/fm27/memory-layout.md remain
//! inherited leads from FM26, not verified FM27 mappings.

mod club;
#[cfg(debug_assertions)]
pub(crate) mod contract;
mod player;
pub(crate) mod policy;
mod stadium;
pub mod staff;

pub(crate) use club::CLUB_SUB_RECORDS;
pub(crate) use club::FIELDS as CLUB_FIELDS;
pub(crate) use player::FIELDS as PLAYER_FIELDS;
pub(crate) use player::PLAYER_SUB_RECORDS;
pub(crate) use stadium::FIELDS as STADIUM_FIELDS;
pub(crate) use staff::FIELDS as STAFF_FIELDS;
pub(crate) use staff::STAFF_SUB_RECORDS;

use crate::fields::VersionedSchemas;

/// FM27 Steam build. FM27 is a Unity build like FM26: game logic lives in
/// `game_plugin.dll`, and the entity RVAs below are relative to its base.
/// Values not listed as verified in docs/fm27/memory-layout.md are inherited
/// from FM26.
const STEAM: VersionedSchemas = VersionedSchemas {
    player_fields: PLAYER_FIELDS,
    staff_fields: STAFF_FIELDS,
    club_fields: CLUB_FIELDS,
    stadium_fields: STADIUM_FIELDS,

    player_sub_records: PLAYER_SUB_RECORDS,
    staff_sub_records: STAFF_SUB_RECORDS,
    club_sub_records: CLUB_SUB_RECORDS,
    stadium_sub_records: &[],
    injury_layout: None,
    player_lists: Some(&player::FM27_PLAYER_LISTS),
    morale_labels: player::MORALE_LABELS,
    person_profile: Some(&staff::PERSON_PROFILE),

    player_struct_size: 992, // 0x3E0 — covers block-2 name sub-records at 0x3D0/0x3D8
    staff_struct_size: 576,
    club_read_size: 0x2000,
    stadium_entity_stride: 0xC0, // 192 bytes, flat inline array (was 0x170 before the 2026-10 patch)

    human_manager_shift: staff::HUMAN_MANAGER_SHIFT,
    manager_shifted_fields: staff::MANAGER_SHIFTED_FIELDS,

    // FM27 keeps FM26's array cluster layout, shifted +0x8BA158 as a block.
    // Found by sampling elements of every heap vector in .sdata and decoding
    // names at FM26's offsets: players (p_name 0x2C8), staff (s_name 0x140),
    // clubs (c_full_name 0xC0 / c_name 0xC8) all decode. Stadium RVA is the
    // same shift applied, not yet checked.
    rva_humans_data: 0x57037D0,
    rva_humans_end: 0x57037D8,
    rva_staff_data: 0x5703818,
    rva_staff_end: 0x5703820,
    rva_clubs_data: 0x5703988,
    rva_clubs_end: 0x5703990,
    rva_stadium_base: 0x5703750,
    // Current game date (DateDayYear, flags above bit 8). Witnesses: raw
    // 0x07EA1AC2 (day 194) on Mon 13 Jul 2026; 195/2026 after one day of
    // holiday, Tue 14 Jul 2026 (2026-10-31). A pointer path reads the same
    // date: +0x56F1A18 -> game object (vtable +0x4C8C858) + 0x7F4.
    rva_current_date: 0x56AD378,

    // No FMRTE reference counts exist for FM27 yet. These are the counts
    // observed in a fresh FM27 save (day 1 of the first season); they
    // depend on the database size chosen at game creation.
    fmrte_players: 106868,
    fmrte_staff: 51466,
    fmrte_clubs: 33570,
    fmrte_humans: 1,

    // Not calibrated: attach reports the running game_plugin.dll hash.
    known_dll_hash: 0,
    // Full club finance class: Spurs, Arsenal, Man City, Liverpool and 881
    // other clubs. 32,682 clubs use a smaller class (vtable +0x4CF07F8) where
    // the budget offsets read packed dates and floats.
    full_finance_vtable_rva: 0x4CF0CC8,
    // Player-coaches (391 in this save): vtable +0x4F75A08, person block at
    // +0x288+0xF8. Witnesses: James Milner (born 4 Jan 1986, Leeds, England,
    // 61 caps), David Nugent, Keisuke Honda (FC Jurong), Remko Pasveer (Twente).
    player_coach_vtable_rva: 0x4F75A08,
    player_person_offset: 0x288,
    player_coach_shift: 0xF8,
    // Human-manager staff class: the user's manager (last staff entry) and the
    // entry FM creates for the player's Steam profile (named "(<account
    // name>)", staff id 2002106548, no club, CA/PA 100/200). Both showed as
    // nameless ID-0 rows in the staff list.
    human_manager_vtable_rva: 0x4CC086C,
    age_reference_year: 2027,
};

/// FM27 schema, Steam build (Windows and Proton).
pub static FM27: VersionedSchemas = STEAM;

/// FM27 schema, PC Game Pass build (Microsoft GDK, Xbox app install).
///
/// UNVERIFIED: a copy of the Steam build. The Game Pass binary is a separate
/// build, so its `game_plugin.dll` hash and module-relative RVAs (entity
/// arrays, stadium root, vtables) are expected to differ; struct field
/// offsets are likely, not known, to match. Bring-up checklist in
/// docs/fm27/memory-layout.md ("Game Pass build").
pub static FM27_GAMEPASS: VersionedSchemas = VersionedSchemas {
    // Not calibrated: attach reports the running game_plugin.dll hash.
    known_dll_hash: 0,
    // Game Pass RVAs differ: never inherit the Steam date address.
    rva_current_date: 0,
    ..STEAM
};
