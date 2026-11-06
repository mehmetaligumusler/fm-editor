# FM24 Memory Layout

The separate [Epic read-only preview](epic-preview.md) records Windows Epic
build detection, array-root observations, fingerprint and verification limits.
The Steam field witnesses below must not be treated as Epic verification.

Mapped 2026-10-22 against a fresh FM24 save (day 0, 3 Jul 2023) under Proton.
The code is the source of truth (`src/fields/fm24/`); this file holds the
discovery context, witnesses and what is still unmapped.

## Module

FM24 has **no `game_plugin.dll`**. Game logic and every entity-array RVA live
in `fm.exe` itself (image base 0x140000000), so `gp_base` is the fm.exe base on
this version (`GameVersion::module_name`). The exe is wrapped (non-standard
sections `.arch`/`.xtls`/`.sxdata`, a 360 MB RWX `.arch`), so static analysis is
not useful; everything below came from live memory.

Under Proton FM24's `/proc/<pid>/comm` is `Main Thread`, not `fm.exe`; the
Linux backend also matches the basename of argv[0] (`S:\…\fm.exe`).

Version gate: `known_dll_hash` = hash of the first 64 KB of `fm.exe`.

## Entity arrays (RVA relative to fm.exe)

| Entity | Count | data / end | Object vtable | Stride |
|--------|-------|------------|---------------|--------|
| Players | 83,426 | 0x642EDD0 / 0x642EDD8 | fm.exe+0x5A4E958 | 0x3E8 |
| Staff | 33,835 | 0x642EDE8 / 0x642EDF0 | fm.exe+0x5A4C2E8 | 0x268 |
| Clubs | 36,725 | 0x642EF58 / 0x642EF60 | fm.exe+0x5A51F38 | 0x170 spacing |

Found by scanning fm.exe's writable sections for `(data, end)` pointer pairs,
then locating witness records in the heap (Guardiola by his personality bytes,
Kane by his UID) and finding which arrays point at them.

- 0x642EDB8 (116,856) points 0x28 into a different object type; not players.
- 0x642EDF0 (24,775) is a subset of the staff array.
- 0x642EF88 (36,498) is a strict subset of the club array.
- 99.8% of player-array entries carry the player vtable.

The stadium array root is not located; stadiums are reached per club through
the `home_stadium` sub-record, so `rva_stadium_base` is 0.

## Player (vs FM26)

- Positions + attributes keep the FM26 order shifted **+0xB8**: positions
  0x208..0x216, attributes 0x217..0x24C.
- Individually moved: home/current/world rep 0x1FA/0x1FC/0x1FE, CA/PA
  0x200/0x202, height 0x14E, UID 0x284, name FmString 0x2C0, name subs
  0x2D0/0x2D8, DOB 0x2BC, personality 0x2F0..0x2F7, club ptr 0x3B0.
- Names: 40% direct FmString, 59% first/last sub-records, 0.5% neither.
- Added 2026-10-30 from FM27€0's FM24 table (leads), each confirmed on Harry
  Kane, Lionel Messi, Cristiano Ronaldo, Kevin De Bruyne and Joe Ward against
  their real-world records:
  - International record, four u8 at 0x3DC..0x3DF: caps, U21 caps, goals,
    U21 goals (Kane 84/14/58/8, Messi 175/18/103/14, Ronaldo 200/10/123/3,
    De Bruyne 99/2/26/0). FM27 widens caps and goals to u16.
  - Nation pointer 0x2E8 (name FmString at nation+0x18): England, Argentina,
    Portugal, Belgium, England.
  - Birth city pointer 0x300 (name at city+0x18): London, Rosario (SF),
    Funchal, Ghent, Chelmsford.
  - Team pointer 0x130, club at team+0x30: FC Bayern, Inter Miami, Al-Nassr,
    Man City, Derby.
  - Contract pointer 0x340: wage i32 £/week +0x18, expiry +0x40, squad status
    +0x4C, squad number +0x55. Expiry and number match real contracts (Kane
    30 Jun 2027 #9, Messi 31 Dec 2025 #10, Ronaldo 30 Jun 2025 #7, De Bruyne
    30 Jun 2025 #17); squad status 1 (Star Player) for the four stars. Squad
    number edited and undone in game on Cristian Romero (#17, Tottenham).
    Wage values (Kane 361,779, Romero 175,000) are not yet checked in game.
- Weight u16 kg **0x14C** (mapped 2026-11-01): Kane 86, Udogie 73 and
  Maddison 73 match FM's profile; edits Maddison 73 -> 80 and Kulusevski
  79 -> 84 showed 80 kg / 84 kg in FM, and Undo restored memory.
- Traits (PPMs) u64 bitmask **0x368**, bit = PPM ID - 1 as on FM26/FM27
  (mapped 2026-11-01). Kane 0x00000A0000080080 = Tries Killer Balls Often,
  Comes Deep To Get Ball, Gets Crowd Going, Tries Long Range Passes, exactly
  FM's list. Maddison 0x0000200000080100 = Shoots From Distance, Comes Deep To
  Get Ball (both in FM's list) and Hits Free Kicks With Power (not checked).
  Adding Shoots From Distance to Udogie (0x2000000000, Dives Into Tackles)
  showed both traits in FM; Undo restored memory.
- Morale level u8 1..20 at **0x25F** (mapped 2026-11-01), the FM26/FM27
  encoding one byte after the old 0x25E lead (0x25E reads 104..185 and is
  something else). All 24 Brighton players' bytes match the squad Dynamics
  view: 8 Fair, 10 Okay, 11 Fairly Good, 12 Quite Good, 13 Good, 14 Really
  Good, 15 Very Good, 16 Extremely Good, 17 Excellent, 18 Superb, 19
  Exceptional (levels 8 and 10..14 match FM27's names). Setting Jason Steele
  17 -> 10 showed "Okay" in FM's squad view at once; Undo showed "Excellent".
- Sharpness lead u16 **0x1F4** (0..10000), still unmapped because no build has
  a condition UI yet. Over Brighton's 24 players FM's squad sharpness icon
  steps with it: red "Lacking Sharpness" up to 5850 (3500 Moder/Undav/Dahoud,
  5850 Groß), orange "Maintaining Sharpness" at 6100 (Milner), yellow at 7400
  (Scherpen). FM shows only these bands, so no exact value can be read off
  screen. Kane's profile header ("Severely lacking in match sharpness" at
  5900) and Maddison's ("Needs match practice" at 5150) look like advice text,
  not the band.
- Other leads read but not confirmed in game: condition u16 0x1F8 (10000 for
  fit players; injured Moder 9321, Scherpen 9325), jadedness i16 0x1F6 (-125
  for most players, Scherpen 0), asking price i32 0x1D4 (-1 = none), looks
  0x309..0x30C. Cristian Romero (Tottenham) is consistent with FM's condition
  popup: condition 10000 "Peak", sharpness 5100 "Needs match practice",
  jadedness -125 "Fresh" (one witness).

## Staff (vs FM26)

- FM24 names two attributes differently from FM26/FM27: the coaching
  attribute at 0x3D is "Mental" (FM26: Possession) and the mental attribute at
  0x1C is "Level of Discipline" (FM26: Authority). Mile Jedinak's FM24 profile
  (2026-10-30) shows Mental 12 and Level of Discipline 14, matching 0x3D = 60
  (encoded) and 0x1C = 14.
- Attribute block (< 0x50) is FM26 **+8**; 0x10..0x17 is a pointer. Judging
  Staff Ability 0x4A, Analysing Data 0x44 and Sports Science 0x47 (encoded,
  displayed = (raw + 2) / 5) were mapped 2026-11-01 against FM24's staff
  profile tabs: Jedinak 9 / 5 / 3 (raw 46 / 25 / 15), Guardiola 15 / 7 / 3
  (74 / 35 / 15). Setting Ryan Mason's Judging Staff Ability 10 -> 16 and
  Analysing Data 4 -> 12 showed 16 and 12 in FM; Undo restored memory. The
  earlier Guardiola "disagreement" did not reproduce on FM24's own screens.
- Hidden attributes FM does not display stay unmapped, with FM26+8 leads
  (Jedinak / Guardiola): versatility 0x43 = 55 / 15, business 0x19 = 10 / 14,
  interference 0x1E = 12 / 17, resources 0x23 = 8 / 1.
- Reps 0xD0/0xD2/0xD4, CA/PA 0xD6/0xD8 (4 earlier than FM26), staff ID 0x104,
  name 0x140, name subs 0x150/0x158, DOB 0x13C, personality 0x170 (unchanged),
  role-ratings ptr 0x1F8.

### Human manager

Last entry of the staff array (vtable fm.exe+0x5A6A318). Identity fields live
at `offset + 0x358` (FM26: 0x350): IDs, DOB, name subs, personality, role ptr.
Unlike FM26 the direct name FmString is null in both slots — the name comes
from the sub-records. Verified on the test save's human manager (role ratings Manager 20).

## Club (vs FM26)

- Top level identical: UID 0x0C, full/short name 0xC0/0xC8, index 0x08.
- Info sub-record (ptr +0x100) is FM26 **+8**: abbreviations 0x00/0x08/0x10,
  season tickets 0xC0, founded 0xD8, owner status 0xDE, social 0xF8,
  supporter % 0x106..0x10B, supporter attributes 0x114..0x119, facilities
  0x120..0x12D. Training is **0x120** and youth facilities **0x12B** (swapped
  until 2026-10-30): Arsenal reads 19 / 16 and FM24's Club Info shows "Superb
  training facilities" / "Excellent youth facilities", with junior coaching
  0x12C = 17 "Excellent" and youth recruitment 0x12D = 15 "Good" on the same
  screen. FM27 uses the same offsets.
- Finance (+0x150), budget (finance+0x48) and attendance (stadium-sub +0xB0,
  0x68/0x6C/0x70) unchanged. The wage budget is **finance+0x810** (i32 £/week),
  not budget+0x0C: Spurs' finance screen shows Wage Budget £3,421,987 p/w
  (+0x810), currently spending £3,271,987 (+0x81C) and next season's minimum
  guaranteed transfer budget £25.92M (+0x7D4 = 25,925,482). budget+0x0C read
  3,464,579 for Spurs and equals +0x810 for Arsenal, Barcelona and Derby.
  Transfer budget is **finance+0x7CC** (mapped 2026-11-01); budget+0x04 holds
  a copy that FM's finance screen does not read. With only finance+0x7CC
  changed, Spurs showed £15.7M / £8.0M and Brighton (second human manager,
  same save) £72.0M; Undo restored £5.7M and £69.5M on screen. Brighton's
  wage budget changed when De Zerbi left (finance+0x810 1,712,351, FM
  £1,712,350 p/w) while budget+0x0C stayed 1,731,413, so the budget
  sub-record copies go stale. Transfer remaining stays at budget+0x08 (FM24
  shows no separate figure to check it against). The average ticket price
  float is at both finance+0x18 (mapped) and +0x870.
- Read-only finance figures (mapped 2026-11-01): **Currently Spending**
  finance+0x81C and **Next Season Min Transfer Budget** finance+0x7D4. Spurs
  £3,271,987 / £25.92M and Brighton £1,543,870 / £34.75M on FM's Finances
  screen; memory 3,271,987 / 25,925,482 and 1,543,871 / 34,750,000 (Brighton's
  wage figures read £1 above FM's display). Shown, not edited: FM recomputes them.
- Home stadium: club+0x18 → [0] senior team → +0x78, same chain as FM26.
- Not mapped: gender (+0x161 holds 0/2/4/8/10/32, not FM26's 0/1), nickname
  (stadium-sub +0x90 is a short name here), team UID, training ground,
  stand types, reputation, nation.

## Stadium entity (stride 0xB0)

UID 0x0C, name 0x38, alt name 0x40, capacity/seated/used/all-seater/expansion
u32 0x6C/0x70/0x74/0x78/0x7C, owner club ptr 0x80, pitch length/width ×10
0x88/0x8A. Standing, environment, condition unmapped.

## Witnesses

| Entity | Identifier | Confirmed |
|--------|-----------|-----------|
| Harry Kane | Player UID 28049320 | name, DOB 209/1993, height 188, CA/PA 184/185, reps 9250/9200/8750, positions (ST 20, AMC 15, MC/AML 12, AMR 10), attributes, club → Tottenham |
| Josep Guardiola | Staff ID 1120 | name, DOB 18/1971, CA/PA 190/200, reps 9500, personality, roles, 31/38 attributes |
| The human manager (test save) | Human manager | shifted identity block, roles |
| FC Bayern | Club UID 915 | names, abbreviations, founded 1900, facilities, supporters, attendance |
| Tottenham | Club UID 728 | as Bayern; ticket prices 65 / 1296 |
| Allianz-Arena, Tottenham Hotspur Stadium | Stadium | capacity, expansion, owner, pitch |

Sample checks (1500 players): GK position vs handling r=0.93, handling/reflexes
r=0.89, DL/WBL r=0.82, attribute bytes ≤ 100, max(left, right foot) = 20 for all.
1500 staff: CA ≤ PA, attribute bytes ≤ 100, personality 1..20, plausible DOBs.

## Unmapped worklist

Player: value, condition (sharpness band confirmed, see above), apps/goals,
dates (transfer/join/debut/listing), secondary name block, contract clauses and
listing. Staff: the four hidden attributes above. Club and stadium: see above.
