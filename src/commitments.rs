//! Locate a club's debt records (bank loan + grant) in the heap.
//!
//! FM stores club commitments in byte-packed buffers that live OUTSIDE the
//! club object, with no pointer path from the club — but every copy embeds a
//! finance summary whose balance matches the live `finance+0x14` value, and
//! the record layout around it is fixed (witnessed live 2026-11-18 on the
//! user's Beşiktaş career, five copies verified):
//!
//!   summary.balance (i32 £)   @ loan − 0x49E
//!   grant amount (i32 £)      @ loan − 0x3FB
//!   loan amount (i32 £)       @ loan
//!   due date (u16 day, year)  @ loan + 0x10  (day ~152 = 1 June, year ≥ 2024)
//!
//! The game's Borçlar screen reads one of these copies; editing every
//! verified copy in one action (CAS per write) is the strategy proven by the
//! live -1M £ test that moved the screen's "174M €" row.

use eframe::egui;

use crate::process::FmProcess;

/// One located copy of the club's debt pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DebtCluster {
    pub loan_addr: u64,
    pub grant_addr: u64,
    pub loan: i32,
    pub grant: i32,
}

/// Byte distance from the embedded balance to the loan amount.
const BALANCE_TO_LOAN: usize = 0x49E;
/// Byte distance from the loan amount back to the grant amount.
const LOAN_TO_GRANT: usize = 0x3FB;
/// Offset of the due-date u16 pair from the loan amount.
const LOAN_DUE_DATE: usize = 0x10;
/// Offset of the start-date u16 pair from the loan amount (witnessed:
/// (152, 2022) — 1 June 2022, matching the screen's BAŞLANGIÇ column).
const LOAN_START_DATE: usize = 0x14;

/// Scan one memory block for verified debt clusters. Exposed for tests.
pub fn clusters_in(data: &[u8], base: u64, balance: i32) -> Vec<DebtCluster> {
    let needle = balance.to_le_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = find_from(data, &needle, from) {
        from = rel + 1;
        let Some(loan_off) = rel.checked_add(BALANCE_TO_LOAN) else {
            continue;
        };
        let Some(grant_off) = loan_off.checked_sub(LOAN_TO_GRANT) else {
            continue;
        };
        if loan_off + LOAN_START_DATE + 4 > data.len() || grant_off + 4 > data.len() {
            continue;
        }
        let loan = i32::from_le_bytes(data[loan_off..loan_off + 4].try_into().unwrap());
        let grant = i32::from_le_bytes(data[grant_off..grant_off + 4].try_into().unwrap());
        // Structural verification: plausible money, and the June-ish due
        // date sits right after the loan amount with the start date after it.
        let date_ok = |off: usize, years: std::ops::RangeInclusive<u16>| {
            let day = u16::from_le_bytes(data[off..off + 2].try_into().unwrap());
            let year = u16::from_le_bytes(data[off + 2..off + 4].try_into().unwrap());
            (140..=170).contains(&day) && years.contains(&year)
        };
        if !(1_000_000..=0x4000_0000).contains(&loan) || !(0..=0x4000_0000).contains(&grant) {
            continue;
        }
        if !date_ok(loan_off + LOAN_DUE_DATE, 2024..=2100)
            || !date_ok(loan_off + LOAN_START_DATE, 2000..=2026)
        {
            continue;
        }
        out.push(DebtCluster {
            loan_addr: base + loan_off as u64,
            grant_addr: base + grant_off as u64,
            loan,
            grant,
        });
    }
    out
}

/// The game keeps every real copy of the debts in sync, so the true
/// (loan, grant) pair is the one the majority agrees on. Strays with other
/// values are scan coincidences (UI caches echoing the amounts, stale
/// snapshots) — drop them, and drop everything when no pair reaches two
/// copies (a single "copy" is not trustworthy enough to write to).
pub fn majority_clusters(mut clusters: Vec<DebtCluster>) -> Vec<DebtCluster> {
    use std::collections::HashMap;
    let mut counts: HashMap<(i32, i32), usize> = HashMap::new();
    for c in &clusters {
        *counts.entry((c.loan, c.grant)).or_default() += 1;
    }
    let Some((_, best)) = counts.iter().max_by_key(|(_, n)| **n) else {
        return Vec::new();
    };
    if *best < 2 {
        return Vec::new();
    }
    clusters.retain(|c| counts[&(c.loan, c.grant)] == *best);
    clusters
}

fn find_from(data: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= data.len() {
        return None;
    }
    data[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// Scan the process heap for every verified copy of the club's debts.
/// `balance` must be the live `finance+0x14` value — the same number the
/// game keeps refreshing inside every summary copy.
pub fn find_debt_clusters(fm: &FmProcess, balance: i32) -> Vec<DebtCluster> {
    let mut out = Vec::new();
    for (base, size) in fm.committed_private_regions() {
        if size > 0x400_0000 {
            continue;
        }
        let Ok(data) = fm.read_bytes(base, size) else {
            continue;
        };
        out.extend(clusters_in(&data, base, balance));
    }
    majority_clusters(out)
}

// ─── egui plumbing (same pattern as overlay.rs) ──────────────────────────────

/// What the finance panel needs to know about the debt scan this frame.
#[derive(Clone, Default)]
pub struct DebtsView {
    pub clusters: Vec<DebtCluster>,
    pub resolved: bool,
    pub resolving: bool,
}

fn debts_id() -> egui::Id {
    egui::Id::new("fm_debts")
}

fn resolve_request_id() -> egui::Id {
    egui::Id::new("fm_debts_resolve_request")
}

/// Publish the debt-scan state for this frame (call from the app loop).
pub fn publish_debts(ctx: &egui::Context, view: &DebtsView) {
    ctx.data_mut(|d| d.insert_temp(debts_id(), view.clone()));
}

/// The debt-scan state published this frame, if any.
pub fn published_debts(ui: &egui::Ui) -> DebtsView {
    ui.ctx()
        .data(|d| d.get_temp::<DebtsView>(debts_id()))
        .unwrap_or_default()
}

/// Ask the app to (re)run the heap scan for the open club's debts.
pub fn request_resolve_debts(ui: &egui::Ui) {
    ui.ctx().data_mut(|d| d.insert_temp(resolve_request_id(), true));
}

/// Consume a pending resolve request (call once per frame from the app loop).
pub fn take_resolve_request(ctx: &egui::Context) -> bool {
    ctx.data_mut(|d| d.remove_temp::<bool>(resolve_request_id()).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(v: i32) -> [u8; 4] {
        v.to_le_bytes()
    }

    #[test]
    fn clusters_accept_the_witnessed_layout_and_reject_noise() {
        // Synthetic block mirroring the live Beşiktaş layout.
        let mut buf = vec![0u8; 0x600];
        let balance_at = 0x100;
        let loan_at = balance_at + BALANCE_TO_LOAN;
        let grant_at = loan_at - LOAN_TO_GRANT;
        buf[balance_at..balance_at + 4].copy_from_slice(&pack(22_276_280));
        buf[grant_at..grant_at + 4].copy_from_slice(&pack(78_118_144));
        buf[loan_at..loan_at + 4].copy_from_slice(&pack(149_509_056));
        buf[loan_at + LOAN_DUE_DATE..loan_at + LOAN_DUE_DATE + 2]
            .copy_from_slice(&152u16.to_le_bytes());
        buf[loan_at + LOAN_DUE_DATE + 2..loan_at + LOAN_DUE_DATE + 4]
            .copy_from_slice(&2030u16.to_le_bytes());
        buf[loan_at + LOAN_START_DATE..loan_at + LOAN_START_DATE + 2]
            .copy_from_slice(&152u16.to_le_bytes());
        buf[loan_at + LOAN_START_DATE + 2..loan_at + LOAN_START_DATE + 4]
            .copy_from_slice(&2022u16.to_le_bytes());

        // A decoy: same balance value but no valid structure after it.
        let decoy = 0x40;
        buf[decoy..decoy + 4].copy_from_slice(&pack(22_276_280));

        let found = clusters_in(&buf, 0x1000, 22_276_280);
        assert_eq!(
            found,
            vec![DebtCluster {
                loan_addr: 0x1000 + loan_at as u64,
                grant_addr: 0x1000 + grant_at as u64,
                loan: 149_509_056,
                grant: 78_118_144,
            }]
        );
        // A different balance finds nothing.
        assert!(clusters_in(&buf, 0x1000, 1).is_empty());
    }

    #[test]
    fn majority_keeps_the_synced_pair_and_drops_coincidences() {
        let cluster = |loan: i32, grant: i32| DebtCluster {
            loan_addr: 0x1000,
            grant_addr: 0xC00,
            loan,
            grant,
        };
        // Three real copies agree; two strays disagree; a lone pair is not
        // trustworthy at all.
        let real = cluster(149_509_056, 78_118_144);
        let mixed = vec![
            real,
            real,
            real,
            cluster(149_509_056, 0),
            cluster(1, 2),
        ];
        assert_eq!(majority_clusters(mixed), vec![real, real, real]);
        assert!(majority_clusters(vec![cluster(1, 2)]).is_empty());
        assert!(majority_clusters(Vec::new()).is_empty());
    }

    #[test]
    fn clusters_reject_implausible_dates_and_amounts() {
        let mut buf = vec![0u8; 0x600];
        let balance_at = 0x100;
        let loan_at = balance_at + BALANCE_TO_LOAN;
        buf[balance_at..balance_at + 4].copy_from_slice(&pack(5_000_000));
        buf[loan_at..loan_at + 4].copy_from_slice(&pack(50_000_000));
        // Wrong day-of-year (1 Jan) with a fine year.
        buf[loan_at + LOAN_DUE_DATE..loan_at + LOAN_DUE_DATE + 2]
            .copy_from_slice(&1u16.to_le_bytes());
        buf[loan_at + LOAN_DUE_DATE + 2..loan_at + LOAN_DUE_DATE + 4]
            .copy_from_slice(&2030u16.to_le_bytes());
        assert!(clusters_in(&buf, 0, 5_000_000).is_empty());
    }
}
