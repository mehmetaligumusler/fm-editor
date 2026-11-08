//! Non-interactive, read-only Epic attach/cache smoke test. Never drives FM.
use fm_editor::bridge::{BridgeCmd, BridgeResult, ProcessBridge};
use fm_editor::game_version::GameBuild;
use fm_editor::process::FmProcess;
use std::time::{Duration, Instant};

fn main() -> Result<(), String> {
    let fm = FmProcess::attach()?;
    if fm.build != GameBuild::Fm24Epic || fm.build.allows_writes() {
        return Err("diagnostic requires the read-only FM24 Epic preview".into());
    }
    let pid = fm.pid;
    println!(
        "{}",
        serde_json::json!({
            "build": fm.build.display_name(), "pid": pid,
            "fingerprint": format!("0x{:016X}", fm.module_header_hash()?),
            "writes_enabled": fm.build.allows_writes(),
        })
    );
    drop(fm);
    let bridge = ProcessBridge::new();
    bridge.send(BridgeCmd::Attach(Some(pid)));
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut next_ping = Instant::now() + Duration::from_secs(2);
    let mut loading = false;
    while Instant::now() < deadline {
        if Instant::now() >= next_ping && !loading {
            bridge.send(BridgeCmd::Ping);
            next_ping = Instant::now() + Duration::from_secs(2);
        }
        while let Some(result) = bridge.try_recv() {
            match result {
                BridgeResult::Attached(_, build, _, _) if build != GameBuild::Fm24Epic => {
                    return Err("unexpected build".into());
                }
                BridgeResult::Connected(_, _, _, _) => {
                    loading = true;
                    bridge.send(BridgeCmd::LoadEntityCache);
                }
                BridgeResult::CacheReady(cache) => {
                    let samples: Vec<_> = cache.players.iter().take(5).map(|p|
                        serde_json::json!({"name":p.name,"uid":p.uid,"dob_year":p.dob_year})
                    ).collect();
                    let screenshot_names = [
                        "Tanju Başar",
                        "Tahir Saltabaş",
                        "Kerim Demir",
                        "Erkan Ünver",
                    ];
                    let witnesses: Vec<_> = cache.players.iter()
                        .filter(|p| screenshot_names.contains(&p.name.as_str()))
                        .map(|p| serde_json::json!({"name":p.name,"uid":p.uid,"dob_year":p.dob_year}))
                        .collect();
                    println!(
                        "{}",
                        serde_json::json!({
                            "cache_ready": true, "players": cache.players.len(),
                            "staff": cache.staff.len(), "clubs": cache.clubs.len(),
                            "player_samples":samples, "screenshot_witnesses":witnesses,
                            "antalyaspor_found": cache.clubs.iter().any(|c| c.name.contains("Antalyaspor")),
                        })
                    );
                    bridge.send(BridgeCmd::Shutdown);
                    return Ok(());
                }
                BridgeResult::CacheFailed(error) | BridgeResult::Error(error) => return Err(error),
                BridgeResult::GameClosed | BridgeResult::SaveUnloaded(_) => {
                    return Err("game/save changed during diagnostic; retry later".into());
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    bridge.send(BridgeCmd::Shutdown);
    Err("timed out waiting for a stable loaded save".into())
}
