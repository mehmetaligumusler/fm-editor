//! Staff field definitions for FM24. Aggregated via `fields::fm24::STAFF_FIELDS`.
//!
//! Mapped 2026-10-22 against a fresh FM24 save. Staff objects are 0x268 bytes
//! (pool stride), vtable fm.exe+0x5A4C2E8. The attribute block (< 0x50) is the
//! FM26 layout shifted by +8; reputation/CA/PA sit 4 bytes earlier than FM26;
//! personality (+0x170) is unchanged.
//!
//! Evidence: Josep Guardiola Sala (ID 1120, DOB day 18/1971, CA 190 / PA 200,
//! reps 9500, personality [20,20,15,18,20,16,14,8], roles Manager 20 / Asst 12 /
//! Coach 12 via role ptr) — 31 of 38 attribute witnesses match at FM26+8.
//! Judge staff, data analysis and sports science were added 2026-11-01 at
//! FM26+8 against FM24's own profile screens (Jedinak and Guardiola); the four
//! hidden ones FM does not display stay unmapped. A 1500-staff sample has CA <= PA,
//! attribute bytes <= 100, personality 1..20 and plausible DOBs for 100%.

use crate::fields::Category::*;
use crate::fields::Entity::Staff;
use crate::fields::{
    Field, date, direct, encoded, fm_string, pointer, raw, raw_capped, role_index,
};

pub(crate) const FIELDS: &[Field] = &[
    Field {
        key: "s_goalkeeping",
        label: "GK Coaching",
        entity: Staff,
        category: Coaching,
        offset: encoded(0x033),
    },
    Field {
        key: "s_attacking",
        label: "Attacking",
        entity: Staff,
        category: Coaching,
        offset: encoded(0x03A),
    },
    Field {
        key: "s_defending",
        label: "Defending",
        entity: Staff,
        category: Coaching,
        offset: encoded(0x03B),
    },
    Field {
        key: "s_fitness",
        label: "Fitness Coaching",
        entity: Staff,
        category: Coaching,
        offset: encoded(0x03C),
    },
    Field {
        key: "s_possession",
        label: "Mental",
        entity: Staff,
        category: Coaching,
        offset: encoded(0x03D),
    },
    Field {
        key: "s_tactical",
        label: "Tactical",
        entity: Staff,
        category: Coaching,
        offset: encoded(0x03F),
    },
    Field {
        key: "s_technical",
        label: "Technical",
        entity: Staff,
        category: Coaching,
        offset: encoded(0x03E),
    },
    Field {
        key: "s_set_pieces",
        label: "Set Pieces",
        entity: Staff,
        category: Coaching,
        offset: encoded(0x04B),
    },
    Field {
        key: "s_youngsters",
        label: "Youngsters",
        entity: Staff,
        category: Coaching,
        offset: direct(0x024),
    },
    Field {
        key: "s_authority",
        label: "Level of Discipline",
        entity: Staff,
        category: Mental,
        offset: direct(0x01C),
    },
    Field {
        key: "s_determination",
        label: "Determination",
        entity: Staff,
        category: Mental,
        offset: encoded(0x025),
    },
    Field {
        key: "s_motivating",
        label: "Motivation",
        entity: Staff,
        category: Mental,
        offset: encoded(0x037),
    },
    Field {
        key: "s_people_mgmt",
        label: "People Mgmt",
        entity: Staff,
        category: Mental,
        offset: encoded(0x036),
    },
    Field {
        key: "s_judge_ability",
        label: "Judge Ability",
        entity: Staff,
        category: Knowledge,
        offset: encoded(0x034),
    },
    Field {
        key: "s_judge_potential",
        label: "Judge Potential",
        entity: Staff,
        category: Knowledge,
        offset: encoded(0x035),
    },
    Field {
        key: "s_negotiating",
        label: "Negotiating",
        entity: Staff,
        category: Knowledge,
        offset: encoded(0x049),
    },
    Field {
        key: "s_tactical_knowl",
        label: "Tactical Knowledge",
        entity: Staff,
        category: Knowledge,
        offset: encoded(0x039),
    },
    Field {
        key: "s_physiotherapy",
        label: "Physiotherapy",
        entity: Staff,
        category: Medical,
        offset: encoded(0x038),
    },
    Field {
        key: "s_ts_attacking",
        label: "Attacking",
        entity: Staff,
        category: TacticalStyle,
        offset: direct(0x018),
    },
    Field {
        key: "s_def_line",
        label: "Defensive Line",
        entity: Staff,
        category: TacticalStyle,
        offset: direct(0x02B),
    },
    Field {
        key: "s_directness",
        label: "Directness",
        entity: Staff,
        category: TacticalStyle,
        offset: direct(0x01B),
    },
    Field {
        key: "s_flexibility",
        label: "Flexibility",
        entity: Staff,
        category: TacticalStyle,
        offset: direct(0x02D),
    },
    Field {
        key: "s_tempo",
        label: "Tempo",
        entity: Staff,
        category: TacticalStyle,
        offset: direct(0x030),
    },
    Field {
        key: "s_width",
        label: "Width",
        entity: Staff,
        category: TacticalStyle,
        offset: direct(0x031),
    },
    Field {
        key: "s_trigger_press",
        label: "Trigger Press",
        entity: Staff,
        category: TacticalStyle,
        offset: direct(0x022),
    },
    Field {
        key: "s_rotation",
        label: "Squad Rotation",
        entity: Staff,
        category: TacticalStyle,
        offset: direct(0x02F),
    },
    Field {
        key: "s_hardness",
        label: "Train Hardness",
        entity: Staff,
        category: TacticalStyle,
        offset: direct(0x02E),
    },
    Field {
        key: "s_dirtiness",
        label: "Dirtiness",
        entity: Staff,
        category: TacticalStyle,
        offset: encoded(0x040),
    },
    Field {
        key: "s_buying_players",
        label: "Buying Players",
        entity: Staff,
        category: BoardRelations,
        offset: direct(0x026),
    },
    Field {
        key: "s_mind_games",
        label: "Mind Games",
        entity: Staff,
        category: BoardRelations,
        offset: direct(0x027),
    },
    Field {
        key: "s_patience",
        label: "Patience",
        entity: Staff,
        category: BoardRelations,
        offset: direct(0x021),
    },
    Field {
        key: "s_adaptability",
        label: "Adaptability",
        entity: Staff,
        category: Personality,
        offset: direct(0x170),
    },
    Field {
        key: "s_ambition",
        label: "Ambition",
        entity: Staff,
        category: Personality,
        offset: direct(0x171),
    },
    Field {
        key: "s_loyalty",
        label: "Loyalty",
        entity: Staff,
        category: Personality,
        offset: direct(0x172),
    },
    Field {
        key: "s_pressure",
        label: "Pressure",
        entity: Staff,
        category: Personality,
        offset: direct(0x173),
    },
    Field {
        key: "s_professionalism",
        label: "Professionalism",
        entity: Staff,
        category: Personality,
        offset: direct(0x174),
    },
    Field {
        key: "s_sportsmanship",
        label: "Sportsmanship",
        entity: Staff,
        category: Personality,
        offset: direct(0x175),
    },
    Field {
        key: "s_temperament",
        label: "Temperament",
        entity: Staff,
        category: Personality,
        offset: direct(0x176),
    },
    Field {
        key: "s_controversy",
        label: "Controversy",
        entity: Staff,
        category: Personality,
        offset: direct(0x177),
    },
    Field {
        key: "s_name",
        label: "Name",
        entity: Staff,
        category: General,
        offset: fm_string(0x140),
    },
    Field {
        key: "s_name_first_sub",
        label: "Name First Sub",
        entity: Staff,
        category: Internal,
        offset: pointer(0x150),
    },
    Field {
        key: "s_name_last_sub",
        label: "Name Last Sub",
        entity: Staff,
        category: Internal,
        offset: pointer(0x158),
    },
    Field {
        key: "s_ca",
        label: "Current Ability",
        entity: Staff,
        category: Ability,
        offset: raw_capped(0x0D6, 200),
    },
    Field {
        key: "s_pa",
        label: "Potential Ability",
        entity: Staff,
        category: Ability,
        offset: raw_capped(0x0D8, 200),
    },
    Field {
        key: "s_id",
        label: "Staff ID",
        entity: Staff,
        category: General,
        offset: raw(0x104),
    },
    Field {
        key: "s_dob",
        label: "Date of Birth",
        entity: Staff,
        category: General,
        offset: date(0x13C),
    },
    Field {
        key: "s_role_ptr",
        label: "Role Ratings",
        entity: Staff,
        category: General,
        offset: pointer(0x1F8),
    },
    Field {
        key: "s_role_manager",
        label: "Manager",
        entity: Staff,
        category: StaffRole,
        offset: role_index(0),
    },
    Field {
        key: "s_role_asst_mgr",
        label: "Asst Manager",
        entity: Staff,
        category: StaffRole,
        offset: role_index(1),
    },
    Field {
        key: "s_role_coach",
        label: "Coach",
        entity: Staff,
        category: StaffRole,
        offset: role_index(2),
    },
    Field {
        key: "s_role_gk_coach",
        label: "GK Coach",
        entity: Staff,
        category: StaffRole,
        offset: role_index(5),
    },
    Field {
        key: "s_role_fit_coach",
        label: "Fitness Coach",
        entity: Staff,
        category: StaffRole,
        offset: role_index(6),
    },
    Field {
        key: "s_role_sp_coach",
        label: "Set Piece Coach",
        entity: Staff,
        category: StaffRole,
        offset: role_index(7),
    },
    Field {
        key: "s_role_scout",
        label: "Scout",
        entity: Staff,
        category: StaffRole,
        offset: role_index(4),
    },
    Field {
        key: "s_role_youth_dev",
        label: "Head of Youth Dev",
        entity: Staff,
        category: StaffRole,
        offset: role_index(10),
    },
    Field {
        key: "s_role_data",
        label: "Data Analyst",
        entity: Staff,
        category: StaffRole,
        offset: role_index(12),
    },
    Field {
        key: "s_role_loan",
        label: "Loan Manager",
        entity: Staff,
        category: StaffRole,
        offset: role_index(11),
    },
    Field {
        key: "s_role_dof",
        label: "Dir of Football",
        entity: Staff,
        category: StaffRole,
        offset: role_index(9),
    },
    Field {
        key: "s_role_physio",
        label: "Physio",
        entity: Staff,
        category: StaffRole,
        offset: role_index(3),
    },
    Field {
        key: "s_role_sports_sci",
        label: "Sports Scientist",
        entity: Staff,
        category: StaffRole,
        offset: role_index(13),
    },
    Field {
        key: "s_role_tech_dir",
        label: "Technical Director",
        entity: Staff,
        category: StaffRole,
        offset: role_index(14),
    },
    Field {
        key: "s_home_rep",
        label: "Home Rep",
        entity: Staff,
        category: Reputation,
        offset: raw_capped(0x0D0, 10000),
    },
    Field {
        key: "s_current_rep",
        label: "Current Rep",
        entity: Staff,
        category: Reputation,
        offset: raw_capped(0x0D2, 10000),
    },
    Field {
        key: "s_world_rep",
        label: "World Rep",
        entity: Staff,
        category: Reputation,
        offset: raw_capped(0x0D4, 10000),
    },
    // FM26+8, confirmed on FM24's staff profile (2026-11-01). Jedinak: Judging
    // Staff Ability 9 (0x4A = 46), Analysing Data 5 (0x44 = 25), Sports
    // Science 3 (0x47 = 15). Guardiola: 15 (74), 7 (35), 3 (15).
    Field {
        key: "s_judge_staff",
        label: "Judge Staff",
        entity: Staff,
        category: Knowledge,
        offset: encoded(0x04A),
    },
    Field {
        key: "s_data_analysis",
        label: "Data Analysis",
        entity: Staff,
        category: Knowledge,
        offset: encoded(0x044),
    },
    Field {
        key: "s_sports_science",
        label: "Sports Science",
        entity: Staff,
        category: Medical,
        offset: encoded(0x047),
    },
    // Hidden attributes FM24 does not display; FM26+8 leads read Jedinak /
    // Guardiola versatility 0x43 = 55 / 15, business 0x19 = 10 / 14,
    // interference 0x1E = 12 / 17, resources 0x23 = 8 / 1. Unverified.
    Field {
        key: "s_versatility",
        label: "Versatility",
        entity: Staff,
        category: TacticalStyle,
        offset: None,
    },
    Field {
        key: "s_business",
        label: "Business",
        entity: Staff,
        category: BoardRelations,
        offset: None,
    },
    Field {
        key: "s_interference",
        label: "Interference",
        entity: Staff,
        category: BoardRelations,
        offset: None,
    },
    Field {
        key: "s_resources",
        label: "Resources",
        entity: Staff,
        category: BoardRelations,
        offset: None,
    },
    Field {
        key: "s_weekly_wage",
        label: "Weekly Wage",
        entity: Staff,
        category: Contract,
        offset: None,
    },
    Field {
        key: "s_contract_expiry",
        label: "Contract Expiry",
        entity: Staff,
        category: Contract,
        offset: None,
    },
    Field {
        key: "s_nationality",
        label: "Nationality",
        entity: Staff,
        category: General,
        offset: None,
    },
    Field {
        key: "s_club",
        label: "Club",
        entity: Staff,
        category: General,
        offset: None,
    },
];

/// Byte offset added to a standard staff field offset to reach the overlaid
/// identity block inside the FM24 human-manager struct (FM26 uses 0x350).
pub const HUMAN_MANAGER_SHIFT: usize = 0x358;

/// Staff field offset ranges that live at `offset + HUMAN_MANAGER_SHIFT` on the
/// FM24 human manager (verified on a test save's human manager). Unlike FM26 the direct
/// name FmString is null in both slots; the name comes from the sub-records.
pub const MANAGER_SHIFTED_FIELDS: &[(usize, usize)] = &[
    (0x104, 4),  // staff id
    (0x108, 4),  // staff id duplicate
    (0x13C, 4),  // DOB day (u16) + year (u16)
    (0x150, 16), // first/last name sub-record pointers
    (0x170, 8),  // personality (8 x u8)
    (0x1F8, 8),  // role sub-record pointer
];
