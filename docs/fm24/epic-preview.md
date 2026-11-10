# FM24 Epic: Windows build support

## Scope

This branch recognises Epic's `FootballManager24` directory, assigns a separate
`Fm24Epic` build and schema, and pins the observed executable fingerprint.
**Editing and Undo are enabled** since 2026-11-07, when attended live
write/Undo witnesses rendered correctly in game (below). Fields that were not
part of that verification remain Steam-inherited mappings — labelled honestly,
not independently witnessed on Epic.

## Observed build and evidence, 2026-11-06

- Windows Epic installation: `Epic Games/FootballManager24/fm.exe`.
- File version `24.4.2`, product version `24.4.2+2081827`.
- `.egstore`, `epic.txt`, and `EOSSDK-Win64-Shipping.dll` exist next to the exe.
- First-64-KiB project fingerprint: `0x8FA113DB9DCD8883`.
- Steam's fingerprint remains `0xFB39952A20287907`; neither gate is disabled.
- Module base observed `0x140000000`.

| Candidate array | Module RVA (data/end) | Initial count | First object's vtable RVA |
| --- | --- | --- | --- |
| Players | `0x642EDD0 / 0x642EDD8` | 75,726 | `0x5A4E958` |
| Staff | `0x642EDE8 / 0x642EDF0` | 38,258 | `0x5A4C2E8` |
| Clubs | `0x642EF58 / 0x642EF60` | 36,046 | `0x5A51F38` |

All three roots and vtable values agree with the Steam candidates. Counts can
change during gameplay. Cached ages inherit the existing FM24 season-year
approximation; they are not reliable current-save ages.

## Write/Undo verification witnesses, 2026-11-07

Attended session on a designated backup career ("Yedek Save"), user watching
every value on the in-game screens. Disk saves were copied to a backup folder
before the session and nothing was saved in game while an edit was live.
Every write was compare-and-swap through `epic_edit_test` (below): re-read
before writing, abort on drift, read-back after.

**Player attributes — direct encoded byte in the player struct:**

- Tanju Başar (uid 2002179387), youth player: nine attributes read matched
  the in-game profile exactly — Crossing 6, Dribbling 7, Finishing 7,
  Heading 4, Long Shots 8, Passing 13, Tackling 7, Technique 15,
  First Touch 15. Finishing was then written 7 → 15 (raw `0x22` → `0x49`),
  the squad-list attribute column rendered 15 live, the value was restored
  and the column reverted to 7.
- Kerim Demir (uid 2002179385): Passing 6, Technique 4, Tackling 3 matched
  the in-game profile.

**Club transfer budget — `finance+0x7CC` 4-byte i32 (internal £):**

- Antalyaspor (uid 1865): read 4,100,645, rendered as 4,782,290 € on the
  finance screen (display currency €; conversion ≈ 1.16626). Written to
  5,100,645; the finance screen updated live to 5,948,520 € without leaving
  the page; restored and the screen reverted. No other finance figure moved.

**Current in-game date mapped, 2026-11-08:** the packed DateDayYear pair for
the user's on-screen date (21 June 2026, Sunday — day-of-year 172) was scanned
for read-only across the fm.exe image. Exactly three stable copies exist;
`module+0x642F458` sits inside the globals cluster just past the entity-array
roots and is wired as `rva_current_date` for `FM24_EPIC`, so profile and
search ages now use the live game date instead of the 2023 reference-year
estimate (the user's career is ~3 seasons in: Hasan Yakub İlçin shows 17 in
game, 14 with the old estimate). The other two copies (`0x631D5BC`,
`0x6356C90`, in string/buffer regions) were not chosen. Re-verify this RVA
after any Epic game update, alongside the fingerprint.

**UI behaviour noted during the session:** FM's detailed player profile panel
caches attribute values for the session — an edited attribute keeps showing
the old number there until the panel is rebuilt. Squad-list attribute columns
and the finance screen re-read live and are the reliable in-game check. The
memory values themselves are authoritative.

## Attended write/Undo verification procedure

The `epic_edit_test` diagnostic runs the verification against a designated
TEST save while the user watches every value in game. The write commands
refuse to run against any build other than `Fm24Epic`.

1. Copy the save files to a backup folder before starting.
2. Load the TEST save in game and wait until it is fully loaded.
3. `cargo run --bin epic_edit_test -- find <name>` resolves entity names to
   UIDs (read-only).
4. `attr show <uid> <key>` prints the current raw and displayed value; the
   user confirms it matches the in-game screen before anything is written.
5. `attr set <uid> <key> <value>` compare-and-swap writes: memory is re-read
   immediately before the write, a changed value aborts, and the written
   bytes are read back afterwards. The user confirms the new value renders
   in game.
6. `attr restore <uid> <key> <old>` writes the previous value back through
   the same guarded path; the user confirms restoration.
7. Never save in game while an edit is unverified or unrestored.

`budget show/set/restore <club-uid>` runs the same round trip for the club
transfer budget through the finance sub-record (4-byte i32, mirroring the
editor's own club paths — the schema's `sub_raw` label for this key is
legacy).

## Remaining caveats

- Staff fields, morale, traits, weight, names and contract fields on Epic are
  inherited from the Steam FM24 schema; only the paths above carry Epic-side
  witnesses. Treat unexpected values as a layout regression before writing.
- The detailed player profile panel caches per session (above); verify edits
  on screens that re-read live.
- Recheck the module fingerprint and array roots after any Epic game update.
