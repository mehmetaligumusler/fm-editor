//! Windows backend: Toolhelp snapshots + ReadProcessMemory/WriteProcessMemory.

use std::ffi::c_void;
use std::mem;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW, PROCESSENTRY32W,
    Process32FirstW, Process32NextW, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Memory::{
    MEM_COMMIT, MEM_IMAGE, MEM_MAPPED, MEM_PRIVATE, MEMORY_BASIC_INFORMATION, PAGE_GUARD,
    PAGE_NOACCESS, VirtualQueryEx,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_ALL_ACCESS, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
};

/// Owns the Win32 process handle; closed on drop.
pub struct ProcHandle {
    handle: HANDLE,
}

impl Drop for ProcHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

/// Open the process for memory access.
pub fn open(pid: u32) -> Result<ProcHandle, String> {
    let handle = unsafe {
        OpenProcess(PROCESS_ALL_ACCESS, false, pid)
            .map_err(|e| format!("OpenProcess failed: {e}"))?
    };
    Ok(ProcHandle { handle })
}

/// (base, size) of every committed, readable, non-image private region —
/// the heap areas where FM keeps game strings (save names, reports, ...).
pub fn committed_private_regions(h: &ProcHandle) -> Vec<(u64, usize)> {
    let mut out = Vec::new();
    let mut addr: usize = 0x10000;
    loop {
        let mut mbi = MEMORY_BASIC_INFORMATION::default();
        let ok = unsafe {
            VirtualQueryEx(
                h.handle,
                Some(addr as *const c_void),
                &mut mbi,
                mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if ok == 0 {
            break;
        }
        let base = mbi.BaseAddress as usize;
        let size = mbi.RegionSize;
        let next = base.saturating_add(size.max(0x1000));
        let readable = mbi.State.0 == MEM_COMMIT.0
            && mbi.Type.0 != MEM_IMAGE.0
            && mbi.Type.0 != MEM_MAPPED.0
            && (mbi.Protect & PAGE_GUARD).0 == 0
            && mbi.Protect.0 != PAGE_NOACCESS.0;
        if readable && size > 0 && size <= 0x4000_0000 {
            out.push((base as u64, size));
        }
        if next <= addr {
            break;
        }
        addr = next;
        if addr >= 0x7FFF_FFFF_FFFF {
            break;
        }
    }
    out
}

/// Read `n` bytes from the target process at `addr`.

/// Read-only builds never acquire VM_WRITE or VM_OPERATION rights.
pub fn open_read_only(pid: u32) -> Result<ProcHandle, String> {
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
            false,
            pid,
        )
        .map_err(|e| format!("OpenProcess (read-only) failed: {e}"))?
    };
    Ok(ProcHandle { handle })
}

/// Read `n` bytes from the target process at `addr`.
pub fn read_bytes(h: &ProcHandle, addr: u64, n: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; n];
    let mut bytes_read = 0usize;
    unsafe {
        ReadProcessMemory(
            h.handle,
            addr as *const c_void,
            buf.as_mut_ptr() as *mut c_void,
            n,
            Some(&mut bytes_read),
        )
        .map_err(|e| format!("ReadProcessMemory at 0x{addr:X}: {e}"))?;
    }
    if bytes_read != n {
        return Err(format!(
            "ReadProcessMemory at 0x{addr:X}: short read ({bytes_read}/{n})"
        ));
    }
    Ok(buf)
}

/// Write bytes to the target process at `addr`.
pub fn write_bytes(h: &ProcHandle, addr: u64, data: &[u8]) -> Result<(), String> {
    let mut bytes_written = 0usize;
    unsafe {
        WriteProcessMemory(
            h.handle,
            addr as *const c_void,
            data.as_ptr() as *const c_void,
            data.len(),
            Some(&mut bytes_written),
        )
        .map_err(|e| format!("WriteProcessMemory at 0x{addr:X}: {e}"))?;
    }
    if bytes_written != data.len() {
        return Err(format!(
            "Short write at 0x{addr:X}: {bytes_written}/{} bytes",
            data.len()
        ));
    }
    Ok(())
}

/// Find every process with this name, return their PIDs in ascending order.
pub fn find_pids(name: &str) -> Result<Vec<u32>, String> {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
            .map_err(|e| format!("CreateToolhelp32Snapshot: {e}"))?;

        let mut entry = PROCESSENTRY32W {
            dwSize: mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        let target = name.to_lowercase();
        let mut pids = Vec::new();
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let exe = String::from_utf16_lossy(
                    &entry.szExeFile[..entry
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szExeFile.len())],
                );
                if exe.to_lowercase() == target {
                    pids.push(entry.th32ProcessID);
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        pids.sort_unstable();
        Ok(pids)
    }
}

/// Find a module's base address, size, and full on-disk path within a process.
pub fn find_module_info(pid: u32, module_name: &str) -> Result<(u64, u64, String), String> {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)
            .map_err(|e| format!("CreateToolhelp32Snapshot (module): {e}"))?;

        let mut entry = MODULEENTRY32W {
            dwSize: mem::size_of::<MODULEENTRY32W>() as u32,
            ..Default::default()
        };

        let target = module_name.to_lowercase();
        if Module32FirstW(snap, &mut entry).is_ok() {
            loop {
                let name = String::from_utf16_lossy(
                    &entry.szModule[..entry
                        .szModule
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szModule.len())],
                );
                if name.to_lowercase() == target {
                    let path = String::from_utf16_lossy(
                        &entry.szExePath[..entry
                            .szExePath
                            .iter()
                            .position(|&c| c == 0)
                            .unwrap_or(entry.szExePath.len())],
                    );
                    let _ = CloseHandle(snap);
                    return Ok((entry.modBaseAddr as u64, entry.modBaseSize as u64, path));
                }
                if Module32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        Err(format!("{module_name} not found in pid {pid}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_handle_reads_but_os_refuses_writes_to_test_process() {
        let handle = open_read_only(std::process::id()).unwrap();
        let mut bytes = std::hint::black_box([0xA5u8, 0x1A, 0xFF, 0x07]);
        let addr = bytes.as_mut_ptr() as u64;
        assert_eq!(read_bytes(&handle, addr, bytes.len()).unwrap(), bytes);
        assert!(write_bytes(&handle, addr, &[0; 4]).is_err());
        assert_eq!(bytes, [0xA5, 0x1A, 0xFF, 0x07]);
    }

    #[test]
    fn reads_exact_bytes_from_the_test_process_and_rejects_unmapped_memory() {
        let handle = open(std::process::id()).unwrap();
        let bytes = std::hint::black_box([0xA5u8, 0x1A, 0xFF, 0x07, 0x40, 0x04]);
        assert_eq!(
            read_bytes(&handle, bytes.as_ptr() as u64, bytes.len()).unwrap(),
            bytes
        );
        assert!(read_bytes(&handle, 0x10, 8).is_err());
    }
}
