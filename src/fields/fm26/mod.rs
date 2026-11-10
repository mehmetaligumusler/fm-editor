//! FM26 per-version field tables. Each sub-module owns one entity type's
//! `FIELDS` const slice; this module re-exports them under the
//! `PLAYER_FIELDS` / `STAFF_FIELDS` / `CLUB_FIELDS` names used by the
//! version-agnostic code in `fields/mod.rs`, and exposes the `FM26`
//! const that collects all of them into a `VersionedSchemas`.
//!
//! FM24 gets its own parallel module at `fields::fm24`.

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
pub(crate) use staff::STAFF_SUB_RECORDS;

use crate::fields::VersionedSchemas;

/// FM26 schema — every layout fact the editor needs to read fm.exe.
///
/// Struct sizes:
///   PLAYER_STRUCT_SIZE = 992 (0x3E0) — extended to cover block-2 name sub-records at 0x3D0/0x3D8
///   STAFF_STRUCT_SIZE  = 576 (0x240)
///   CLUB_READ_SIZE     = 0x2000 (clubs are 7456+ bytes; read a safe chunk)
///
/// RVAs for the FM26 entity pointer arrays, relative to game_plugin.dll base
/// (2026-10 patch; every RVA moved by +0x4F7F0 from the April build):
///
/// - `humans` (full player array): 0x4E49678 / 0x4E49680. The pair 0x18
///   bytes later points to a filtered subset (~496k entries); the full array
///   (std::vector begin/end/cap) gives the ~542k entries matching FMLE/FMRTE.
/// - `staff`: 0x4E496C0 / 0x4E496C8
/// - `clubs`: 0x4E49830 / 0x4E49838
/// - `stadiums`: 0x4E495F8 (5-hop chain, not a simple data/end pair)
///
/// Stadium access chain:
///   GP+0x4E495F8 → container → container+0x80 → type_desc → vec_hdr → data_ptr
///   entity[i] = data_ptr + i * 0xC0
///
/// NB: "human manager pointer" RVAs that appeared in early notes
/// (0x4DE7A08, 0x4DF9EB8/0x4DF9EC0) do NOT point to the human manager.
/// The actual human manager is the LAST entry of the ptr_staff array —
/// see `bridge::entity_load::load_human_manager`.
pub static FM26: VersionedSchemas = VersionedSchemas {
    player_fields: PLAYER_FIELDS,
    staff_fields: STAFF_FIELDS,
    club_fields: CLUB_FIELDS,
    stadium_fields: STADIUM_FIELDS,

    player_sub_records: PLAYER_SUB_RECORDS,
    staff_sub_records: STAFF_SUB_RECORDS,
    club_sub_records: CLUB_SUB_RECORDS,
    stadium_sub_records: &[],
    injury_layout: None,
    player_lists: None,
    morale_labels: &[],
    person_profile: None,

    player_struct_size: 992, // 0x3E0 — covers block-2 name sub-records at 0x3D0/0x3D8
    staff_struct_size: 576,
    club_read_size: 0x2000,
    stadium_entity_stride: 0xC0, // 192 bytes, flat inline array (was 0x170 before the 2026-10 patch)

    human_manager_shift: staff::HUMAN_MANAGER_SHIFT,
    manager_shifted_fields: staff::MANAGER_SHIFTED_FIELDS,

    // Shifted +0x4F7F0 by the 2026-10 FM26 patch (was 0x4DF9E88 etc.).
    rva_humans_data: 0x4E49678,
    rva_humans_end: 0x4E49680,
    rva_staff_data: 0x4E496C0,
    rva_staff_end: 0x4E496C8,
    rva_clubs_data: 0x4E49830,
    rva_clubs_end: 0x4E49838,
    rva_stadium_base: 0x4E495F8,
    rva_current_date: 0,

    // FMRTE's reference counts for FM26 (used by scan_arrays.rs to
    // validate RVA candidates).
    fmrte_players: 542102,
    fmrte_staff: 188865,
    fmrte_clubs: 113894,
    fmrte_humans: 1,

    // FNV-1a hash of the first 64 KB of game_plugin.dll for the FM26 build
    // this schema was verified against (2026-10 patch). Attach fails with a
    // version-mismatch error when the running DLL hashes differently.
    known_dll_hash: 0x3039_4AFE_C086_E3DC,
    full_finance_vtable_rva: 0,
    player_coach_vtable_rva: 0,
    player_person_offset: 0,
    player_coach_shift: 0,
    human_manager_vtable_rva: 0,
    age_reference_year: 2026,
};
