//! Detect the currently loaded save-game name from the running FM process.
//!
//! FM keeps the active save's name as plain NUL-terminated UTF-8 in private
//! heap memory (e.g. "Ana save.fm"). We scan committed private regions for
//! `*.fm` candidates and score them: candidates whose stem exists as a file
//! in the user's save-games folder win (the active save must be one of those
//! files), with the most recently written file preferred over others.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::process::FmProcess;

/// Extract `*.fm` save-name candidates from a NUL-separated memory block.
pub fn candidates_in(data: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    for chunk in data.split(|&b| b == 0) {
        if chunk.len() < 6 || chunk.len() > 68 || !chunk.ends_with(b".fm") {
            continue;
        }
        if let Ok(text) = std::str::from_utf8(chunk) {
            if text
                .chars()
                .all(|c| !c.is_control() && (c.is_alphanumeric() || " ._-()".contains(c)))
            {
                out.push(text.to_string());
            }
        }
    }
    out
}

/// Files matching `*.fm` in the user's save-games folder, keyed by stem
/// (lowercased), with their modification times.
fn known_save_files() -> HashMap<String, (PathBuf, std::time::SystemTime)> {
    let mut map = HashMap::new();
    let Some(doc) = dirs::document_dir() else {
        return map;
    };
    let games = doc
        .join("Sports Interactive")
        .join("Football Manager 2024")
        .join("games");
    let Ok(entries) = std::fs::read_dir(&games) else {
        return map;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("fm") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                let mtime = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                map.insert(stem.to_lowercase(), (path, mtime));
            }
        }
    }
    map
}

/// Detect the active save's display name (stem, e.g. "Ana save").
/// Candidates that match a file in the games folder win; among those, the
/// most recently modified file is preferred (the active save is the one the
/// user saved most recently in this career).
pub fn detect_save_name(fm: &FmProcess) -> Option<String> {
    let known = known_save_files();
    let mut scores: HashMap<String, (usize, std::time::SystemTime)> = HashMap::new();
    for (base, size) in fm.committed_private_regions() {
        if size > 0x400_0000 {
            continue;
        }
        let Ok(data) = fm.read_bytes(base, size) else {
            continue;
        };
        for candidate in candidates_in(&data) {
            let stem = candidate
                .strip_suffix(".fm")
                .unwrap_or(&candidate)
                .to_lowercase();
            if let Some((_, mtime)) = known.get(&stem) {
                let entry = scores.entry(stem).or_insert((0usize, *mtime));
                entry.0 += 1;
                if *mtime > entry.1 {
                    entry.1 = *mtime;
                }
            }
        }
    }
    let mut ranked: Vec<(String, usize, std::time::SystemTime)> = scores
        .into_iter()
        .map(|(stem, (hits, mtime))| (stem, hits, mtime))
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)));
    ranked.first().map(|(stem, _, _)| {
        // Restore the original casing from the known files.
        known_save_files()
            .into_iter()
            .find(|(k, _)| *k == *stem)
            .and_then(|(_, (path, _))| path.file_stem().and_then(|s| s.to_str()).map(String::from))
            .unwrap_or_else(|| stem.clone())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_extract_save_names() {
        let mut data = b"junk\x00Ana save.fm\x00other text\x00Yedek Save.fm\x00tail".to_vec();
        data.extend(b"not-a-save\x00");
        let found = candidates_in(&data);
        assert!(found.iter().any(|s| s == "Ana save.fm"));
        assert!(found.iter().any(|s| s == "Yedek Save.fm"));
        assert!(!found.iter().any(|s| s.contains("junk")));
        assert!(!found.iter().any(|s| s.contains("not-a-save")));
    }

    #[test]
    fn candidates_reject_too_long_and_non_utf8() {
        let long = vec![b'a'; 100];
        let mut data = long;
        data.extend(b".fm\x00");
        assert!(candidates_in(&data).is_empty());
    }
}
