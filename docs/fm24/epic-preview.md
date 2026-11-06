# FM24 Epic: read-only Windows research preview

## Scope

This branch recognises Epic's `FootballManager24` directory, assigns a separate
`Fm24Epic` build and schema, and pins the observed executable fingerprint.
It does **not** claim verified Epic editing support. The inherited Steam field
offsets are read-only research candidates, not independently confirmed mappings.
The UI labels all Epic readings experimental and disables editing and Undo.
`FmProcess::write_bytes` rejects every Epic write. On Windows the Epic process
handle has VM_READ and QUERY_LIMITED_INFORMATION only, so the OS also refuses
writes. The only bypass is the `epic-write-test` cargo feature below, which
exists solely for the attended verification diagnostic and must never ship.

The user's live career is not a test save. Do not use the repository's historical
Linux test-save assumptions on this computer. The user is continuing gameplay:
do not drive the game UI, advance time, make test edits, change game files or
write/save the career. A later, separate test copy and permission are required
for in-game attribute comparisons and edit/Undo tests.

## Observed build and evidence, 2026-11-06

- Windows Epic installation: `Epic Games/FootballManager24/fm.exe`.
- File version `24.4.2`, product version `24.4.2+2081827`.
- `.egstore`, `epic.txt`, and `EOSSDK-Win64-Shipping.dll` exist next to the exe.
- First-64-KiB project fingerprint: `0x8FA113DB9DCD8883`.
- Steam's fingerprint remains `0xFB39952A20287907`; neither gate is disabled.
- Module base observed `0x140000000`.
- Live reads used a read-only Windows handle; no memory writes were made.

| Candidate array | Module RVA (data/end) | Initial count | First object's vtable RVA |
| --- | --- | --- | --- |
| Players | `0x642EDD0 / 0x642EDD8` | 75,726 | `0x5A4E958` |
| Staff | `0x642EDE8 / 0x642EDF0` | 38,258 | `0x5A4C2E8` |
| Clubs | `0x642EF58 / 0x642EF60` | 36,046 | `0x5A51F38` |

All three roots and vtable values agree with the Steam candidates, but that does
not prove the validity of every entity field or write operation. Counts can
change during gameplay. Cached ages inherit the existing FM24 season-year
approximation; they are not reliable current-save ages.

## Attended write/Undo verification procedure

The `epic-write-test` cargo feature plus the `epic_edit_test` diagnostic run
the verification against a designated TEST save while the user watches every
value in game. The write commands refuse to run unless the attached build is
Fm24Epic and the feature lifted the lock.

1. Copy the save files to a backup folder before starting.
2. Load the TEST save in game and wait until it is fully loaded.
3. `cargo run --features epic-write-test --bin epic_edit_test -- find <name>`
   resolves entity names to UIDs (read-only).
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
transfer budget through the finance sub-record. Record the witnesses
(entity, field, before/after values, date) in this file before lifting the
default write lock for the build.

## Verification still required before editing support

For each enabled field, compare two independently known in-game entities and
record their UIDs, values, representation and date. On a separate test save,
verify a controlled edit through the acknowledged bridge and Undo, including
the value rendered in FM. Leave unsupported fields unavailable. Recheck the
module fingerprint and array roots after any Epic game update.

Build and runtime results are recorded separately in the delivered notes; a
successful attach alone does not establish field correctness.
