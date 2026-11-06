//! Attended write/Undo verification for the FM24 Epic build.
//!
//! Built only with `--features epic-write-test`; every command refuses to
//! run unless the attached game is `Fm24Epic` and the feature lifted the
//! build's write lock. Run it against a designated TEST save with the user
//! watching the value in game (docs/fm24/epic-preview.md). It never drives
//! FM itself, never touches save files, and every write is compare-and-swap:
//! memory is re-read immediately before writing, a changed value aborts, and
//! the written bytes are read back afterwards. `restore` writes the value
//! printed by `set` back through the same path.
//!
//! Usage:
//!   epic_edit_test find <name-substring>
//!   epic_edit_test attr show <player-uid> <field-key>
//!   epic_edit_test attr set <player-uid> <field-key> <displayed-value>
//!   epic_edit_test attr restore <player-uid> <field-key> <old-displayed-value>
//!   epic_edit_test budget show <club-uid>
//!   epic_edit_test budget set <club-uid> <new-raw-u32>
//!   epic_edit_test budget restore <club-uid> <old-raw-u32>

use fm_editor::bridge::{BridgeCmd, BridgeResult, ProcessBridge};
use fm_editor::fields::{self, Encoding, FieldLocation};
use fm_editor::game_version::GameBuild;
use fm_editor::process::FmProcess;
use std::time::{Duration, Instant};

const USAGE: &str = "usage: epic_edit_test find <name-substring> \
                    | epic_edit_test attr <show|set|restore> <player-uid> <key> [value] \
                    | epic_edit_test budget <show|set|restore> <club-uid> [value]";

fn main() {
    if let Err(e) = run() {
        println!(
            "{{\"error\":{}}}",
            serde_json::to_string(&e).unwrap_or_default()
        );
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("attr") => attr_cmd(&args[1..]),
        Some("budget") => budget_cmd(&args[1..]),
        Some("find") => find_cmd(&args[1..]),
        _ => Err(USAGE.into()),
    }
}

/// Read-only helper: list players and clubs whose name contains the given
/// (case-insensitive) substring, with the UIDs the other commands take.
fn find_cmd(args: &[String]) -> Result<(), String> {
    let Some(needle) = args.first() else {
        return Err("usage: epic_edit_test find <name-substring>".into());
    };
    let needle = needle.to_lowercase();
    if needle.len() < 3 {
        return Err("search substring must be at least 3 characters".into());
    }
    let fm = FmProcess::attach()?;
    if fm.build != GameBuild::Fm24Epic {
        return Err(format!(
            "this diagnostic only runs against the FM24 Epic build, not {}",
            fm.build.display_name()
        ));
    }
    let cache = load_cache(fm.pid)?;
    let players: Vec<_> = cache
        .players
        .iter()
        .filter(|p| p.name.to_lowercase().contains(&needle) && !p.name.is_empty())
        .take(10)
        .map(|p| serde_json::json!({"uid": p.uid, "name": p.name, "dob_year": p.dob_year}))
        .collect();
    let clubs: Vec<_> = cache
        .clubs
        .iter()
        .filter(|c| c.name.to_lowercase().contains(&needle))
        .take(10)
        .map(|c| serde_json::json!({"uid": c.uid, "name": c.name}))
        .collect();
    println!(
        "{}",
        serde_json::json!({ "players": players, "clubs": clubs })
    );
    Ok(())
}

/// Attach directly and enforce the Epic-only, feature-gated contract.
fn attach_epic_writable() -> Result<FmProcess, String> {
    let fm = FmProcess::attach()?;
    if fm.build != GameBuild::Fm24Epic {
        return Err(format!(
            "this diagnostic only runs against the FM24 Epic build, not {}",
            fm.build.display_name()
        ));
    }
    if !fm.build.allows_writes() {
        return Err(
            "FM24 Epic is read-only; rebuild with --features epic-write-test for the \
             attended test-save diagnostic"
                .into(),
        );
    }
    Ok(fm)
}

/// Wait for a loaded save through the bridge and return its entity cache.
/// Mirrors `epic_probe`: attach, ping until `Connected`, then bulk load.
fn load_cache(pid: u32) -> Result<std::sync::Arc<fm_editor::cache::EntityCache>, String> {
    let bridge = ProcessBridge::new();
    bridge.send(BridgeCmd::Attach(Some(pid)));
    let deadline = Instant::now() + Duration::from_secs(300);
    let mut next_ping = Instant::now() + Duration::from_secs(2);
    let mut asked_cache = false;
    let result = loop {
        if Instant::now() >= next_ping && !asked_cache {
            bridge.send(BridgeCmd::Ping);
            next_ping = Instant::now() + Duration::from_secs(2);
        }
        let mut outcome: Option<Result<std::sync::Arc<fm_editor::cache::EntityCache>, String>> =
            None;
        while let Some(result) = bridge.try_recv() {
            match result {
                BridgeResult::Connected(..) => {
                    bridge.send(BridgeCmd::LoadEntityCache);
                    asked_cache = true;
                }
                BridgeResult::CacheReady(cache) => {
                    outcome = Some(Ok(cache));
                    break;
                }
                BridgeResult::CacheFailed(e) | BridgeResult::Error(e) => {
                    outcome = Some(Err(e));
                    break;
                }
                BridgeResult::ChooseGame(games) => {
                    outcome = Some(Err(format!("several games running: {games:?}")));
                    break;
                }
                BridgeResult::GameClosed => {
                    outcome = Some(Err("game closed during load".into()));
                    break;
                }
                BridgeResult::SaveUnloaded(_) => {
                    outcome = Some(Err(
                        "save unloaded during load; retry when a save is loaded".into(),
                    ));
                    break;
                }
                BridgeResult::Attached(..) | BridgeResult::PingOk(_) => {}
                _ => {}
            }
        }
        if let Some(outcome) = outcome {
            break outcome;
        }
        if Instant::now() >= deadline {
            break Err("timed out waiting for a stable loaded save".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    bridge.send(BridgeCmd::Shutdown);
    result
}

/// Compare-and-swap write: refuse when memory moved, verify what landed.
fn cas_write(fm: &FmProcess, addr: u64, expected: &[u8], new: &[u8]) -> Result<(), String> {
    let current = fm
        .read_bytes(addr, expected.len())
        .map_err(|e| format!("pre-write read failed: {e}"))?;
    if current != expected {
        return Err(format!(
            "memory changed before the write: expected [{}] read [{}]",
            hex(&current),
            hex(expected)
        ));
    }
    fm.write_bytes(addr, new)?;
    let verify = fm
        .read_bytes(addr, new.len())
        .map_err(|e| format!("post-write read failed: {e}"))?;
    if verify != new {
        return Err(format!(
            "write did not stick: wrote [{}] read [{}]",
            hex(new),
            hex(&verify)
        ));
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn sane_addr(addr: u64, width: usize) -> Result<(), String> {
    if addr > 0x10000 && addr.checked_add(width as u64).unwrap_or(u64::MAX) < 0x7FFFFFFFFFFF {
        Ok(())
    } else {
        Err(format!("refusing out-of-bounds address 0x{addr:X}"))
    }
}

/// Displayed-value bounds per encoding; every branch returns the byte width.
fn check_value(encoding: Encoding, value: u32) -> Result<usize, String> {
    match encoding {
        Encoding::Encoded if (1..=20).contains(&value) => Ok(1),
        Encoding::Direct if value <= 200 => Ok(1),
        Encoding::Raw if value <= u16::MAX as u32 => Ok(2),
        Encoding::RawCapped(max) if value <= max as u32 => Ok(2),
        _ => Err(format!(
            "value {value} outside the accepted range for {encoding:?}"
        )),
    }
}

fn attr_cmd(args: &[String]) -> Result<(), String> {
    let (action, uid, key, value) = match args {
        [a, u, k] if a == "show" => (
            a.as_str(),
            u.parse::<u32>().map_err(bad_uid)?,
            k.as_str(),
            None,
        ),
        [a, u, k, v] if a == "set" || a == "restore" => (
            a.as_str(),
            u.parse::<u32>().map_err(bad_uid)?,
            k.as_str(),
            Some(
                v.parse::<u32>()
                    .map_err(|_| "value must be a whole number".to_string())?,
            ),
        ),
        _ => return Err(USAGE.into()),
    };
    let fm = attach_epic_writable()?;
    let cache = load_cache(fm.pid)?;
    let player = cache
        .players
        .iter()
        .find(|p| p.uid == uid)
        .ok_or_else(|| format!("no player with uid {uid} in the loaded save"))?;
    if player.person_shift != 0 {
        return Err("shifted player-coach layout: not supported by this diagnostic".into());
    }
    let field = fm
        .schemas()
        .field(key)
        .ok_or_else(|| format!("unknown field key {key}"))?;
    let Some(FieldLocation::Direct { offset, encoding }) = field.offset else {
        return Err(format!("field {key} is not a direct in-struct field"));
    };
    let width = match encoding {
        Encoding::Encoded | Encoding::Direct | Encoding::Raw | Encoding::RawCapped(_) => {
            check_value(encoding, value.unwrap_or(0))?
        }
        other => {
            return Err(format!(
                "encoding {other:?} not supported by this diagnostic"
            ));
        }
    };
    let off = offset as usize;
    if off.checked_add(width).unwrap_or(usize::MAX) > fm.schemas().player_struct_size {
        return Err(format!("offset 0x{off:X} lies outside the player struct"));
    }
    let addr = player.addr + offset;
    sane_addr(addr, width)?;
    let raw = fm
        .read_bytes(addr, width)
        .map_err(|e| format!("read failed: {e}"))?;
    let displayed = match encoding {
        Encoding::Encoded => FieldLocation::Direct { offset, encoding }.decode_u8(raw[0]) as u32,
        Encoding::Direct => raw[0] as u32,
        _ => u16::from_le_bytes([raw[0], raw[1]]) as u32,
    };
    match (action, value) {
        ("show", _) => {
            println!(
                "{}",
                serde_json::json!({
                    "mode": "show", "name": player.name, "uid": uid, "field": key,
                    "addr": format!("0x{addr:X}"), "raw": hex(&raw), "displayed": displayed,
                })
            );
            Ok(())
        }
        (action, Some(new_value)) => {
            check_value(encoding, new_value)?;
            let new_bytes = match encoding {
                Encoding::Encoded => {
                    [FieldLocation::Direct { offset, encoding }.encode_u8(new_value as u8)]
                }
                Encoding::Direct => [new_value as u8],
                _ => new_value.to_le_bytes()[..2].try_into().unwrap(),
            };
            cas_write(&fm, addr, &raw, &new_bytes)?;
            let verify = fm
                .read_bytes(addr, width)
                .map_err(|e| format!("read failed: {e}"))?;
            println!(
                "{}",
                serde_json::json!({
                    "mode": action, "name": player.name, "uid": uid, "field": key,
                    "addr": format!("0x{addr:X}"),
                    "before_displayed": displayed, "after_displayed": new_value,
                    "raw_before": hex(&raw), "raw_after": hex(&verify),
                    "restore_with": format!("epic_edit_test attr restore {uid} {key} {displayed}"),
                })
            );
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

fn budget_cmd(args: &[String]) -> Result<(), String> {
    let (action, uid, value) = match args {
        [a, u] if a == "show" => (a.as_str(), u.parse::<u32>().map_err(bad_uid)?, None),
        [a, u, v] if a == "set" || a == "restore" => (
            a.as_str(),
            u.parse::<u32>().map_err(bad_uid)?,
            Some(
                v.parse::<u32>()
                    .map_err(|_| "value must be a whole number".to_string())?,
            ),
        ),
        _ => return Err(USAGE.into()),
    };
    let fm = attach_epic_writable()?;
    let cache = load_cache(fm.pid)?;
    let club = cache
        .clubs
        .iter()
        .find(|c| c.uid == uid)
        .ok_or_else(|| format!("no club with uid {uid} in the loaded save"))?;
    let field = fm
        .schemas()
        .field("c_transfer_budget")
        .ok_or("c_transfer_budget unmapped for this build")?;
    let Some(FieldLocation::SubRecord {
        container,
        offset,
        encoding,
    }) = field.offset
    else {
        return Err("c_transfer_budget is not a sub-record field".into());
    };
    if encoding != Encoding::RawU32 {
        return Err(format!("unexpected budget encoding {encoding:?}"));
    }
    let sub_ptr = fields::resolve_sub_ptr(&fm, club.addr, container, fm.schemas().club_sub_records)
        .ok_or("finance sub-record pointer is unreadable")?;
    let addr = sub_ptr + offset;
    sane_addr(addr, 4)?;
    let raw = fm
        .read_bytes(addr, 4)
        .map_err(|e| format!("read failed: {e}"))?;
    let current = u32::from_le_bytes(raw[..].try_into().unwrap());
    match (action, value) {
        ("show", _) => {
            println!(
                "{}",
                serde_json::json!({
                    "mode": "show", "club": club.name, "uid": uid,
                    "addr": format!("0x{addr:X}"), "budget": current,
                })
            );
            Ok(())
        }
        (action, Some(new_value)) => {
            if new_value > 2_000_000_000 {
                return Err("refusing budgets above 2,000,000,000".into());
            }
            cas_write(&fm, addr, &raw, &new_value.to_le_bytes())?;
            let verify = fm
                .read_bytes(addr, 4)
                .map_err(|e| format!("read failed: {e}"))?;
            println!(
                "{}",
                serde_json::json!({
                    "mode": action, "club": club.name, "uid": uid,
                    "addr": format!("0x{addr:X}"),
                    "before": current, "after": u32::from_le_bytes(verify[..].try_into().unwrap()),
                    "restore_with": format!("epic_edit_test budget restore {uid} {current}"),
                })
            );
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

fn bad_uid(e: std::num::ParseIntError) -> String {
    format!("uid must be a number: {e}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_reject_out_of_range_displayed_values() {
        assert_eq!(check_value(Encoding::Encoded, 1), Ok(1));
        assert_eq!(check_value(Encoding::Encoded, 20), Ok(1));
        assert!(check_value(Encoding::Encoded, 0).is_err());
        assert!(check_value(Encoding::Encoded, 21).is_err());
        assert!(check_value(Encoding::Direct, 201).is_err());
        assert!(check_value(Encoding::Raw, u16::MAX as u32 + 1).is_err());
        assert!(check_value(Encoding::RawCapped(200), 201).is_err());
        assert!(check_value(Encoding::RawU32, 1).is_err()); // not an attr encoding
    }

    #[test]
    fn encoded_attributes_round_trip_through_the_schema_helpers() {
        let loc = FieldLocation::Direct {
            offset: 0,
            encoding: Encoding::Encoded,
        };
        for displayed in 1u8..=20 {
            let raw = loc.encode_u8(displayed);
            assert_eq!(loc.decode_u8(raw), displayed);
        }
    }
}
