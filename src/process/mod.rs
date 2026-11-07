//! Attach to fm.exe and read/write its memory.
//!
//! Platform backends implement the raw primitives — pid lookup, module
//! enumeration, cross-process byte transfer. Everything above raw bytes
//! (version sniffing, the DLL-hash gate, typed reads, FM name decoding)
//! is shared and platform-independent.
//!
//! On Linux the target is the same Windows fm.exe running under
//! Proton/Wine, so every schema offset is identical to a native Windows
//! install; only the process-access primitives differ.

#[cfg(windows)]
mod win32;
#[cfg(windows)]
use self::win32 as backend;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use self::linux as backend;

use crate::fields::VersionedSchemas;
use crate::game_date::GameDate;
use crate::game_version::{GameBuild, GameVersion, Storefront};

/// A running `fm.exe` whose game build could be identified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunningGame {
    pub pid: u32,
    pub build: GameBuild,
}

impl std::fmt::Display for RunningGame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (PID {})", self.build.display_name(), self.pid)
    }
}

/// Every running `fm.exe` whose build can be determined, ordered by PID.
///
/// FM24, FM26 and FM27 all run as `fm.exe`, so several can be open at once.
/// A process whose build cannot be sniffed (e.g. a launcher sharing the
/// name, or a storefront without a schema) is left out; when nothing is
/// identifiable the error says why.
pub fn running_games() -> Result<Vec<RunningGame>, String> {
    let pids = backend::find_pids("fm.exe")?;
    if pids.is_empty() {
        return Err("fm.exe is not running".to_string());
    }
    let mut games = Vec::with_capacity(pids.len());
    let mut first_err = None;
    for pid in pids {
        match detect_build(pid) {
            Ok(build) => games.push(RunningGame { pid, build }),
            Err(e) => {
                first_err.get_or_insert(format!("pid {pid}: {e}"));
            }
        }
    }
    match (games.is_empty(), first_err) {
        (true, Some(e)) => Err(e),
        _ => Ok(games),
    }
}

/// Identify the build of one `fm.exe`.
///
/// The version comes from the install path (it always contains the release
/// year): FM26/FM27 keep their game logic in `game_plugin.dll`, FM24 in
/// `fm.exe` itself, so the plugin path is tried first and the exe path is
/// the fallback. The storefront comes from the exe's install location.
fn detect_build(pid: u32) -> Result<GameBuild, String> {
    let exe_path = backend::find_module_info(pid, "fm.exe").map(|m| m.2);
    let sniff_path = match backend::find_module_info(pid, "game_plugin.dll") {
        Ok(m) => m.2,
        Err(e) => exe_path.clone().map_err(|_| e)?,
    };
    let version = sniff_version(&sniff_path)?;
    let storefront = match &exe_path {
        Ok(exe) if has_epic_marker(exe) => Storefront::Epic,
        Ok(exe) => storefront_of(exe, has_gdk_config(exe)),
        Err(_) => storefront_of(&sniff_path, false),
    };
    GameBuild::resolve(version, storefront).map_err(|e| format!("{e} ({sniff_path})"))
}

/// Require two Epic markers when recognising a custom installation directory.
fn has_epic_marker(exe_path: &str) -> bool {
    std::path::Path::new(exe_path)
        .parent()
        .is_some_and(|dir| dir.join(".egstore").is_dir() && dir.join("epic.txt").is_file())
}

/// True when Microsoft's GDK manifest sits next to the exe.
fn has_gdk_config(exe_path: &str) -> bool {
    let dir = match exe_path.rfind(['/', '\\']) {
        Some(i) => &exe_path[..i],
        None => return false,
    };
    std::path::Path::new(dir)
        .join("MicrosoftGame.config")
        .is_file()
}

/// Storefront of an install, from its exe (or module) path and whether the
/// GDK manifest sits next to the exe. Game Pass installs live under an
/// `XboxGames` folder (any drive, the Xbox app default) or, for older Store
/// packages, `Program Files\WindowsApps`. Epic's default install path is also
/// recognised; .egstore + epic.txt markers handle custom Epic install paths.
fn storefront_of(path: &str, has_gdk_config: bool) -> Storefront {
    let lower = path.to_lowercase().replace('\\', "/");
    if has_gdk_config || lower.contains("/xboxgames/") || lower.contains("/windowsapps/") {
        Storefront::GamePass
    } else if lower.contains("/epic games/") {
        Storefront::Epic
    } else {
        Storefront::Steam
    }
}

/// Handle to an open game process with its resolved game module and
/// selected game build.
pub struct FmProcess {
    handle: backend::ProcHandle,
    pub pid: u32,
    /// Base of the module that holds the game data RVAs:
    /// `game_plugin.dll` on FM26/FM27, `fm.exe` itself on FM24
    /// (see `GameVersion::module_name`).
    pub gp_base: u64,
    pub gp_end: u64,
    /// On-disk path of that module, as the OS reports it.
    pub module_path: String,
    pub build: GameBuild,
    /// Set when `known_dll_hash` is 0 in the schema (not yet calibrated).
    /// The worker forwards this to the UI as a status notice so the developer
    /// knows the current hash to paste into `fields/<version>/mod.rs`.
    pub dll_hash_notice: Option<String>,
}

impl FmProcess {
    /// Attach to the only running game.
    ///
    /// Fails when no game runs, and when several do (FM24, FM26 and FM27
    /// all run as `fm.exe`): the error lists them so the caller can pick
    /// one with [`FmProcess::attach_pid`].
    pub fn attach() -> Result<Self, String> {
        let games = running_games()?;
        match games.as_slice() {
            [game] => Self::attach_pid(game.pid),
            _ => Err(format!(
                "{} games are running: {}. Choose one by PID.",
                games.len(),
                games
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
            )),
        }
    }

    /// Attach to the game with this PID. The schema comes from that
    /// process's own install path, never from any other attachment.
    pub fn attach_pid(pid: u32) -> Result<Self, String> {
        let build = detect_build(pid)?;
        let handle = if build.allows_writes() {
            backend::open(pid)?
        } else {
            backend::open_read_only(pid)?
        };
        let module = build.version().module_name();
        let (gp_base, gp_size, gp_path) = backend::find_module_info(pid, module)?;
        // Probe one read up front so a blocked cross-process access path
        // (e.g. Linux yama ptrace_scope) fails the attach with the OS hint
        // instead of erroring on the first field read later.
        backend::read_bytes(&handle, gp_base, 2)
            .map_err(|e| format!("attached to pid {pid} but cannot read its memory: {e}"))?;
        let known = build.schemas().known_dll_hash;
        let dll_hash_notice = match (known, hash_module_header(&gp_path)) {
            // Schema not yet calibrated — allow attach but surface the hash.
            (0, Ok(hash)) => Some(format!(
                "{} {module} hash not calibrated (current: 0x{hash:016X}). \
                 Set known_dll_hash: 0x{hash:016X} in {} in src/fields/{}/mod.rs \
                 to enable the version gate.",
                build.display_name(),
                build.schema_name(),
                build.version().short_name(),
            )),
            (0, Err(e)) => Some(format!(
                "{} {module} hash not calibrated and unavailable: {e}",
                build.display_name(),
            )),
            (_, Err(e)) => {
                return Err(format!(
                    "cannot check the {} build: {e}",
                    build.display_name()
                ));
            }
            (known, Ok(hash)) if known != hash => {
                return Err(format!(
                    "{} version mismatch — schema {} built for 0x{known:016X}, \
                     running 0x{hash:016X} ({gp_path}). Re-verify offsets \
                     against the new patch before use.",
                    build.display_name(),
                    build.schema_name(),
                ));
            }
            _ => None,
        };
        Ok(Self {
            handle,
            pid,
            gp_base,
            gp_end: gp_base + gp_size,
            module_path: gp_path,
            build,
            dll_hash_notice,
        })
    }

    /// The build-specific schema bundle for this process.
    ///
    /// Every reader that holds an `FmProcess` pulls field data, RVAs and
    /// struct sizes through here, so two games open at once never share a
    /// schema.
    pub fn schemas(&self) -> &'static VersionedSchemas {
        self.build.schemas()
    }

    /// Observed on-disk module header fingerprint, using the attachment gate's
    /// algorithm. Reporting it does not calibrate or override that gate.
    pub fn module_header_hash(&self) -> Result<u64, String> {
        hash_module_header(&self.module_path)
    }

    /// Read the current in-game date through this attachment's verified schema.
    /// Unmapped builds, invalid pointers/dates, and failed reads return None.
    pub fn game_date(&self) -> Option<GameDate> {
        let rva = self.schemas().rva_current_date;
        if rva == 0 {
            return None;
        }
        let valid = |address: u64| address > 0x10000 && address < 0x7FFF_FFFF_FFFF;
        let address = self.gp_base.checked_add(rva)?;
        if !valid(address) || !valid(address.checked_add(3)?) {
            return None;
        }
        let bytes = self.read_bytes(address, 4).ok()?;
        GameDate::from_bytes(&bytes)
    }

    /// Read `n` bytes from the target process at `addr`.
    pub fn read_bytes(&self, addr: u64, n: usize) -> Result<Vec<u8>, String> {
        backend::read_bytes(&self.handle, addr, n)
    }

    /// Read a u64 (little-endian) from the target process.
    pub fn read_u64(&self, addr: u64) -> Result<u64, String> {
        let buf = self.read_bytes(addr, 8)?;
        Ok(u64::from_le_bytes(buf.try_into().unwrap()))
    }

    /// Read a u32 (little-endian) from the target process.
    pub fn read_u32(&self, addr: u64) -> Result<u32, String> {
        let buf = self.read_bytes(addr, 4)?;
        Ok(u32::from_le_bytes(buf.try_into().unwrap()))
    }

    /// Read a `std::vector<T*>`-style `(data, end)` pointer pair stored at
    /// `gp_base + data_rva` / `gp_base + end_rva` and return
    /// `(data, end, count)`. Rejects bounds that are not valid user-space
    /// pointers, run backwards, or are not a whole number of 8-byte slots —
    /// the usual symptom of an RVA that a game patch has moved.
    pub fn read_ptr_array(&self, data_rva: u64, end_rva: u64) -> Result<(u64, u64, usize), String> {
        let data = self.read_u64(self.gp_base + data_rva)?;
        let end = self.read_u64(self.gp_base + end_rva)?;
        let valid = |p: u64| p > 0x10000 && p < 0x7FFF_FFFF_FFFF;
        if !valid(data) || !valid(end) || end < data || (end - data) % 8 != 0 {
            return Err(format!(
                "bad pointer array at gp+0x{data_rva:X}: data=0x{data:X} end=0x{end:X} \
                 (RVA moved by a game patch? run Tools/scan-arrays.sh)"
            ));
        }
        Ok((data, end, ((end - data) / 8) as usize))
    }

    /// Read a u8 from the target process.
    pub fn read_u8(&self, addr: u64) -> Result<u8, String> {
        let buf = self.read_bytes(addr, 1)?;
        Ok(buf[0])
    }

    /// Write bytes to the target process at `addr`. The build write gate
    /// (`GameBuild::allows_writes`) is enforced where the handle is opened;
    /// a read-only OS handle refuses any write that reaches it anyway.
    pub fn write_bytes(&self, addr: u64, data: &[u8]) -> Result<(), String> {
        backend::write_bytes(&self.handle, addr, data)
    }

    /// Write a single u8 to the target process.
    pub fn write_u8(&self, addr: u64, val: u8) -> Result<(), String> {
        self.write_bytes(addr, &[val])
    }

    /// Read an FM name string: pointer → [u32 len][UTF-8 bytes].
    ///
    /// Reads the length prefix + name bytes in a single cross-process call
    /// (up to 260 bytes) instead of two separate calls, cutting round trips.
    pub fn read_fm_name(&self, name_ptr: u64) -> Result<String, String> {
        if name_ptr < 0x10000 || name_ptr > 0x7FFFFFFFFFFF {
            return Err("invalid name pointer".into());
        }
        // Single read: 4-byte length prefix + up to 256 bytes of name data.
        let buf = self.read_bytes(name_ptr, 4 + 256)?;
        let len = u32::from_le_bytes(buf[..4].try_into().unwrap()) as usize;
        if len == 0 || len > 256 {
            return Err(format!("bad name length: {len}"));
        }
        String::from_utf8(buf[4..4 + len].to_vec()).map_err(|e| format!("name UTF-8 error: {e}"))
    }
}

/// Determine the game version from the full path of `game_plugin.dll`.
///
/// The Football Manager install directory always contains the release year
/// (e.g. `…\Football Manager 2026\game_plugin.dll`), which is the only
/// reliable distinguisher when every version shares the `fm.exe` exe name.
fn sniff_version(dll_path: &str) -> Result<GameVersion, String> {
    let lower = dll_path.to_lowercase();
    // Match both "Football Manager 2026" and "Football Manager 26" naming conventions.
    if lower.contains("2027") || lower.contains("manager 27") || lower.contains("footballmanager27")
    {
        Ok(GameVersion::Fm27)
    } else if lower.contains("2026")
        || lower.contains("manager 26")
        || lower.contains("footballmanager26")
    {
        Ok(GameVersion::Fm26)
    } else if lower.contains("2024")
        || lower.contains("manager 24")
        || lower.contains("footballmanager24")
    {
        Ok(GameVersion::Fm24)
    } else {
        Err(format!(
            "cannot determine game version from game module path: {dll_path}"
        ))
    }
}

/// Project fingerprint of the first 64 KB of a module file.
/// The historical multiplier below differs from standard FNV-1a. Keep it
/// unchanged so existing calibrated schemas retain their compatibility gate.
///
/// Used as a lightweight build fingerprint to detect when FM has been
/// patched and the schema should be re-verified. A file that cannot be read
/// (e.g. a Store install under the access-restricted `WindowsApps` folder)
/// is an error naming the path, not a hash that merely fails to match.
fn hash_module_header(path: &str) -> Result<u64, String> {
    use std::io::Read;
    let mut bytes = Vec::with_capacity(65536);
    std::fs::File::open(path)
        .and_then(|f| f.take(65536).read_to_end(&mut bytes))
        .map_err(|e| format!("cannot read {path}: {e}"))?;
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in &bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000000001b3);
    }
    Ok(h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_game_label_names_build_and_pid() {
        let game = RunningGame {
            pid: 4242,
            build: GameBuild::Fm27Steam,
        };
        assert_eq!(game.to_string(), "FM27 (PID 4242)");
        let game = RunningGame {
            pid: 4242,
            build: GameBuild::Fm27GamePass,
        };
        assert_eq!(game.to_string(), "FM27 Game Pass (PID 4242)");
    }

    #[cfg(windows)]
    #[test]
    fn epic_write_passes_the_gate_and_the_read_only_os_handle_still_refuses() {
        let fm = FmProcess {
            handle: backend::open_read_only(std::process::id()).unwrap(),
            pid: std::process::id(),
            gp_base: 0,
            gp_end: 0,
            module_path: String::new(),
            build: GameBuild::Fm24Epic,
            dll_hash_notice: None,
        };
        let mut value = std::hint::black_box([0x12u8, 0x34]);
        let addr = value.as_mut_ptr() as u64;
        assert_eq!(fm.read_bytes(addr, 2).unwrap(), value);
        // The build gate is open after live verification, so the read-only
        // OS handle itself must be the barrier that refuses the write.
        let error = fm.write_bytes(addr, &[0, 0]).unwrap_err();
        assert!(!error.contains("no write was attempted"));
        assert_eq!(value, [0x12, 0x34]);
    }

    #[test]
    fn version_sniffed_per_install_path() {
        let fm24 = r"S:\SteamLibrary\steamapps\common\Football Manager 2024\fm.exe";
        let fm27 = r"S:\SteamLibrary\steamapps\common\Football Manager 27\data\game_plugin.dll";
        assert_eq!(sniff_version(fm24), Ok(GameVersion::Fm24));
        assert_eq!(sniff_version(fm27), Ok(GameVersion::Fm27));
        assert_eq!(
            sniff_version(r"C:\Program Files\Epic Games\FootballManager24\fm.exe"),
            Ok(GameVersion::Fm24)
        );
        assert_eq!(
            sniff_version(r"D:\Games\FootballManager26\game_plugin.dll"),
            Ok(GameVersion::Fm26)
        );
        assert_eq!(
            sniff_version(r"D:\Games\FootballManager27\game_plugin.dll"),
            Ok(GameVersion::Fm27)
        );
        assert!(sniff_version(r"C:\Games\fm.exe").is_err());
    }

    #[test]
    fn storefront_from_install_location() {
        let steam = r"S:\SteamLibrary\steamapps\common\Football Manager 27\fm.exe";
        let proton = "/home/u/.steam/steam/steamapps/common/Football Manager 27/fm.exe";
        let xbox = r"D:\XboxGames\Football Manager 27\Content\fm.exe";
        let store = r"C:\Program Files\WindowsApps\SportsInteractive.FM27_1.0.0.0_x64__abc\fm.exe";
        assert_eq!(storefront_of(steam, false), Storefront::Steam);
        assert_eq!(
            storefront_of(
                r"C:\Program Files\Epic Games\FootballManager24\fm.exe",
                false
            ),
            Storefront::Epic
        );
        assert_eq!(storefront_of(proton, false), Storefront::Steam);
        assert_eq!(storefront_of(xbox, false), Storefront::GamePass);
        assert_eq!(storefront_of(store, false), Storefront::GamePass);
        // A moved Game Pass install is still recognised by its GDK manifest.
        assert_eq!(
            storefront_of(r"E:\Games\FM27\fm.exe", true),
            Storefront::GamePass
        );
    }

    #[test]
    fn module_hash_reads_only_the_header() {
        let dir = std::env::temp_dir().join(format!("fm-editor-hash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let short = dir.join("short.bin");
        let long = dir.join("long.bin");
        std::fs::write(&short, vec![7u8; 65536]).unwrap();
        std::fs::write(&long, [vec![7u8; 65536], vec![9u8; 100]].concat()).unwrap();
        let h = |p: &std::path::Path| hash_module_header(p.to_str().unwrap());
        assert_eq!(h(&short), h(&long));
        let missing = h(&dir.join("missing.bin")).unwrap_err();
        assert!(missing.contains("missing.bin"), "{missing}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
