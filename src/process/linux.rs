//! Linux backend: /proc scanning + process_vm_readv/process_vm_writev.
//!
//! The target is the Windows fm.exe running under Proton/Wine. Wine mmaps
//! PE modules file-backed, so game_plugin.dll shows up in /proc/<pid>/maps
//! with its full native path — module discovery is a maps parse, and the
//! path feeds the same version sniff / DLL-hash gate as on Windows.
//!
//! Cross-process access needs ptrace rights on the target. With
//! kernel.yama.ptrace_scope=1 (the common default) even same-user reads
//! are denied; read/write errors carry the fix as a hint.

use std::ffi::c_void;

/// Just the pid — process_vm_* take a pid per call, there is no handle to own.
pub struct ProcHandle {
    pid: libc::pid_t,
}

/// Open the process for memory access.
///
/// Nothing to acquire on Linux; permission problems surface on the first
/// read (the attach path probes one immediately).
pub fn open(pid: u32) -> Result<ProcHandle, String> {
    Ok(ProcHandle {
        pid: pid as libc::pid_t,
    })
}

/// Linux has no acquired process handle; the shared FmProcess write gate
/// enforces read-only builds before process_vm_writev can be reached.
pub fn open_read_only(pid: u32) -> Result<ProcHandle, String> {
    open(pid)
}

/// Read `n` bytes from the target process at `addr`.
pub fn read_bytes(h: &ProcHandle, addr: u64, n: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; n];
    let local = libc::iovec {
        iov_base: buf.as_mut_ptr() as *mut c_void,
        iov_len: n,
    };
    let remote = libc::iovec {
        iov_base: addr as *mut c_void,
        iov_len: n,
    };
    let got = unsafe { libc::process_vm_readv(h.pid, &local, 1, &remote, 1, 0) };
    if got == n as isize {
        Ok(buf)
    } else {
        Err(transfer_error("readv", addr, got))
    }
}

/// Write bytes to the target process at `addr`.
pub fn write_bytes(h: &ProcHandle, addr: u64, data: &[u8]) -> Result<(), String> {
    let local = libc::iovec {
        iov_base: data.as_ptr() as *mut c_void,
        iov_len: data.len(),
    };
    let remote = libc::iovec {
        iov_base: addr as *mut c_void,
        iov_len: data.len(),
    };
    let put = unsafe { libc::process_vm_writev(h.pid, &local, 1, &remote, 1, 0) };
    if put == data.len() as isize {
        Ok(())
    } else {
        Err(transfer_error("writev", addr, put))
    }
}

fn transfer_error(op: &str, addr: u64, transferred: isize) -> String {
    if transferred < 0 {
        let err = std::io::Error::last_os_error();
        let hint = if err.raw_os_error() == Some(libc::EPERM) {
            "\nhint: cross-process access is blocked (kernel.yama.ptrace_scope). \
             Run `sudo sysctl kernel.yama.ptrace_scope=0`, or grant the editor \
             CAP_SYS_PTRACE: `sudo setcap cap_sys_ptrace+ep <binary>` (must be \
             re-applied after every rebuild)."
        } else {
            ""
        };
        format!("process_vm_{op} at 0x{addr:X}: {err}{hint}")
    } else {
        // Partial transfer: the range crosses into an unmapped page. Treat
        // like ReadProcessMemory does — whole call fails.
        format!("process_vm_{op} at 0x{addr:X}: partial transfer ({transferred} bytes)")
    }
}

/// Find every process with this name, return their PIDs in ascending order.
///
/// Matches /proc/<pid>/comm, which the Wine loader usually sets to the exe
/// name (truncated to 15 chars by the kernel). Some games rename their main
/// thread instead (FM24's comm is "Main Thread"), so the basename of argv[0]
/// — a Windows path like `S:\…\fm.exe` under Proton — is matched too.
pub fn find_pids(name: &str) -> Result<Vec<u32>, String> {
    let target = name.to_lowercase();
    let truncated: String = target.chars().take(15).collect();
    let entries = std::fs::read_dir("/proc").map_err(|e| format!("read /proc: {e}"))?;
    let mut pids = Vec::new();
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        if let Ok(comm) = std::fs::read_to_string(format!("/proc/{pid}/comm")) {
            let comm = comm.trim().to_lowercase();
            if comm == target || comm == truncated {
                pids.push(pid);
                continue;
            }
        }
        if let Ok(cmdline) = std::fs::read(format!("/proc/{pid}/cmdline")) {
            let argv0 = cmdline.split(|&b| b == 0).next().unwrap_or(&[]);
            let argv0 = String::from_utf8_lossy(argv0).to_lowercase();
            if argv0.rsplit(['/', '\\']).next() == Some(target.as_str()) {
                pids.push(pid);
            }
        }
    }
    pids.sort_unstable();
    Ok(pids)
}

/// Find a module's base address, size, and full on-disk path within a process.
///
/// Wine maps the PE header page file-backed, but may load the sections into
/// anonymous memory (it does for FM27's game_plugin.dll, whose file alignment
/// is below the page size), so the file-backed span can be a single page.
/// The size is therefore the larger of that span and the image's
/// `SizeOfImage` from the on-disk PE header, mirroring Toolhelp's modBaseSize.
pub fn find_module_info(pid: u32, module_name: &str) -> Result<(u64, u64, String), String> {
    let maps = std::fs::read_to_string(format!("/proc/{pid}/maps"))
        .map_err(|e| format!("read /proc/{pid}/maps: {e}"))?;
    let target = module_name.to_lowercase();
    let mut base = u64::MAX;
    let mut end = 0u64;
    let mut path = String::new();
    for line in maps.lines() {
        // "start-end perms offset dev inode      /full/path" — the path may
        // contain spaces ("Football Manager 26"), so take from the first '/'.
        let Some(slash) = line.find('/') else {
            continue;
        };
        let p = &line[slash..];
        let file_name = p.rsplit('/').next().unwrap_or("");
        if file_name.to_lowercase() != target {
            continue;
        }
        let range = line.split_whitespace().next().unwrap_or("");
        let Some((s, e)) = range.split_once('-') else {
            continue;
        };
        let (Ok(s), Ok(e)) = (u64::from_str_radix(s, 16), u64::from_str_radix(e, 16)) else {
            continue;
        };
        base = base.min(s);
        end = end.max(e);
        if path.is_empty() {
            path = p.to_string();
        }
    }
    if path.is_empty() {
        return Err(format!(
            "{module_name} not found in /proc/{pid}/maps — is the game past the launcher?"
        ));
    }
    let size = (end - base).max(pe_size_of_image(&path).unwrap_or(0));
    Ok((base, size, path))
}

/// `SizeOfImage` from a PE file's optional header, or `None` if the file
/// cannot be read or is not a PE image.
fn pe_size_of_image(path: &str) -> Option<u64> {
    use std::io::Read;
    let mut head = [0u8; 4096];
    std::fs::File::open(path).ok()?.read_exact(&mut head).ok()?;
    let pe = u32::from_le_bytes(head.get(0x3C..0x40)?.try_into().ok()?) as usize;
    if head.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    // COFF header is 20 bytes after the signature; SizeOfImage sits at
    // offset 56 of the optional header for both PE32 and PE32+.
    let at = pe + 4 + 20 + 56;
    Some(u32::from_le_bytes(head.get(at..at + 4)?.try_into().ok()?) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Self-access via process_vm_* is always permitted (no ptrace_scope
    // restriction), so these exercise the real syscalls without needing a
    // running game.

    #[test]
    fn read_own_memory() {
        let h = open(std::process::id()).unwrap();
        let data = std::hint::black_box([1u8, 2, 3, 4, 5, 6, 7, 8]);
        let got = read_bytes(&h, data.as_ptr() as u64, 8).unwrap();
        assert_eq!(got, data);
    }

    #[test]
    fn write_own_memory() {
        let h = open(std::process::id()).unwrap();
        let mut data = std::hint::black_box([0u8; 4]);
        write_bytes(&h, data.as_mut_ptr() as u64, &[9, 8, 7, 6]).unwrap();
        assert_eq!(std::hint::black_box(data), [9, 8, 7, 6]);
    }

    #[test]
    fn read_unmapped_address_errors() {
        let h = open(std::process::id()).unwrap();
        assert!(read_bytes(&h, 0x10, 8).is_err());
    }

    #[test]
    fn find_module_in_own_maps() {
        // The test binary links glibc dynamically; find it in our own maps
        // the same way attach() finds game_plugin.dll in fm.exe's.
        let (base, size, path) = find_module_info(std::process::id(), "libc.so.6").unwrap();
        assert!(base > 0);
        assert!(size > 0);
        assert!(path.ends_with("libc.so.6"), "path: {path}");
    }

    #[test]
    fn find_own_pid_by_comm() {
        let comm = std::fs::read_to_string("/proc/self/comm").unwrap();
        let pids = find_pids(comm.trim()).unwrap();
        assert!(pids.contains(&std::process::id()), "pids: {pids:?}");
    }
}
