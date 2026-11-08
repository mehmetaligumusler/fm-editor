//! Worker thread loop — the heart of the bridge. Owns the `FmProcess` and
//! the `ArrayCache`, dispatches each `BridgeCmd` to the appropriate handler,
//! and implements the Attach/Ping state machine: count-stability promotes a
//! save to ready, and the save identity (see `SAVE_IDENTITY_SAMPLE`) decides
//! whether it is still the same save.
//!
//! Adding a new command: drop a handler module (e.g. `bridge/my_thing.rs`)
//! with a `pub(super) fn handle(...)` and add one match arm below. The arm
//! should be a single-line call into the new module's handler — keeping
//! this file boring is the whole point.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;

use crate::club;
use crate::player;
use crate::process::{self, FmProcess};
use crate::staff;

use super::bulk_load;
use super::cmd::{BridgeCmd, BridgeResult, EntityCounts};
use super::entity_load;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

pub(super) struct ArrayCache {
    pub(super) humans_data: u64,
    pub(super) humans_count: usize,
    pub(super) staff_data: u64,
    pub(super) staff_count: usize,
    pub(super) clubs_data: u64,
    pub(super) clubs_count: usize,
    /// Club object addresses at the start of the club array (see
    /// `SAVE_IDENTITY_SAMPLE`). Empty when they could not be read.
    pub(super) save_identity: Vec<u64>,
}

/// Number of club objects whose addresses identify the loaded save.
///
/// FM allocates new entity objects whenever it loads a save, so the same
/// addresses mean the same loaded save, while counts can change without a
/// new save. FM27 Steam (2026-10-31): one game day added a staff record
/// (49,197 → 49,198); across a later game day every sampled club, player
/// and staff address stayed put; reloading the same save replaced all of
/// them, and the counts then differed by one from before the reload.
const SAVE_IDENTITY_SAMPLE: usize = 8;

pub(super) fn build_cache(fm: &FmProcess) -> ArrayCache {
    let (hd, _, hc) = player::humans_array(fm).unwrap_or((0, 0, 0));
    let (sd, _, sc) = staff::staff_array(fm).unwrap_or((0, 0, 0));
    let (cd, _, cc) = club::clubs_array(fm).unwrap_or((0, 0, 0));
    ArrayCache {
        humans_data: hd,
        humans_count: hc,
        staff_data: sd,
        staff_count: sc,
        clubs_data: cd,
        clubs_count: cc,
        save_identity: read_save_identity(fm, cd, cc),
    }
}

fn read_save_identity(fm: &FmProcess, clubs_data: u64, clubs_count: usize) -> Vec<u64> {
    let n = clubs_count.min(SAVE_IDENTITY_SAMPLE);
    if n == 0 || !player::is_valid_ptr(clubs_data) {
        return Vec::new();
    }
    match fm.read_bytes(clubs_data, n * 8) {
        Ok(bytes) if bytes.len() == n * 8 => bytes
            .chunks_exact(8)
            .map(|c| u64::from_le_bytes(c.try_into().expect("8-byte chunk")))
            .collect(),
        _ => Vec::new(),
    }
}

/// Cheap plausibility check on an ArrayCache: non-null userspace pointers,
/// counts in realistic ranges and a readable save identity. Does NOT
/// guarantee save-ready — must be paired with count-stability tracking to
/// reject save-creation mid-populate.
fn counts_are_plausible(ac: &ArrayCache) -> bool {
    if ac.humans_data < 0x10000 || ac.humans_data > 0x7FFFFFFFFFFF {
        return false;
    }
    if ac.staff_data < 0x10000 || ac.staff_data > 0x7FFFFFFFFFFF {
        return false;
    }
    if ac.clubs_data < 0x10000 || ac.clubs_data > 0x7FFFFFFFFFFF {
        return false;
    }
    if ac.save_identity.is_empty() || !ac.save_identity.iter().all(|&p| player::is_valid_ptr(p)) {
        return false;
    }
    (1000..=2_000_000).contains(&ac.humans_count)
        && (1000..=2_000_000).contains(&ac.staff_count)
        && (100..=200_000).contains(&ac.clubs_count)
}

fn counts_tuple(ac: &ArrayCache) -> (usize, usize, usize) {
    (ac.humans_count, ac.staff_count, ac.clubs_count)
}

fn entity_counts(ac: &ArrayCache) -> EntityCounts {
    EntityCounts {
        players: ac.humans_count,
        staff: ac.staff_count,
        clubs: ac.clubs_count,
    }
}

/// What one status probe means for the attached game's save.
#[derive(Debug, PartialEq)]
enum PingTransition {
    /// No save is loaded yet, or its counts are not yet stable.
    Waiting,
    /// Plausible counts were seen twice in a row: the save is ready.
    SaveReady,
    /// The loaded save is gone or was replaced. Undo addresses must not be
    /// reused, so the app discards entity state.
    SaveUnloaded,
    /// The same save is still loaded. FM can add or remove records while the
    /// game advances; that changes the counts but not the save.
    SameSave { counts_changed: bool },
}

fn ping_transition(
    loaded: Option<&ArrayCache>,
    current: &ArrayCache,
    last_counts: Option<(usize, usize, usize)>,
) -> PingTransition {
    let plausible = counts_are_plausible(current);
    match loaded {
        None if plausible && last_counts == Some(counts_tuple(current)) => {
            PingTransition::SaveReady
        }
        None => PingTransition::Waiting,
        Some(loaded) if plausible && loaded.save_identity == current.save_identity => {
            PingTransition::SameSave {
                counts_changed: counts_tuple(loaded) != counts_tuple(current),
            }
        }
        Some(_) => PingTransition::SaveUnloaded,
    }
}

// ---------------------------------------------------------------------------
// Worker loop
// ---------------------------------------------------------------------------

fn next_policy_epoch() -> u64 {
    static EPOCH: AtomicU64 = AtomicU64::new(1);
    let mut value = EPOCH.load(Ordering::Relaxed);
    loop {
        let next = value
            .checked_add(1)
            .expect("policy session counter exhausted");
        match EPOCH.compare_exchange_weak(value, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return value,
            Err(current) => value = current,
        }
    }
}

pub(super) fn worker_loop(
    rx: mpsc::Receiver<BridgeCmd>,
    tx: mpsc::Sender<BridgeResult>,
    allow_policy_trial: bool,
) {
    let mut policy_epoch = next_policy_epoch();
    let mut fm: Option<FmProcess> = None;
    let mut cache: Option<ArrayCache> = None;
    // Count-stability tracking: the (humans, staff, clubs) counts we saw
    // on the previous probe. Only promote to Connected when two
    // consecutive probes report the same plausible counts — rejects the
    // save-creation window where counts drift as FM populates the world.
    let mut last_counts: Option<(usize, usize, usize)> = None;

    while let Ok(cmd) = rx.recv() {
        match cmd {
            BridgeCmd::Attach(pid) => {
                policy_epoch = next_policy_epoch();
                let attached = match pid {
                    Some(pid) => FmProcess::attach_pid(pid),
                    None => match process::running_games() {
                        Ok(games) if games.len() > 1 => {
                            let _ = tx.send(BridgeResult::ChooseGame(games));
                            continue;
                        }
                        Ok(games) => match games.first() {
                            Some(game) => FmProcess::attach_pid(game.pid),
                            None => Err("fm.exe is not running".to_string()),
                        },
                        Err(e) => Err(e),
                    },
                };
                match attached {
                    Ok(process) => {
                        let pid = process.pid;
                        let build = process.build;
                        let notice = process.dll_hash_notice.clone();
                        let game_date = process.game_date();
                        let ac = build_cache(&process);
                        // Never promote on the first observation — even if
                        // counts look plausible right now, we haven't seen
                        // them stable yet. Record them and let the next
                        // Ping (2s later) decide.
                        last_counts = if counts_are_plausible(&ac) {
                            Some(counts_tuple(&ac))
                        } else {
                            None
                        };
                        cache = None;
                        fm = Some(process);
                        let save_name = fm.as_ref().and_then(crate::save_name::detect_save_name);
                        let _ = tx.send(BridgeResult::Attached(pid, build, game_date, save_name));
                        if let Some(msg) = notice {
                            let _ = tx.send(BridgeResult::Error(msg));
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(BridgeResult::Error(e));
                    }
                }
            }

            BridgeCmd::LoadEntityCache => {
                if let (Some(f), Some(c)) = (&fm, &cache) {
                    let _ = tx.send(BridgeResult::PingOk(f.game_date()));
                    match bulk_load::load_entity_cache(f, c, &tx) {
                        Ok(arc) => {
                            let _ = tx.send(BridgeResult::CacheReady(arc));
                        }
                        Err(e) => {
                            let _ = tx.send(BridgeResult::CacheFailed(e));
                        }
                    }
                }
            }

            BridgeCmd::LoadPlayer(addr, index) => {
                if let Some(f) = &fm {
                    match player::read_player_at(f, addr, index) {
                        Ok(rec) => {
                            let _ = tx.send(BridgeResult::PlayerLoaded(rec));
                        }
                        Err(e) => {
                            let _ = tx.send(BridgeResult::Error(e));
                        }
                    }
                }
            }

            BridgeCmd::LoadStaff(addr, index) => {
                if let Some(f) = &fm {
                    match entity_load::load_staff_view(f, addr, index) {
                        Ok(v) => {
                            let _ = tx.send(BridgeResult::StaffLoaded(v));
                        }
                        Err(e) => {
                            let _ = tx.send(BridgeResult::Error(e));
                        }
                    }
                }
            }

            BridgeCmd::LoadClub(addr, index) => {
                if let Some(f) = &fm {
                    match entity_load::load_club_view(f, addr, index) {
                        Ok(v) => {
                            let _ = tx.send(BridgeResult::ClubLoaded(v));
                        }
                        Err(e) => {
                            let _ = tx.send(BridgeResult::Error(e));
                        }
                    }
                }
            }

            BridgeCmd::LoadNameIndex => {
                let result = match (&fm, &cache) {
                    (Some(f), Some(c)) => crate::person::build_name_index(
                        f,
                        (c.humans_data, c.humans_count),
                        (c.staff_data, c.staff_count),
                    )
                    .map(std::sync::Arc::new)
                    .ok_or_else(|| "Name records are not mapped for this build".to_string()),
                    _ => Err("No save is loaded".to_string()),
                };
                let _ = tx.send(BridgeResult::NameIndex(result));
            }

            BridgeCmd::LoadHumanManager => {
                if let (Some(f), Some(_c)) = (&fm, &cache) {
                    match entity_load::load_human_manager(f) {
                        Ok(v) => {
                            let _ = tx.send(BridgeResult::HumanManagerLoaded(v));
                        }
                        Err(e) => {
                            let _ = tx.send(BridgeResult::Error(e));
                        }
                    }
                }
            }

            BridgeCmd::PrepareClubPolicy {
                request_id,
                identity,
            } => {
                let result = fm
                    .as_ref()
                    .ok_or_else(|| "No process attached".to_owned())
                    .and_then(|f| {
                        if cache.is_none() || !counts_are_plausible(&build_cache(f)) {
                            return Err("Save is not ready for a policy preview".into());
                        }
                        crate::club_policy::prepare(f, policy_epoch, identity)
                    });
                let _ = tx.send(BridgeResult::ClubPolicyPrepared { request_id, result });
            }

            BridgeCmd::WriteBytes {
                id,
                addr,
                expected,
                data,
                policy_guard,
            } => {
                let result = match &fm {
                    Some(f) if policy_guard.is_some() => {
                        if !allow_policy_trial {
                            Err(("Policy writes are disabled; start explicitly with --enable-policy-trial for a separate-copy trial".into(),Some(false)))
                        } else {
                            policy_guard
                                .as_ref()
                                .expect("matched Some")
                                .write(f, policy_epoch, addr, &expected, &data)
                                .map_err(|e| (e.message, Some(e.write_attempted)))
                        }
                    }
                    Some(f) => checked_write_ready(
                        cache.is_some(),
                        addr,
                        &expected,
                        &data,
                        |at, len| f.read_bytes(at, len),
                        |at, bytes| f.write_bytes(at, bytes),
                    )
                    .map(|()| None)
                    .map_err(|e| (e.message, Some(e.write_attempted))),
                    None => Err(("Cannot write: no process attached".to_owned(), Some(false))),
                };
                let response = match result {
                    Ok(warning) => BridgeResult::WriteOk { id, warning },
                    Err((error, write_attempted)) => BridgeResult::WriteFail {
                        id,
                        error,
                        write_attempted,
                    },
                };
                let _ = tx.send(response);
            }

            BridgeCmd::Ping => {
                if let Some(f) = &fm {
                    // Cheap process-alive probe first
                    if f.read_u8(f.gp_base).is_err() {
                        let had_cache = cache.is_some();
                        fm = None;
                        policy_epoch = next_policy_epoch();
                        cache = None;
                        last_counts = None;
                        if had_cache {
                            // Game exited while save was loaded — enter ReadOnly.
                            // The app preserves its Arc<EntityCache>; send
                            // GameClosed so it disables writes but keeps browsing.
                            let _ = tx.send(BridgeResult::GameClosed);
                        } else {
                            let _ = tx.send(BridgeResult::PingFail);
                        }
                    } else {
                        let pid = f.pid;
                        let current = build_cache(f);
                        let plausible = counts_are_plausible(&current);
                        let current_tuple = counts_tuple(&current);

                        match ping_transition(cache.as_ref(), &current, last_counts) {
                            PingTransition::SaveReady => {
                                // Two consecutive stable probes → save ready.
                                // Auto-trigger human manager + entity cache load.
                                let counts = entity_counts(&current);
                                cache = Some(current);
                                let save_name = crate::save_name::detect_save_name(f);
                                let _ = tx.send(BridgeResult::Connected(
                                    pid,
                                    counts,
                                    f.game_date(),
                                    save_name,
                                ));
                                // Human manager (fast, goes first)
                                if let (Some(f2), Some(_)) = (&fm, &cache) {
                                    match entity_load::load_human_manager(f2) {
                                        Ok(v) => {
                                            let _ = tx.send(BridgeResult::HumanManagerLoaded(v));
                                        }
                                        Err(e) => {
                                            let _ = tx.send(BridgeResult::Error(e));
                                        }
                                    }
                                }
                                // Entity cache (slow bulk load)
                                if let (Some(f2), Some(c2)) = (&fm, &cache) {
                                    match bulk_load::load_entity_cache(f2, c2, &tx) {
                                        Ok(arc) => {
                                            let _ = tx.send(BridgeResult::CacheReady(arc));
                                        }
                                        Err(e) => {
                                            let _ = tx.send(BridgeResult::CacheFailed(e));
                                        }
                                    }
                                }
                            }
                            PingTransition::SaveUnloaded => {
                                policy_epoch = next_policy_epoch();
                                // The save identity changed or the arrays became
                                // implausible → a different save, or none.
                                cache = None;
                                let _ = tx.send(BridgeResult::SaveUnloaded(pid));
                            }
                            PingTransition::SameSave {
                                counts_changed: true,
                            } => {
                                // Same save, records added or removed. The arrays
                                // may have been reallocated, so keep the new bounds.
                                let counts = entity_counts(&current);
                                cache = Some(current);
                                let _ = tx.send(BridgeResult::CountsChanged(counts, f.game_date()));
                            }
                            PingTransition::SameSave {
                                counts_changed: false,
                            }
                            | PingTransition::Waiting => {
                                let _ = tx.send(BridgeResult::PingOk(f.game_date()));
                            }
                        }
                        last_counts = if plausible { Some(current_tuple) } else { None };
                    }
                } else {
                    let _ = tx.send(BridgeResult::PingFail);
                }
            }

            BridgeCmd::Shutdown => break,
        }
    }
}

#[derive(Debug)]
struct CheckedWriteError {
    message: String,
    write_attempted: bool,
}

impl CheckedWriteError {
    fn refused(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            write_attempted: false,
        }
    }
}

/// A queued command must not outlive the worker's observed save context.
/// SaveUnloaded clears the cache before the next command is processed.
fn checked_write_ready(
    save_ready: bool,
    addr: u64,
    expected: &[u8],
    data: &[u8],
    read: impl FnOnce(u64, usize) -> Result<Vec<u8>, String>,
    write: impl FnOnce(u64, &[u8]) -> Result<(), String>,
) -> Result<(), CheckedWriteError> {
    if !save_ready {
        return Err(CheckedWriteError::refused(
            "Cannot write: the save is not ready or was unloaded; load a profile again before editing",
        ));
    }
    checked_write(addr, expected, data, read, write)
}

/// A best-effort compare-before-write check, not an atomic operation across
/// processes: FM can still update memory between the read and the write.
fn checked_write(
    addr: u64,
    expected: &[u8],
    data: &[u8],
    read: impl FnOnce(u64, usize) -> Result<Vec<u8>, String>,
    write: impl FnOnce(u64, &[u8]) -> Result<(), String>,
) -> Result<(), CheckedWriteError> {
    if expected.len() != data.len() || data.is_empty() {
        return Err(CheckedWriteError::refused(
            "Cannot write: expected bytes and data must have the same non-zero length",
        ));
    }
    let last = addr.checked_add(data.len() as u64 - 1);
    if !crate::player::is_valid_ptr(addr) || !last.is_some_and(crate::player::is_valid_ptr) {
        return Err(CheckedWriteError::refused(
            "Cannot write: invalid memory range; reload the player and retry",
        ));
    }
    let changed = "Memory changed since it was read (FM updated it or the record moved); reload the player and retry";
    let current = read(addr, data.len())
        .map_err(|error| CheckedWriteError::refused(format!("{changed}: {error}")))?;
    if current != expected {
        return Err(CheckedWriteError::refused(changed));
    }
    write(addr, data).map_err(|message| CheckedWriteError {
        message,
        write_attempted: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arrays(counts: (usize, usize, usize), identity: &[u64]) -> ArrayCache {
        ArrayCache {
            humans_data: 0x1_0000_0000,
            humans_count: counts.0,
            staff_data: 0x2_0000_0000,
            staff_count: counts.1,
            clubs_data: 0x3_0000_0000,
            clubs_count: counts.2,
            save_identity: identity.to_vec(),
        }
    }

    const SAVE_A: [u64; 3] = [0x2_56F3_0040, 0x2_56F3_01B0, 0x2_56F3_0320];
    const SAVE_B: [u64; 3] = [0x2_57CB_0930, 0x2_57CB_07C0, 0x2_57CB_0650];
    const COUNTS: (usize, usize, usize) = (103_516, 49_197, 33_471);

    #[test]
    fn records_added_to_the_same_save_keep_it_loaded() {
        // FM27: one game day added a staff record to the loaded save.
        let loaded = arrays(COUNTS, &SAVE_A);
        let grown = arrays((103_516, 49_198, 33_471), &SAVE_A);
        assert_eq!(
            ping_transition(Some(&loaded), &grown, Some(COUNTS)),
            PingTransition::SameSave {
                counts_changed: true
            }
        );
        assert_eq!(
            ping_transition(Some(&loaded), &arrays(COUNTS, &SAVE_A), Some(COUNTS)),
            PingTransition::SameSave {
                counts_changed: false
            }
        );
    }

    #[test]
    fn a_replaced_save_is_unloaded_even_with_unchanged_counts() {
        let loaded = arrays(COUNTS, &SAVE_A);
        for counts in [COUNTS, (103_517, 49_197, 33_471)] {
            assert_eq!(
                ping_transition(Some(&loaded), &arrays(counts, &SAVE_B), Some(COUNTS)),
                PingTransition::SaveUnloaded
            );
        }
    }

    #[test]
    fn empty_unreadable_or_implausible_arrays_unload_the_save() {
        let loaded = arrays(COUNTS, &SAVE_A);
        for current in [
            arrays((0, 0, 0), &[]),
            arrays(COUNTS, &[]),
            arrays(COUNTS, &[SAVE_A[0], 0, SAVE_A[2]]),
            arrays((10, 49_197, 33_471), &SAVE_A),
        ] {
            assert_eq!(
                ping_transition(Some(&loaded), &current, Some(COUNTS)),
                PingTransition::SaveUnloaded
            );
        }
    }

    #[test]
    fn a_save_becomes_ready_only_after_two_matching_plausible_probes() {
        let current = arrays(COUNTS, &SAVE_A);
        assert_eq!(
            ping_transition(None, &current, None),
            PingTransition::Waiting
        );
        assert_eq!(
            ping_transition(None, &current, Some((103_516, 49_196, 33_471))),
            PingTransition::Waiting
        );
        assert_eq!(
            ping_transition(None, &current, Some(COUNTS)),
            PingTransition::SaveReady
        );
        // An unreadable identity never promotes, so Undo is never tied to a
        // save the bridge cannot recognise later.
        assert_eq!(
            ping_transition(None, &arrays(COUNTS, &[]), Some(COUNTS)),
            PingTransition::Waiting
        );
    }
    #[test]
    fn checked_write_matches_snapshot_before_writing() {
        use std::cell::RefCell;
        let memory = RefCell::new(vec![1, 2]);
        let result = checked_write(
            0x20000,
            &[1, 2],
            &[3, 4],
            |at, len| {
                assert_eq!((at, len), (0x20000, 2));
                Ok(memory.borrow().clone())
            },
            |at, data| {
                assert_eq!(at, 0x20000);
                *memory.borrow_mut() = data.to_vec();
                Ok(())
            },
        );
        assert!(result.is_ok());
        assert_eq!(*memory.borrow(), vec![3, 4]);
    }

    #[test]
    fn queued_write_requires_a_ready_save_before_any_memory_access() {
        let error = checked_write_ready(
            false,
            0x20000,
            &[1],
            &[2],
            |_, _| panic!("must not read after the save unloads"),
            |_, _| panic!("must not write after the save unloads"),
        )
        .unwrap_err();
        assert!(!error.write_attempted);
        assert!(error.message.contains("save is not ready"));
        let wrote = std::cell::Cell::new(false);
        checked_write_ready(
            true,
            0x20000,
            &[1],
            &[2],
            |_, _| Ok(vec![1]),
            |_, bytes| {
                assert_eq!(bytes, &[2]);
                wrote.set(true);
                Ok(())
            },
        )
        .unwrap();
        assert!(wrote.get());
    }

    #[test]
    fn checked_write_refuses_changed_unreadable_or_invalid_memory_without_writing() {
        let refuse_write = |_, _: &[u8]| -> Result<(), String> { panic!("must not write") };
        for actual in [vec![9, 2], vec![1]] {
            let error = checked_write(0x20000, &[1, 2], &[3, 4], |_, _| Ok(actual), refuse_write)
                .unwrap_err();
            assert!(!error.write_attempted);
            assert!(error.message.contains("Memory changed since it was read"));
        }
        let error = checked_write(
            0x20000,
            &[1, 2],
            &[3, 4],
            |_, _| Err("unmapped".into()),
            refuse_write,
        )
        .unwrap_err();
        assert!(!error.write_attempted);
        assert!(error.message.contains("reload the player and retry"));
        for (addr, expected, data) in [
            (0x20000, vec![1], vec![3, 4]),
            (0x20000, vec![], vec![]),
            (u64::MAX, vec![1], vec![3]),
            (0x10000, vec![1], vec![3]),
            (0x7FFF_FFFF_FFFE, vec![1, 2], vec![3, 4]),
        ] {
            let error = checked_write(
                addr,
                &expected,
                &data,
                |_, _| panic!("must not read"),
                refuse_write,
            )
            .unwrap_err();
            assert!(!error.write_attempted);
        }
    }

    #[test]
    fn checked_write_api_error_is_attempted() {
        let error = checked_write(
            0x20000,
            &[1],
            &[2],
            |_, _| Ok(vec![1]),
            |_, _| Err("write failed".into()),
        )
        .unwrap_err();
        assert!(error.write_attempted);
        assert_eq!(error.message, "write failed");
    }

    #[test]
    fn write_without_attached_process_is_acknowledged_as_failure() {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        cmd_tx
            .send(BridgeCmd::WriteBytes {
                id: 42,
                addr: 100,
                data: vec![1],
                expected: vec![0],
                policy_guard: None,
            })
            .unwrap();
        cmd_tx.send(BridgeCmd::Shutdown).unwrap();
        worker_loop(cmd_rx, result_tx, false);
        match result_rx.recv().unwrap() {
            BridgeResult::WriteFail {
                id,
                error,
                write_attempted,
            } => {
                assert_eq!(id, 42);
                assert_eq!(write_attempted, Some(false));
                assert!(error.contains("no process attached"));
            }
            _ => panic!("expected a write failure acknowledgement"),
        }
        assert!(result_rx.try_recv().is_err());
    }
}
