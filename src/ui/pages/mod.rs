//! Per-page UI modules.
//!
//! Each search page owns its filter state struct, its `show()` function that
//! renders the page and returns an optional clicked-row index, and any
//! page-specific helpers (role-label tables, budget formatting, etc.).
//!
//! The main app wires these together in `src/app.rs`: it owns one state
//! struct per page, and its central-panel match calls the page's `show()`
//! function. Adding a new search page means dropping a new file here and
//! adding one field + one match arm in `app.rs`.

pub mod advanced_search_modal;
pub mod best_xi;
pub mod club_search;
pub mod injured_players;
pub mod player_search;
pub mod staff_search;
