# Changelog

## Unreleased

### Features

- FM24 Epic (Windows): **read-only preview** for the Epic Games Store build (product version `24.4.2+2081827`). Epic installs are recognised by their `.egstore`/`epic.txt` markers or the default install path, attach with an OS read-only handle (`VM_READ` only) and a pinned module fingerprint, and browse players, staff and clubs using the inherited Steam field offsets. Editing and Undo stay disabled at both the UI and `write_bytes` boundary until Epic field mappings are witnessed in game on a test save; see `docs/fm24/epic-preview.md`.

## 0.4.0 (prerelease)

### Features

- FM24 Steam: editable player **Weight** and **Traits (PPMs)**, verified in game on Kane, Maddison, Kulusevski and Udogie.
- FM24 Steam: editable player **Morale** (numeric 1–20), matched against all 24 Brighton players and written/undone live.
- FM24 Steam: staff **Judge Staff**, **Data Analysis** and **Sports Science** are now mapped and editable, verified on Jedinak, Guardiola and Ryan Mason.
- FM24 Steam: clubs show read-only **Wage Spend** (FM's "Currently spending", p/w) and **Next Min Budget** (next season's minimum guaranteed transfer budget).
- Add read-only two-player comparison: **Select for compare** on one profile, then **Compare with selected** on another. Named columns stay visible while scrolling, with coloured attributes, grouped sections and available age, CA/PA, wage and expiry context. Snapshots clear on save reload/reconnection. No season-statistics mappings or memory writes are added.
- FM27 Steam: staff **Role ratings** are shown again (read-only), checked against every employed staff member's job and FM's Preferred Jobs icons.
- FM27 Steam: staff profiles show **Languages**, **Coaching Licence** and preferred in/out-of-possession **Formations** (read-only); player profiles show **Languages**, in FM's order and level words.
- FM27 Steam: edit player and staff **first, last and common names** by picking an existing name from a search (names are shared records; the editor re-points the person and never changes a name's text). Verified in game, through Undo, and across Save As and reload.
- FM27 Steam: player **Asking Price** read and write verified in game (-1 = Unspecified).
- Player Overview: **Show potential attributes** previews estimated values read-only in the normal attribute categories, replacing the separate Positions & Traits projection list. Actual attributes and editing return when switched off; other Overview fields remain editable.
- Add structured **Change history** with entity/field identity, before/after values, UTC submission time, labelled write/Undo outcomes, filters and expandable per-write attempts. Undone/failed actions and audit-only closed sessions remain visible in memory; old-session write/navigation authority is invalidated.
- Treat facility mirror writes and one date-picker interaction as logical Undo actions. Track acknowledgements per physical write, report partial success and retry only the remaining confirmed Undo portions; keep unrelated controls separate.

### Fixes

- Keep player and manager overviews compact: shallow identity headers and independently stacked personal-detail/attribute columns bring attributes into view without shrinking text. Reflow columns on narrow windows; constrain long read-only values beside their labels, with full text on hover.
- Fit more player attribute columns at default window sizes; move Status below Identity and display morale numerically (1–20). Place manager Roles below Knowledge.
- FM24 Steam: the club **Transfer Budget** now reads and writes the value FM's finance screen shows (finance+0x7CC). The budget-record copy used before is not what FM displays, so edits to it did not show in game.
- Make player profile layouts use available space: independently stacked attribute cards reduce gaps and scrolling; **Positions & Traits** sizes the pitch to the profile viewport and places traits beside it on wide views, with readable stacked layouts on narrow screens. Keep the existing schema-backed numeric editing and Undo behaviour.
- FM27 Steam: favoured-club reasons read the right byte. **As a player** / **As a supporter** were wrong for some clubs (Mbappé's R. Madrid, Litmanen's Ajax).
- Remove redundant inline UI instructions and the inert contract-type Apply button. Keep concise units and scope caveats in labels/tooltips; Best XI development research remains in the UI guide rather than the planner.
- Distinguish ordinary writes refused before writing from attempted writes whose completion is unconfirmed. Do not imply an API failure restored game memory; retain visible errors and refresh the active record.
- Refuse queued ordinary writes after the worker observes a save unload or while a new attachment is still loading, before memory access; retain the existing per-write byte guard when ready.

### Documentation

- Prioritize nation, competition, award, official and stadium discovery/browsing in the roadmap, with read-only coverage first; mark reusable player templates as low priority.

### Packaging

- Pin the 0.4.0 native packaging identity to the immutable `v0.4.0` application tag at commit `52b5e810fa2f3559816b6799954278aa1640bf38` from successful build `41305419521`, with an independent packaging revision. No verified installer or package is claimed yet.

## 0.3.0 (prerelease)

### Features

- FM27 Steam player data: editable morale and transfer value; injuries with **Heal**; suspensions and holidays with **Lift ban**; other nationalities and favoured clubs; **Transfer listed** / **Loan listed** with the listing date; editable bonus and fee clauses. See the UI guide's "FM27 player data".
- Replace the Best XI and player-position grids with a shared striped, fully marked football pitch and vector shirt icons, readable position abbreviations and a distinct goalkeeper kit. Player familiarity retains its numeric edit controls; Best XI retains player links, fit/growth scores and narrow-window stacked depth cards.

### Safety

- Before every edit or Undo, the worker re-reads memory and refuses the write if it no longer matches what was read.

### Fixes

- FM27 Steam: ages use the in-game date instead of a fixed year, so a player born 8 Jul 1991 is 35 on 14 Jul 2026, not 36. Headers, search columns, age filters and sorting all use it. Other builds keep the previous year-based estimate.
- FM27: the club finance value previously shown as an editable "Spending Wage Budget" is FM's current wage total (incl. bonuses). It is now shown read-only as **Current Wage Total**, beside the read-only **Wage Budget**.
- Club search matches the full name as well as the short name ("Tottenham" finds Spurs).
- FM27: club headers show the club's nation (Spurs England, Swansea Wales) instead of "Unknown".
- Undo history survives FM adding or removing records while the game advances (FM27 added a staff member during one game day, which previously cleared Undo). The editor now treats a save as replaced only when its club objects move or the entity arrays become unreadable, so reloading a save still clears Undo.

## 0.2.0 (prerelease)

### Features

- Add a read-only **Best XI** planner: choose a lineup from your managed clubs or any selected club across five formations, with a minimum position familiarity, three ranked alternatives per slot, a **Room to grow** mode ranked by PA minus CA, and links to player profiles. Injuries, fitness, suspensions and registration are not checked. Contributed by Laurentiu-Andronache (#2).
- Add **Club ID** (exact current club) to player Advanced search and a **My clubs** filter covering the managed club, its reserve/B and youth squads, incoming loans and owned players loaned elsewhere. My clubs is FM27 Steam only. Contributed by Laurentiu-Andronache (#1).

### Safety

- Map FM27 Steam contract ownership (player -> contract -> team -> club) and club reserve-team vectors, verified against independently known loan and reserve-team witnesses; membership resolves by cached club identity, fails closed on incomplete data, and is rebuilt on Reload Save.
- FM27 Steam human-manager discovery requires the verified human-manager class and skips the platform-account placeholder, fixing a false match; Game Pass keeps the previous heuristic because its inherited class address is unverified.

### Distribution

- Add a per-user Windows installer with Start Menu/optional desktop shortcuts and uninstall support, plus an Arch/CachyOS `fm-editor-bin` package with declared dependencies and desktop/icon integration.
- Add native-package CI that verifies and wraps existing tagged portable binaries, tests installation/removal and settings preservation, and exports separate packaging source without moving the application tag or publishing a release.

## 0.1.0 (prerelease)

First portable Windows/Linux preview. Game/build coverage and verification limits
are described in the README and per-version memory-layout documentation.

### Usability

- Add real FM27 screenshots to the README for player profiles, player search and club facilities.
- Keep player position cells in a compact, evenly spaced formation instead of stretching across the panel; place Sweeper between DC and GK in the centre column.
- Leave the platform-account entry FM27 creates for your Steam profile ("(<account name>)") out of the staff list; it is not a person in the game world.
- Show staff job code 16 as "Manager", the title FM27 uses, instead of "Head Coach".
- Add calendar popups beside date inputs, with month/year navigation and explicit day selection through the existing undo-aware edit path.
- Show the attached version in the editor/window title while retaining worktree identity; simplify the connection block to avoid repeating the status bar's version/PID.
- Replace colour-dependent pitch toggles with explicit position checkboxes and per-position numeric familiarity minima; make Advanced Search resizable and scrollable with an always-reachable footer and Sweeper option.
- Retain the left sidebar with grouped Browse/Management destinations and a separate connection block; move Undo and Change history into the editing toolbar. Add bordered identity summaries and attribute sections.
- Align shared profile value boxes, including reputation, with a fixed-width label column and hover text for truncated labels; add a render-only alignment regression test.
- Standardize player, staff, human-manager and club identity headers, section presentation and attribute controls; adapt attribute layouts to narrower windows.
- Add related club-name links using existing schema chains/cached identities, and a ground-name shortcut to the club Stadium tab.
- Make Back/Forward history span entity types and search pages while retaining search filters.
- Standardize Name/UID-first search toolbars, reset actions, inclusive filter labels, numeric validation, result selection and keyboard navigation; add club sorting.
- Remove non-functional controls, unfinished tabs and empty placeholder result columns; label read-only sections explicitly.
- Document current workflows and remaining relationship support in [the UI guide](docs/ui-guide.md).

### Safety

- Select schemas per build (release + storefront) instead of per release. PC Game Pass installs are recognised by their GDK manifest or `XboxGames` / `WindowsApps` location and get their own schema; FM27 Game Pass starts as an unverified copy of the Steam schema with an uncalibrated hash, so it can attach for bring-up on Windows. The toolbar names the build ("FM27 Game Pass"), and fm-debug `attach_status` reports the storefront and the module path.
- An unreadable game module (e.g. under `WindowsApps`) now fails the version gate with the file error instead of a misleading hash mismatch; the gate reads only the first 64 KB of the module.
- `Tools/scan-arrays.sh` scans every writable, non-executable section from the module's PE section table instead of a fixed FM26 window, so it reaches the FM27 arrays and any build's relocated ones.

- Support several Football Manager versions open at once: the toolbar offers one attach button per running `fm.exe` with its version and PID, and the status bar names the attached game. The editor, the fm-debug MCP server and the diagnostic binaries each read through the schema of the process they attached to; the process-global "active version" is gone, so one attachment can no longer switch another's offsets.
- fm-debug `attach_status` reports every running game, follows the game the editor is attached to, and takes an optional `pid` to switch; `get_known_schema` now reports the attached game's schema. `Tools/dump-manager.sh` and `Tools/scan-arrays.sh` accept `--pid <n>`.

- Disable memory editing and Undo when read-only or awaiting acknowledgement; record undo entries only after successful writes.
- Correlate write responses with request IDs, report detached writes as failures, preserve failed undo for retry, reconcile local buffers and keep write errors visible.
- Clear stale-save undo addresses on reconnect; retain read-only detail snapshots and reject unrelated asynchronous detail responses.
- Reject unmapped/out-of-bounds trait writes, avoid invalid ticket-price clamping on render, and safely handle missing DOB/gender data.

### Editing

- FM24 players show international caps and goals, nationality, place of birth and club, and their contract wage, expiry, squad status and squad number (mapped from FM27€0 leads, confirmed against five players' real records; squad-number edit and undo checked in game on Cristian Romero).
- FM24 clubs: training and youth facilities were swapped, and the wage budget read a copy that can differ from FM's screen; both now match FM24's Club Info and Finances. FM24 staff show FM24's own attribute names (Mental, Level of Discipline).
- Player contracts are editable: wage, expiry, squad status (FM's selectable statuses only) and squad number, plus asking price; transfer listing stays read-only.
- Player height and appearance (ethnicity, skin tone, hair colour and length, FM27) are editable.
- Staff and human-manager contract wage and expiry, and club reputation, are editable.
- Dates keep FM's flag bits above the day-of-year when edited and display correctly when flags are set.

### Project

- Keep unregistered worktree directories and failed cleanup targets intact instead of force-deleting them; report incomplete post-merge cleanup explicitly.
- Remove the superseded GUI plan and old UI-session verification logs, and clarify that uncalibrated builds can attach without a fingerprint guard.
- Remove obsolete witness snapshots and their active references; retain field-specific mapping evidence in the per-version memory-layout documentation.
- Add read-only-permission GitHub Actions jobs for native Windows/Linux checks, release startup smoke tests and portable test-build artifacts, with matching source archives, build metadata and SHA-256 checksums. Automatic release publishing is not enabled.
- Expand the roadmap using FMLE26's advertised capability scope, distinguish existing foundations from follow-up work, and propose a distinct search-and-inspect UI direction. These are planning changes, not implemented features.
- Explicitly track removing active player-unhappiness concerns separately from morale/squad status, and add a first-release CI/packaging checklist to the roadmap.
- Consolidate project safety and verification rules in a harness-neutral AGENTS.md, move detailed procedures into docs/development.md, and archive the original GUI implementation plan rather than treating it as current tasks.
- Add a README and license the project under GPL-3.0-or-later.
- Remove one-off research scripts, the snapshot-analysis toolchain and unused diagnostic binaries; keep `scan_arrays` and `dump_manager`.
- Keep local agent and editor configuration out of the repository; `.mcp.json` is now a cross-platform example for the fm-debug MCP server.
- Make the MCP server crates ordinary dependencies (Cargo cannot select dependencies on `debug_assertions`); the server itself remains debug-only.
