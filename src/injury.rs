//! Active-injury reading for FM24 Epic (player medical conditions).
//!
//! Healing clears BOTH vectors of the conditions object — the injury
//! record AND the absence entry ("player is out") that squad screens
//! check. Zeroing only the injury's day counts leaves the absence active
//! and the game keeps the player marked injured (observed live).
//!
//! Layout witnessed live on 2026-11-08 against Livan Burcu's recorded
//! ankle fracture ("ayak bile\u{11f}inde k\u{131}r\u{131}k", injury type id 15,
//! start 17 Sep 2026 = doy 260, 75 full + 52 light days): the player
//! struct points at a conditions object (+0xF8, NULL on healthy players)
//! whose first vector holds one pointer per active injury record. Each
//! record starts with [type ptr @+0x8][packed start date @+0x20]
//! [FFFFFFFF][full days u16 @+0x28][light days u16 @+0x2A]. Healing zeroes
//! the two day counts through the acknowledged compare-and-swap path.

use crate::cache::InjuryInfo;
use crate::process::FmProcess;

/// Offsets of the injury chain inside the player struct / record, per build.
pub struct InjuryLayout {
    /// Player-struct offset of the pointer to the conditions object.
    pub conditions_ptr: u64,
    /// Conditions-object offset of the vector of injury-record pointers.
    pub injuries_vec: u64,
    /// Conditions-object offset of the absence vector ("player is out"
    /// entries — what squad screens check). Clearing the injury record
    /// alone is NOT enough: this entry keeps the player marked injured.
    pub absences_vec: u64,
    /// Injury-record offset of the pointer to the injury-type object.
    pub type_ptr: u64,
    /// Injury-type-object offset of the FmString with the descriptive name.
    pub type_name: u64,
    /// Injury-record offset of the packed start date.
    pub start: u64,
    /// Injury-record offsets of the day counts.
    pub full_days: u64,
    pub light_days: u64,
}

fn valid(p: u64) -> bool {
    (0x10000..0x7FFF_FFFF_FFFF).contains(&p)
}

/// Read the raw 16-byte [begin][end] pair of a pointer vector.
fn read_vec_raw(fm: &FmProcess, addr: u64) -> Option<Vec<u8>> {
    let bytes = fm.read_bytes(addr, 16).ok()?;
    Some(bytes)
}

/// Parse the witnessed record head into its date and day fields.
pub fn parse_record_head(head: &[u8], layout: &InjuryLayout) -> Option<((u16, u16), u16, u16)> {
    let off = layout.start as usize;
    let doy = u16::from_le_bytes(head.get(off..off + 2)?.try_into().ok()?);
    let year = u16::from_le_bytes(head.get(off + 2..off + 4)?.try_into().ok()?);
    let fo = layout.full_days as usize;
    let full = u16::from_le_bytes(head.get(fo..fo + 2)?.try_into().ok()?);
    let lo = layout.light_days as usize;
    let light = u16::from_le_bytes(head.get(lo..lo + 2)?.try_into().ok()?);
    Some(((doy, year), full, light))
}

/// Read every active injury of one player whose full struct bytes are `raw`.
pub fn read_injuries(fm: &FmProcess, raw: &[u8], layout: &InjuryLayout) -> Vec<InjuryInfo> {
    let mut out = Vec::new();
    let cp = layout.conditions_ptr as usize;
    let Some(bytes) = raw.get(cp..cp + 8) else {
        return out;
    };
    let cond = u64::from_le_bytes(bytes.try_into().unwrap());
    if !valid(cond) {
        return out;
    }
    let Ok(block) = fm.read_bytes(cond + layout.injuries_vec, 24) else {
        return out;
    };
    let begin = u64::from_le_bytes(block[0..8].try_into().unwrap());
    let end = u64::from_le_bytes(block[8..16].try_into().unwrap());
    if !valid(begin) || end < begin || (end - begin) > 8 * 64 || (end - begin) % 8 != 0 {
        return out;
    }
    let count = ((end - begin) / 8) as usize;
    for i in 0..count {
        let Ok(slot) = fm.read_bytes(begin + i as u64 * 8, 8) else {
            continue;
        };
        let rec = u64::from_le_bytes(slot.try_into().unwrap());
        if !valid(rec) {
            continue;
        }
        let Ok(head) = fm.read_bytes(rec, 0x30) else {
            continue;
        };
        let Some((start, full_days, light_days)) = parse_record_head(&head, layout) else {
            continue;
        };
        let to = layout.type_ptr as usize;
        let Some(t) = head.get(to..to + 8) else {
            continue;
        };
        let type_ptr = u64::from_le_bytes(t.try_into().unwrap());
        let name = read_type_name(fm, type_ptr, layout);
        out.push(InjuryInfo {
            record_addr: rec,
            conditions_addr: cond,
            name,
            start_doy: start.0,
            start_year: start.1,
            full_days,
            light_days,
            injuries_vec_raw: read_vec_raw(fm, cond + layout.injuries_vec).unwrap_or_default(),
            absences_vec_raw: read_vec_raw(fm, cond + layout.absences_vec).unwrap_or_default(),
        });
    }
    out
}

/// Descriptive injury name, e.g. "ayak bile\u{11f}inde k\u{131}r\u{131}k" (falls back
/// to the raw type-id text when the string cannot be read).
fn read_type_name(fm: &FmProcess, type_ptr: u64, layout: &InjuryLayout) -> String {
    if !valid(type_ptr) {
        return format!("type @0x{type_ptr:X}");
    }
    let Ok(obj) = fm.read_bytes(type_ptr, 0x28) else {
        return format!("type @0x{type_ptr:X}");
    };
    let no = layout.type_name as usize;
    let Some(n) = obj.get(no..no + 8) else {
        return format!("type @0x{type_ptr:X}");
    };
    let str_ptr = u64::from_le_bytes(n.try_into().unwrap());
    if !valid(str_ptr) {
        return format!("type @0x{type_ptr:X}");
    }
    let Ok(len_bytes) = fm.read_bytes(str_ptr, 4) else {
        return format!("type @0x{type_ptr:X}");
    };
    let len = u32::from_le_bytes(len_bytes.try_into().unwrap()) as usize;
    if !(4..=64).contains(&len) {
        return format!("type @0x{type_ptr:X}");
    }
    fm.read_bytes(str_ptr + 4, len)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_else(|| format!("type @0x{type_ptr:X}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAYOUT: InjuryLayout = InjuryLayout {
        conditions_ptr: 0xF8,
        injuries_vec: 0x0,
        absences_vec: 0x70,
        type_ptr: 0x8,
        type_name: 0x20,
        start: 0x20,
        full_days: 0x28,
        light_days: 0x2A,
    };

    #[test]
    fn record_head_parses_the_witnessed_fracture() {
        let mut head = [0u8; 0x30];
        head[0x20..0x24].copy_from_slice(&[0x04, 0x6F, 0xEA, 0x07]); // doy 260, 2026, flags
        head[0x24..0x28].copy_from_slice(&[0xFF; 4]);
        head[0x28..0x2C].copy_from_slice(&[0x4B, 0x00, 0x34, 0x00]); // 75 / 52
        let ((doy_flags, year), full, light) = parse_record_head(&head, &LAYOUT).unwrap();
        assert_eq!((doy_flags & 0x1FF, year), (260, 2026)); // 17 Sep 2026
        assert_eq!((full, light), (75, 52));
    }
}
