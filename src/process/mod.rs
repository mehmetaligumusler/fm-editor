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

mod detection_log;
mod version_resource;

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
    let mut errors = Vec::new();
    for pid in pids {
        match detect_build(pid) {
            Ok(build) => games.push(RunningGame { pid, build }),
            Err(e) => {
                errors.push(format!("pid {pid}: {e}"));
            }
        }
    }
    if games.is_empty() {
        Err(errors.join("\n\n"))
    } else {
        Ok(games)
    }
}

/// Identify the build of one `fm.exe`.
///
/// Read the game module's PE product metadata first: FM24 embeds it in
/// `fm.exe`; FM26/FM27 use `game_plugin.dll` (their exe describes Unity).
/// The install directory is only a fallback when no FM identity is available.
/// Storefront detection is separate and does not establish layout compatibility.
fn detect_build(pid: u32) -> Result<GameBuild, String> {
    let (result, report) = detect_build_from_modules(
        pid,
        backend::find_module_info(pid, "fm.exe"),
        backend::find_module_info(pid, "game_plugin.dll"),
        version_resource::read,
    );
    detection_log::record(pid, &report);
    result.map_err(|e| format!("{e}\n{report}"))
}

struct InspectedModule {
    name: &'static str,
    path: String,
    metadata: Result<version_resource::VersionMetadata, String>,
}

fn inspect_module(
    name: &'static str,
    info: Result<(u64, u64, String), String>,
    read_metadata: &mut impl FnMut(&str) -> Result<version_resource::VersionMetadata, String>,
    report: &mut String,
) -> Option<InspectedModule> {
    use std::fmt::Write;
    match info {
        Ok((base, size, path)) => {
            let _ = writeln!(
                report,
                "module {name}: path={path:?}, base=0x{base:X}, size=0x{size:X}"
            );
            let metadata = read_metadata(&path);
            match &metadata {
                Ok(info) => {
                    let _ = writeln!(report, "  ProductName: {:?}", info.product_names);
                    let _ = writeln!(report, "  ProductVersion: {:?}", info.product_versions);
                    let _ = writeln!(report, "  FileVersion: {:?}", info.file_versions);
                }
                Err(e) => {
                    let _ = writeln!(report, "  PE version resource unavailable: {e}");
                }
            }
            Some(InspectedModule {
                name,
                path,
                metadata,
            })
        }
        Err(e) => {
            let _ = writeln!(report, "module {name}: discovery failed for this PID: {e}");
            None
        }
    }
}

/// Keep discovery, file metadata and selection in one process-local report.
/// Dependency injection lets tests prove that a missing plugin never borrows
/// another process's path or metadata, without starting or attaching to FM.
fn detect_build_from_modules(
    pid: u32,
    exe_info: Result<(u64, u64, String), String>,
    plugin_info: Result<(u64, u64, String), String>,
    mut read_metadata: impl FnMut(&str) -> Result<version_resource::VersionMetadata, String>,
) -> (Result<GameBuild, String>, String) {
    use std::fmt::Write;
    let mut report = format!("FM build detection: PID {pid}\n");
    let exe = inspect_module("fm.exe", exe_info, &mut read_metadata, &mut report);
    let plugin = inspect_module(
        "game_plugin.dll (optional)",
        plugin_info,
        &mut read_metadata,
        &mut report,
    );
    let result = (|| {
        let selected = plugin.as_ref().or(exe.as_ref()).ok_or_else(|| {
            "cannot identify game: neither game module nor executable could be located".to_string()
        })?;
        let _ = writeln!(
            report,
            "selected module: {} ({:?})",
            selected.name, selected.path
        );
        let storefront_path = exe.as_ref().map_or(&selected.path, |exe| &exe.path);
        let gdk = has_gdk_config(storefront_path);
        let storefront = storefront_of(storefront_path, gdk);
        let _ = writeln!(
            report,
            "storefront hint: {}; path={storefront_path:?}; MicrosoftGame.config present={gdk}",
            storefront.display_name()
        );
        let identity = selected.metadata.as_ref().ok().map(version_from_metadata);
        let source = if matches!(identity, Some(Ok(Some(_)))) {
            "PE ProductName"
        } else {
            let _ = writeln!(
                report,
                "PE identity unavailable or unsupported: {identity:?}; resource error: {:?}",
                selected.metadata.as_ref().err()
            );
            "install-directory fallback"
        };
        let version = identify_version(&selected.path, selected.metadata.clone())?;
        let _ = writeln!(
            report,
            "release: {}; source: {source}",
            version.display_name()
        );
        GameBuild::resolve(version, storefront)
    })();
    match &result {
        Ok(build) => {
            let _ = writeln!(
                report,
                "result: selected {} schema {}; layout compatibility still requires the attachment fingerprint gate",
                build.display_name(),
                build.schema_name()
            );
        }
        Err(e) => {
            let _ = writeln!(report, "result: {e}");
        }
    }
    (result, report)
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
/// packages, `Program Files\WindowsApps`. Recognised Epic installs must not
/// inherit Steam's schema. Other locations retain the historical Steam default;
/// metadata identifies the release, not the storefront or compatible offsets.
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

    /// (base, size) of committed private heap regions — used by save-name
    /// detection. Delegates to the platform backend.
    pub fn committed_private_regions(&self) -> Vec<(u64, usize)> {
        backend::committed_private_regions(&self.handle)
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

/// Recognise an FM product name, without interpreting engine version numbers.
/// An explicit unsupported or conflicting FM identity must not fall back to a
/// directory that happens to name a supported release.
fn version_from_metadata(
    metadata: &version_resource::VersionMetadata,
) -> Result<Option<GameVersion>, String> {
    let mut found = None;
    for name in &metadata.product_names {
        let lower = name.trim().to_ascii_lowercase();
        let Some(release) = lower.strip_prefix("football manager ") else {
            continue;
        };
        let version = match release {
            "24" | "2024" => GameVersion::Fm24,
            "26" | "2026" => GameVersion::Fm26,
            "27" | "2027" => GameVersion::Fm27,
            _ => return Err(format!("unsupported Football Manager product: {name}")),
        };
        if found.is_some_and(|previous| previous != version) {
            return Err("conflicting Football Manager product names in version resource".into());
        }
        found = Some(version);
    }
    Ok(found)
}

fn identify_version(
    module_path: &str,
    metadata: Result<version_resource::VersionMetadata, String>,
) -> Result<GameVersion, String> {
    let reason = match metadata {
        Ok(metadata) => match version_from_metadata(&metadata) {
            Ok(Some(version)) => return Ok(version),
            Ok(None) => format!(
                "no Football Manager product identity (product names: {:?}, product versions: {:?}, file versions: {:?})",
                metadata.product_names, metadata.product_versions, metadata.file_versions,
            ),
            Err(e) => return Err(format!("{e} ({module_path})")),
        },
        Err(e) => e,
    };
    sniff_version(module_path).map_err(|e| format!("{e}; version resource: {reason}"))
}

/// Legacy fallback for missing product metadata. Match an installation-directory
/// component, not an arbitrary year elsewhere in the path. Compact Epic-style
/// names and the spaced Steam names are both accepted.
fn sniff_version(module_path: &str) -> Result<GameVersion, String> {
    for component in module_path.split(['/', '\\']) {
        let compact: String = component
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
            .flat_map(char::to_lowercase)
            .collect();
        match compact.as_str() {
            "footballmanager24" | "footballmanager2024" => return Ok(GameVersion::Fm24),
            "footballmanager26" | "footballmanager2026" => return Ok(GameVersion::Fm26),
            "footballmanager27" | "footballmanager2027" => return Ok(GameVersion::Fm27),
            _ => {}
        }
    }
    Err(format!(
        "cannot determine game version from module path: {module_path}"
    ))
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
        assert_eq!(
            sniff_version(r"C:\Program Files\Epic Games\FootballManager24\fm.exe"),
            Ok(GameVersion::Fm24)
        );
        assert!(sniff_version(r"C:\2024\UnknownGame\fm.exe").is_err());
    }

    fn metadata(name: &str, version: &str) -> version_resource::VersionMetadata {
        version_resource::VersionMetadata {
            product_names: vec![name.into()],
            product_versions: vec![version.into()],
            file_versions: vec![],
        }
    }

    fn module_info(path: &str) -> Result<(u64, u64, String), String> {
        Ok((0x140000000, 0x100000, path.into()))
    }

    #[test]
    fn failed_detection_reports_only_this_process_modules_and_raw_metadata() {
        let path = r"C:\Custom\fm.exe";
        let (result, report) = detect_build_from_modules(
            25592,
            module_info(path),
            Err("module not mapped".into()),
            |requested| {
                assert_eq!(requested, path);
                let mut info = metadata("Unknown game", "99.2+123");
                info.file_versions.push("99.2".into());
                Ok(info)
            },
        );
        assert!(result.is_err());
        for expected in [
            "PID 25592",
            "module fm.exe:",
            "base=0x140000000",
            "size=0x100000",
            "Unknown game",
            "99.2+123",
            "FileVersion: [\"99.2\"]",
            "game_plugin.dll (optional): discovery failed for this PID: module not mapped",
            "selected module: fm.exe",
            "cannot determine game version",
        ] {
            assert!(report.contains(expected), "missing {expected:?}: {report}");
        }
        assert!(!report.contains("FM27"), "{report}");
        assert!(!report.contains("Football Manager 27"), "{report}");
    }

    #[test]
    fn detection_report_retains_metadata_when_schema_is_unsupported() {
        let path = r"C:\Program Files\Epic Games\FootballManager24\fm.exe";
        let (result, report) =
            detect_build_from_modules(1, module_info(path), Err("not mapped".into()), |_| {
                Ok(metadata("Football Manager 2024", "24.4.2+2081827"))
            });
        // Epic now carries the live-verified Fm24Epic schema.
        assert_eq!(result.unwrap(), GameBuild::Fm24Epic);
        assert!(report.contains("24.4.2+2081827"), "{report}");
        assert!(
            report.contains("release: FM24; source: PE ProductName"),
            "{report}"
        );
        assert!(report.contains("storefront hint: Epic"), "{report}");
    }

    #[test]
    fn detection_report_explains_successful_directory_fallback() {
        let (result, report) = detect_build_from_modules(
            2,
            module_info("/Football Manager 2024/fm.exe"),
            Err("not mapped".into()),
            |_| Err("permission denied reading resource".into()),
        );
        assert_eq!(result, Ok(GameBuild::Fm24Steam));
        assert!(
            report.contains("permission denied reading resource"),
            "{report}"
        );
        assert!(
            report.contains("source: install-directory fallback"),
            "{report}"
        );
        assert!(
            report.contains("layout compatibility still requires"),
            "{report}"
        );
    }

    #[test]
    fn detection_report_distinguishes_unity_exe_from_fm_plugin() {
        let (result, report) = detect_build_from_modules(
            3,
            module_info("/custom/fm.exe"),
            module_info("/custom/game_plugin.dll"),
            |path| {
                if path.ends_with(".exe") {
                    Ok(metadata("Unity", "6000.3.18f1"))
                } else {
                    Ok(metadata("Football Manager 27", "27.0.0+2493426"))
                }
            },
        );
        assert_eq!(result, Ok(GameBuild::Fm27Steam));
        assert!(
            report.contains("Unity") && report.contains("6000.3.18f1"),
            "{report}"
        );
        assert!(
            report.contains("selected module: game_plugin.dll"),
            "{report}"
        );
        assert!(report.contains("27.0.0+2493426"), "{report}");
    }

    #[test]
    fn failed_module_discovery_retains_both_errors_without_reading_files() {
        let (result, report) = detect_build_from_modules(
            4,
            Err("exe enumeration denied".into()),
            Err("plugin not mapped".into()),
            |_| panic!("must not read an undiscovered module"),
        );
        assert!(result.is_err());
        assert!(
            report.contains("exe enumeration denied") && report.contains("plugin not mapped"),
            "{report}"
        );
        assert!(!report.contains("selected module:"), "{report}");
    }

    #[test]
    fn conflicting_identity_report_retains_all_product_versions() {
        let (result, report) = detect_build_from_modules(
            5,
            module_info("/Football Manager 2024/fm.exe"),
            Err("not mapped".into()),
            |_| {
                let mut info = metadata("Football Manager 2024", "24.4.2");
                info.product_names.push("Football Manager 27".into());
                info.product_versions.push("27.0.0".into());
                Ok(info)
            },
        );
        assert!(result.unwrap_err().contains("conflicting"));
        assert!(
            report.contains("24.4.2") && report.contains("27.0.0"),
            "{report}"
        );
        assert!(
            !report.contains("source: install-directory fallback"),
            "{report}"
        );
    }

    #[test]
    #[ignore = "requires FM_VERSION_TEST_MODULE pointing to an installed game module"]
    fn diagnose_installed_module_from_metadata() {
        let path = std::env::var("FM_VERSION_TEST_MODULE").expect("set FM_VERSION_TEST_MODULE");
        let info = version_resource::read(&path);
        let result = identify_version(&path, info.clone());
        let report = format!(
            "FILE-ONLY PE diagnostic (no process/module discovery or memory reads); test-runner PID {}\npath: {path:?}\nmetadata: {info:#?}\nrelease: {result:?}",
            std::process::id()
        );
        println!("{report}");
        detection_log::record(std::process::id(), &report);
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn product_metadata_identifies_release_without_install_directory() {
        let fm24 = metadata("Football Manager 2024", "24.4.2+2081827");
        assert_eq!(
            identify_version(r"C:\Custom\fm.exe", Ok(fm24)),
            Ok(GameVersion::Fm24)
        );
        let fm27 = metadata("Football Manager 27", "27.0.0+2493426");
        // Metadata wins over a stale or misleading directory name.
        assert_eq!(
            identify_version("/Football Manager 2024/game_plugin.dll", Ok(fm27)),
            Ok(GameVersion::Fm27)
        );
        assert_eq!(
            version_from_metadata(&metadata("Football Manager 2026", "26.3.2")),
            Ok(Some(GameVersion::Fm26))
        );
    }

    #[test]
    fn missing_or_generic_metadata_uses_path_fallback() {
        let path = r"C:\Epic Games\FootballManager24\fm.exe";
        assert_eq!(
            identify_version(path, Err("resource missing".into())),
            Ok(GameVersion::Fm24)
        );
        // Unity's version number is not an FM release.
        assert_eq!(
            version_from_metadata(&metadata("Unity", "6000.3.18f1")),
            Ok(None)
        );
        assert_eq!(
            identify_version(
                "/Football Manager 27/fm.exe",
                Ok(metadata("Unity", "6000.3.18f1"))
            ),
            Ok(GameVersion::Fm27)
        );
        let err = identify_version("/custom/fm.exe", Err("cannot read file".into())).unwrap_err();
        assert!(
            err.contains("module path") && err.contains("cannot read file"),
            "{err}"
        );
        assert!(!err.contains("game_plugin.dll"), "{err}");
    }

    #[test]
    fn explicit_unsupported_or_conflicting_metadata_never_uses_path_fallback() {
        let path = "/Football Manager 2024/fm.exe";
        assert!(
            identify_version(path, Ok(metadata("Football Manager 2025", "25.0")))
                .unwrap_err()
                .contains("unsupported")
        );
        let mut conflict = metadata("Football Manager 2024", "24.4.2");
        conflict.product_names.push("Football Manager 27".into());
        assert!(
            identify_version(path, Ok(conflict))
                .unwrap_err()
                .contains("conflicting")
        );
    }

    #[test]
    #[ignore = "requires FM_VERSION_TEST_MODULE pointing to an installed game module"]
    fn identify_installed_module_from_metadata() {
        let path = std::env::var("FM_VERSION_TEST_MODULE").expect("set FM_VERSION_TEST_MODULE");
        let info = version_resource::read(&path).expect("read version resource");
        let version = version_from_metadata(&info)
            .expect("unambiguous FM identity")
            .expect("FM product name");
        println!("{path}: {info:?}; identified {}", version.display_name());
        // A neutral path proves the result is independent of the install folder.
        assert_eq!(identify_version("/neutral/game.bin", Ok(info)), Ok(version));
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
        let epic = r"C:\Program Files\Epic Games\FootballManager24\fm.exe";
        assert_eq!(storefront_of(epic, false), Storefront::Epic);
        // The Epic build carries the live-verified Fm24Epic schema.
        assert_eq!(
            GameBuild::resolve(GameVersion::Fm24, storefront_of(epic, false)),
            Ok(GameBuild::Fm24Epic)
        );
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
