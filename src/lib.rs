//! fm-editor — real-time memory editor for Football Manager 26 and 24.
//!
//! This library crate holds the shared code for the main editor binary
//! (`src/main.rs`) and the standalone dump/scan tools (`src/bin/*.rs`).
//! Everything that needs version-aware schemas, process attachment, or
//! field lookups lives here so it has exactly one implementation.

pub mod app;
pub mod best_xi;
pub mod bridge;
pub mod cache;
pub mod club;
pub mod club_policy;
pub mod edit_history;
pub mod fields;
pub mod game_date;
pub mod game_version;
pub mod i18n;
pub mod injury;
pub mod overlay;
pub mod person;
pub mod player;
pub mod process;
pub mod ratings;
pub mod settings;
pub mod staff;
pub mod ui;
pub mod undo;
pub mod write_tracker;

#[cfg(debug_assertions)]
pub mod debug;
