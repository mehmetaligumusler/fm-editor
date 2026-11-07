//! FmEditorApp — main application state and eframe::App implementation.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::bridge::{BridgeCmd, BridgeResult, ClubView, EntityCounts, ProcessBridge, StaffView};
use crate::cache::{CachedClub, CachedPlayer, CachedStaff, EntityCache};
use crate::edit_history::{
    ActionDraft, ChangeDraft, ChangeHistory, EditPresentation, EntityKind, EntityRef,
};
use crate::game_version::GameBuild;
use crate::i18n::UiTr;
use crate::player::PlayerRecord;
use crate::process::RunningGame;
use crate::settings::{AppSettings, Scale, Theme, WagePeriod, theme_data_id};
use crate::staff::StaffRecord;
use crate::ui::navigation::{self, BrowserHistory};
use crate::ui::pages::{
    advanced_search_modal, club_search::ClubSearchState, player_search::PlayerSearchState,
    staff_search::StaffSearchState,
};
use crate::ui::{club_panel, field_widget, pages, player_panel, staff_panel};
use crate::undo::UndoEntry;

// ---------------------------------------------------------------------------
// Shared types used by UI modules
// ---------------------------------------------------------------------------

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Page {
    BestXi,
    PlayerSearch,
    StaffSearch,
    ClubSearch,
    HumanManager,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EntityTabType {
    Player,
    Staff,
    Club,
}

/// Cache load state — tracks whether the bulk entity cache is being built.
#[derive(Clone, Copy, PartialEq)]
pub enum CacheLoadState {
    Idle,
    Loading { done: usize, total: usize },
    Ready,
}

impl EntityTabType {
    /// The search page that hosts a tab of this type.
    fn home_page(self) -> Page {
        match self {
            EntityTabType::Player => Page::PlayerSearch,
            EntityTabType::Staff => Page::StaffSearch,
            EntityTabType::Club => Page::ClubSearch,
        }
    }
}

/// One entry in the browser-style navigation history. Separate from
/// `EntityTab` — tabs are "things currently open", history is "things I've
/// visited in order". The back/forward buttons walk this vector.
#[derive(Clone, PartialEq, Eq)]
struct HistoryEntry {
    entity_type: EntityTabType,
    addr: u64,
    index: usize,
    display_name: String,
}

#[derive(Clone, PartialEq, Eq)]
enum Location {
    Results(Page),
    Entity(HistoryEntry),
}

/// Four-state connection machine.
#[derive(Clone, Copy, PartialEq)]
pub enum ConnState {
    Disconnected,
    /// fm.exe is attached but no save is loaded yet (or save creation is
    /// still in progress). The worker keeps pinging until counts stabilize.
    WaitingForSave(u32),
    /// fm.exe attached and a save is loaded. Entity reads and writes are safe.
    Ready(u32),
    /// fm.exe exited while a save was loaded. The entity cache is preserved
    /// for read-only browsing; writes are disabled until reconnect.
    ReadOnly(u32),
}

impl ConnState {
    pub fn is_ready(self) -> bool {
        matches!(self, ConnState::Ready(_))
    }
    pub fn is_attached(self) -> bool {
        !matches!(self, ConnState::Disconnected)
    }
    /// True only when connected AND the game is running. Used to gate writes.
    pub fn is_writable(self) -> bool {
        matches!(self, ConnState::Ready(_))
    }
}

pub struct EntityTab {
    pub entity_type: EntityTabType,
    pub display_name: String,
    pub addr: u64,
    pub index: usize,
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

pub struct FmEditorApp {
    // Navigation
    active_page: Page,

    // Connection
    bridge: ProcessBridge,
    conn: ConnState,
    attached_build: Option<GameBuild>,
    /// Current in-game date for this attachment; unavailable builds use reference ages.
    pub game_date: Option<crate::game_date::GameDate>,
    window_branch: Option<String>,
    /// Games offered by the last `ChooseGame` (several `fm.exe` running and
    /// none chosen yet). Cleared once a game is attached.
    game_choices: Vec<RunningGame>,
    auto_connect: bool,
    last_ping: Instant,
    last_auto_connect: Instant,

    // Per-page filter state. Each struct owns all its filter inputs.
    // Filtering runs every frame against the entity cache — no bridge round-trip.
    player_search: PlayerSearchState,
    best_xi: pages::best_xi::BestXiState,
    player_comparison: crate::ui::player_comparison::PlayerComparison,
    staff_search: StaffSearchState,
    club_search: ClubSearchState,

    // Advanced search modal state
    advanced_open: bool,
    /// Snapshot taken when the modal opens; restored on Cancel.
    advanced_snapshot: Option<PlayerSearchState>,

    // Entity cache — built by the worker on connect, preserved across GameClosed.
    entity_cache: Option<Arc<EntityCache>>,
    cache_load_state: CacheLoadState,
    /// Shared name records for the name pickers; tied to the loaded save.
    names: crate::ui::name_picker::NamePicker,

    // Entity tabs
    open_tabs: Vec<EntityTab>,
    active_tab: Option<usize>,

    // One history across search pages and related-entity links.
    history: BrowserHistory<Location>,
    // Keep opened detail snapshots available when the game exits.
    club_snapshots: HashMap<u64, ClubView>,
    player_snapshots: HashMap<u64, PlayerRecord>,
    staff_snapshots: HashMap<u64, StaffView>,

    // Selected entity data (refreshed on tab switch)
    selected_player: Option<PlayerRecord>,
    selected_staff: Option<StaffView>,
    selected_club: Option<ClubView>,
    loading_entity: bool,

    // Sub-tabs
    player_tab: player_panel::PlayerTab,
    staff_tab: staff_panel::StaffTab,
    club_tab: club_panel::ClubTab,

    // Human manager
    manager_view: Option<StaffView>,
    manager_loading: bool,

    // Audit reports and current-session chronological Undo authority.
    change_history: ChangeHistory,
    history_ui: crate::ui::edit_history::HistoryUi,
    write_error: Option<String>,
    policy_ui: crate::ui::club_policy::PolicyUi,

    // Status
    status_msg: Option<(String, Instant)>,
    entity_counts: Option<EntityCounts>,

    // User preferences
    settings: AppSettings,
    // Deferred window resize — set when scale changes, applied next frame
    // after set_zoom_factor has taken effect so InnerSize uses the new ppp.
    pending_scale_resize: Option<f32>,

    // Debug-only: handle to the MCP server state — used to show the traffic LED.
    #[cfg(debug_assertions)]
    mcp_state: Option<std::sync::Arc<crate::debug::DebugState>>,
}

impl FmEditorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings = AppSettings::load();
        cc.egui_ctx.set_visuals(settings.theme.visuals());
        cc.egui_ctx.set_zoom_factor(settings.scale.zoom_factor());
        egui_extras::install_image_loaders(&cc.egui_ctx);

        Self::from_settings(settings)
    }

    fn from_settings(settings: AppSettings) -> Self {
        Self {
            active_page: Page::HumanManager,
            bridge: ProcessBridge::new(),
            conn: ConnState::Disconnected,
            attached_build: None,
            game_date: None,
            window_branch: None,
            game_choices: Vec::new(),
            auto_connect: true,
            last_ping: Instant::now(),
            last_auto_connect: Instant::now() - std::time::Duration::from_secs(10),
            player_search: PlayerSearchState::default(),
            best_xi: pages::best_xi::BestXiState::default(),
            player_comparison: Default::default(),
            staff_search: StaffSearchState::default(),
            club_search: ClubSearchState::default(),
            advanced_open: false,
            advanced_snapshot: None,
            entity_cache: None,
            names: Default::default(),
            cache_load_state: CacheLoadState::Idle,
            open_tabs: Vec::new(),
            active_tab: None,
            history: BrowserHistory::new(Location::Results(Page::HumanManager)),
            club_snapshots: HashMap::new(),
            player_snapshots: HashMap::new(),
            staff_snapshots: HashMap::new(),
            selected_player: None,
            selected_staff: None,
            selected_club: None,
            loading_entity: false,
            player_tab: player_panel::PlayerTab::Overview,
            staff_tab: staff_panel::StaffTab::Overview,
            club_tab: club_panel::ClubTab::Overview,
            manager_view: None,
            manager_loading: false,
            change_history: ChangeHistory::new(),
            history_ui: Default::default(),
            write_error: None,
            policy_ui: Default::default(),
            status_msg: None,
            entity_counts: None,
            settings,
            pending_scale_resize: None,
            #[cfg(debug_assertions)]
            mcp_state: None,
        }
    }

    /// Keep the preview/worktree identity when the attached game changes the title.
    pub fn with_branch(mut self, branch: Option<String>) -> Self {
        self.window_branch = branch;
        self
    }

    /// Only an explicit startup flag enables policy trial writes in the worker.
    pub fn with_policy_trial(mut self, enabled: bool) -> Self {
        if enabled {
            self.bridge.send(BridgeCmd::Shutdown);
            self.bridge = ProcessBridge::with_policy_trial(true);
            self.policy_ui.enabled = true;
        }
        self
    }

    #[cfg(debug_assertions)]
    pub fn with_mcp_state(mut self, state: std::sync::Arc<crate::debug::DebugState>) -> Self {
        self.mcp_state = Some(state);
        self
    }

    // --------------------------------------------------
    // Bridge result processing
    // --------------------------------------------------

    /// Wipe all entity state on disconnect / save-unload transitions.
    /// Does NOT touch the entity cache — call `clear_cache()` separately
    /// when stale data must be discarded (e.g. save unloaded).
    fn clear_entity_state(&mut self, reason: &str) {
        self.entity_counts = None;
        self.selected_player = None;
        self.selected_staff = None;
        self.selected_club = None;
        self.manager_view = None;
        self.manager_loading = false;
        self.loading_entity = false;
        self.open_tabs.clear();
        self.active_tab = None;
        self.history = BrowserHistory::new(Location::Results(self.active_page));
        self.club_snapshots.clear();
        self.player_snapshots.clear();
        self.staff_snapshots.clear();
        self.change_history.end_session(reason);
        self.policy_ui.reset();
        self.player_comparison.clear();
        self.write_error = None;
    }

    fn handle_game_closed(&mut self) {
        self.game_date = None;
        // Ping has already detached the worker. FIFO results mean every write
        // still pending here was queued after Ping and cannot have been attempted.
        // Disable refreshes before completing refusals against retained snapshots.
        if let ConnState::Ready(pid) = self.conn {
            self.conn = ConnState::ReadOnly(pid);
        }
        let mut pending = self.change_history.pending_ids();
        pending.sort_unstable_by(|a, b| b.cmp(a));
        let queued = pending.len();
        for id in pending {
            self.complete_write(
                id,
                false,
                Some("Game closed; queued command refused before any write was attempted".into()),
                Some(false),
            );
        }
        self.change_history
            .end_session("Game closed; Undo addresses invalidated");
        self.policy_ui.reset();
        if queued > 0 {
            self.write_error = Some(format!(
                "Game closed: {queued} queued write part(s) refused before writing. Unconfirmed UI edits were restored; review Change history for confirmed parts."
            ));
        }
        self.set_status(if queued > 0 {
            format!("Game not running — browse only; {queued} queued write part(s) refused")
        } else {
            "Game not running — browse only".into()
        });
    }

    /// Drop the entity cache — called when a save is unloaded or a new one
    /// starts loading. Not called on GameClosed (cache is preserved for
    /// read-only browsing).
    fn clear_cache(&mut self) {
        self.best_xi.clear();
        self.names.clear();
        self.entity_cache = None;
        self.cache_load_state = CacheLoadState::Idle;
    }

    fn process_results(&mut self) {
        while let Some(result) = self.bridge.try_recv() {
            match result {
                BridgeResult::Connected(pid, counts, game_date) => {
                    self.game_date = game_date;
                    self.clear_entity_state("Save ready: previous editing context closed");
                    self.clear_cache();
                    self.conn = ConnState::Ready(pid);
                    let cache_total = counts.players + counts.staff + counts.clubs;
                    self.entity_counts = Some(counts);
                    self.cache_load_state = CacheLoadState::Loading {
                        done: 0,
                        total: cache_total,
                    };
                    let game = self.game_label(pid);
                    self.set_status(format!("Connected to {game}"));
                }
                BridgeResult::Attached(pid, build, game_date) => {
                    self.game_date = game_date;
                    self.conn = ConnState::WaitingForSave(pid);
                    self.clear_entity_state("Reconnected or attached to a process/build");
                    self.clear_cache();
                    self.attached_build = Some(build);
                    self.game_choices.clear();
                    let game = self.game_label(pid);
                    self.set_status(format!("Attached to {game} \u{2014} waiting for save"));
                }
                BridgeResult::ChooseGame(games) => {
                    // A stale auto-connect answer after a game was attached.
                    if !matches!(self.conn, ConnState::Disconnected | ConnState::ReadOnly(_)) {
                        continue;
                    }
                    if games != self.game_choices {
                        self.set_status(format!(
                            "{} games running \u{2014} choose one to attach",
                            games.len()
                        ));
                    }
                    self.game_choices = games;
                }
                BridgeResult::SaveUnloaded(pid) => {
                    self.game_date = None;
                    self.conn = ConnState::WaitingForSave(pid);
                    self.clear_entity_state("Save unloaded or replaced");
                    self.clear_cache();
                    self.set_status("Save unloaded \u{2014} waiting for next save".into());
                }
                BridgeResult::GameClosed => self.handle_game_closed(),
                BridgeResult::CacheProgress { done, total } => {
                    self.cache_load_state = CacheLoadState::Loading { done, total };
                }
                BridgeResult::CacheFailed(e) => {
                    // Leave the loading state so the status bar (hidden by the
                    // progress bar) shows why the save could not be loaded.
                    self.cache_load_state = CacheLoadState::Idle;
                    self.set_status(format!("Loading save failed: {e}"));
                }
                BridgeResult::NameIndex(result) => self.names.receive(result),
                BridgeResult::CacheReady(arc) => {
                    self.entity_cache = Some(arc);
                    self.cache_load_state = CacheLoadState::Ready;
                }
                BridgeResult::PingOk(game_date) => self.game_date = game_date,
                BridgeResult::CountsChanged(counts, game_date) => {
                    // Same save: keep entity state and Undo. The entity
                    // cache predates the new records; Reload Save adds them.
                    self.game_date = game_date;
                    self.entity_counts = Some(counts);
                }
                BridgeResult::PingFail => {
                    self.game_date = None;
                    self.conn = ConnState::Disconnected;
                    self.clear_entity_state("Connection lost");
                    self.clear_cache();
                    self.attached_build = None;
                    self.set_status("Disconnected".into());
                }
                BridgeResult::PlayerLoaded(rec) => {
                    self.player_snapshots.insert(rec.addr, rec.clone());
                    if self.is_active_entity(EntityTabType::Player, rec.addr) {
                        self.loading_entity = false;
                        self.selected_player = Some(rec);
                    }
                }
                BridgeResult::StaffLoaded(view) => {
                    self.staff_snapshots.insert(view.record.addr, view.clone());
                    if self.is_active_entity(EntityTabType::Staff, view.record.addr) {
                        self.loading_entity = false;
                        self.selected_staff = Some(view);
                    }
                }
                BridgeResult::ClubLoaded(view) => {
                    self.club_snapshots.insert(view.addr, view.clone());
                    if self.is_active_entity(EntityTabType::Club, view.addr) {
                        self.loading_entity = false;
                        self.selected_club = Some(view);
                    }
                }
                BridgeResult::HumanManagerLoaded(view) => {
                    self.manager_loading = false;
                    self.manager_view = Some(view);
                }
                BridgeResult::ClubPolicyPrepared { request_id, result } => {
                    self.policy_ui.receive(request_id, result)
                }
                BridgeResult::WriteOk { id, warning } => {
                    self.complete_write(id, true, warning, None)
                }
                BridgeResult::WriteFail {
                    id,
                    error,
                    write_attempted,
                } => self.complete_write(id, false, Some(error), write_attempted),
                BridgeResult::Error(e) => {
                    // While detached, errors come from attach attempts: the
                    // games offered earlier may be gone, so stop offering them.
                    if matches!(self.conn, ConnState::Disconnected | ConnState::ReadOnly(_)) {
                        self.game_choices.clear();
                    }
                    self.loading_entity = false;
                    self.manager_loading = false;
                    self.set_status(format!("Error: {e}"));
                }
            }
        }
    }

    fn is_active_entity(&self, entity_type: EntityTabType, addr: u64) -> bool {
        self.active_tab
            .and_then(|i| self.open_tabs.get(i))
            .is_some_and(|tab| {
                tab.entity_type == entity_type
                    && tab.addr == addr
                    && tab.entity_type.home_page() == self.active_page
            })
    }

    /// "FM27 (PID 1234)" for the attached game; just the PID before the
    /// version is known.
    fn game_label(&self, pid: u32) -> String {
        match self.attached_build {
            Some(build) => RunningGame { pid, build }.to_string(),
            None => format!("PID {pid}"),
        }
    }

    fn set_status(&mut self, msg: String) {
        self.status_msg = Some((msg, Instant::now()));
    }

    fn handle_auto_connect(&mut self) {
        // Retry attach when disconnected OR in read-only mode (game exited).
        // In WaitingForSave, Ping handles the promotion — no need to re-attach.
        if self.auto_connect
            && matches!(self.conn, ConnState::Disconnected | ConnState::ReadOnly(_))
            && self.last_auto_connect.elapsed().as_secs() >= 3
        {
            self.game_date = None;
            self.bridge.send(BridgeCmd::Attach(None));
            self.last_auto_connect = Instant::now();
        }
    }

    fn handle_ping(&mut self) {
        // Ping when attached and game is running (Ready or WaitingForSave).
        // Don't ping in ReadOnly — the worker has no FmProcess; it would
        // just return PingFail and we'd wipe the cache unnecessarily.
        if matches!(
            self.conn,
            ConnState::Ready(_) | ConnState::WaitingForSave(_)
        ) && self.last_ping.elapsed().as_secs() >= 2
        {
            self.bridge.send(BridgeCmd::Ping);
            self.last_ping = Instant::now();
        }
    }
    fn reconcile_write_buffers(&mut self, entry: &UndoEntry, was_undo: bool, success: bool) {
        // Failed edits restore the last known bytes. Failed undo never changed the UI.
        if was_undo && !success {
            return;
        }
        let bytes = if was_undo || !success {
            &entry.old_raw
        } else {
            &entry.new_raw
        };
        for p in self
            .player_snapshots
            .values_mut()
            .chain(self.selected_player.iter_mut())
        {
            patch_player_buffer(
                &mut p.raw,
                p.addr,
                entry.addr,
                bytes,
                p.build.schemas().player_person_offset,
                p.person_shift,
            );
            if let (Some(raw), Some(base)) = (&mut p.contract_raw, p.contract_addr) {
                patch_buffer(raw, base, entry.addr, bytes);
            }
            patch_player_list_buffers(p, entry.addr, bytes);
        }
        // A human manager's identity edits land `identity_shift` bytes past
        // the schema offset; map them back before patching the buffer.
        let normalized = |s: &StaffView| {
            let schema = s.record.build.schemas();
            let shift = s.record.identity_shift as u64;
            entry
                .addr
                .checked_sub(s.record.addr)
                .and_then(|rel| rel.checked_sub(shift))
                .filter(|rel| shift != 0 && schema.is_manager_shifted_offset(*rel))
                .map(|rel| s.record.addr + rel)
                .unwrap_or(entry.addr)
        };
        for s in self
            .staff_snapshots
            .values_mut()
            .chain(self.selected_staff.iter_mut())
        {
            let at = normalized(s);
            patch_buffer(&mut s.record.raw, s.record.addr, at, bytes);
            if let (Some(raw), Some(base)) = (&mut s.contract_raw, s.contract_addr) {
                patch_buffer(raw, base, entry.addr, bytes);
            }
        }
        if let Some(s) = &mut self.manager_view {
            if let (Some(raw), Some(base)) = (&mut s.contract_raw, s.contract_addr) {
                patch_buffer(raw, base, entry.addr, bytes);
            }
            let at = normalized(s);
            patch_buffer(&mut s.record.raw, s.record.addr, at, bytes);
        }
        for c in self
            .club_snapshots
            .values_mut()
            .chain(self.selected_club.iter_mut())
        {
            patch_buffer(&mut c.raw, c.addr, entry.addr, bytes);
            for (raw, base) in [
                (&mut c.info_raw, c.info_addr),
                (&mut c.finance_raw, c.finance_addr),
                (&mut c.budget_raw, c.budget_addr),
                (&mut c.senior_team_raw, c.senior_team_addr),
            ] {
                if let (Some(raw), Some(base)) = (raw, base) {
                    patch_buffer(raw, base, entry.addr, bytes);
                }
            }
        }
    }

    // Browser navigation intentionally crosses entity and sidebar boundaries.
    fn push_history(&mut self, entry: HistoryEntry) {
        self.history.visit(Location::Entity(entry));
    }

    fn can_nav_back(&self) -> bool {
        self.history.can_back()
    }
    fn can_nav_forward(&self) -> bool {
        self.history.can_forward()
    }

    fn activate_location(&mut self, location: Location) {
        match location {
            Location::Results(page) => {
                self.active_page = page;
                self.active_tab = None;
                self.loading_entity = false;
            }
            Location::Entity(entry) => self.activate_history_entry(entry),
        }
    }

    fn editing_allowed(&self) -> bool {
        self.conn.is_writable() && self.attached_build.is_some_and(GameBuild::allows_writes)
    }

    fn undo_unavailable_reason(&self) -> Option<String> {
        if self.attached_build == Some(GameBuild::Fm24Epic) && !GameBuild::Fm24Epic.allows_writes()
        {
            return Some("Epic preview is read-only; editing and Undo are disabled.".into());
        }
        match self.conn {
            ConnState::Ready(_) => self.change_history.undo_reason(),
            ConnState::WaitingForSave(_) => Some("Wait for a loaded save before undoing.".into()),
            ConnState::ReadOnly(_) => {
                Some("The game is closed. Old-session Undo cannot be reused.".into())
            }
            ConnState::Disconnected => {
                Some("Connect to a game and load a save before undoing.".into())
            }
        }
    }

    fn open_edit_history_entity(&mut self, target: EntityRef) -> bool {
        let entry = self.entity_cache.as_ref().and_then(|cache| {
            self.conn
                .is_ready()
                .then(|| history_navigation_target(cache, self.attached_build, &target))
                .flatten()
        });
        if let Some(entry) = entry {
            self.activate_history_entry(entry.clone());
            self.push_history(entry);
            true
        } else {
            self.set_status(
                "History target no longer matches the loaded save; no navigation performed.".into(),
            );
            false
        }
    }

    fn complete_write(
        &mut self,
        id: u64,
        success: bool,
        error: Option<String>,
        write_attempted: Option<bool>,
    ) {
        let Some(completion) =
            self.change_history
                .complete(id, success, error.clone(), write_attempted)
        else {
            return;
        };
        if completion.entry.policy_guard.is_some() {
            self.policy_ui.finished(
                success,
                error.as_deref(),
                completion.was_undo,
                write_attempted,
            );
        }
        let list_players: std::collections::BTreeSet<_> = if completion.success
            && self.conn.is_ready()
        {
            self.player_snapshots
                .values()
                .chain(self.selected_player.iter())
                .filter(|p| {
                    is_player_list_write(p, completion.entry.addr, completion.entry.new_raw.len())
                })
                .map(|p| (p.addr, p.index))
                .collect()
        } else {
            Default::default()
        };
        self.reconcile_write_buffers(&completion.entry, completion.was_undo, completion.success);
        if completion.success {
            if let Some(warning) = &error {
                self.write_error = Some(warning.clone());
            }
        } else {
            let operation = if completion.was_undo {
                "Undo"
            } else {
                "Change"
            };
            self.write_error = Some(if write_attempted == Some(false) {
                format!(
                    "{operation} refused before writing: {}. No memory write was attempted for this part. Review Change history for any other parts that completed.",
                    error.as_deref().unwrap_or("Validation failed")
                )
            } else {
                format!(
                    "{operation} unconfirmed: {}. Memory may already have changed. Inspect readback before retrying; only confirmed changes are eligible for Undo.",
                    error.as_deref().unwrap_or("No acknowledgement of success")
                )
            });
        }
        if completion.action_finished {
            if let Some(message) = self
                .change_history
                .report(completion.action_id)
                .map(|r| format!("{}: {} — {}", r.status.label(), r.entity.name, r.label))
            {
                self.set_status(message);
            }
        }
        for (addr, index) in list_players {
            if self.change_history.is_pending()
                || !self.is_active_entity(EntityTabType::Player, addr)
            {
                self.bridge.send(BridgeCmd::LoadPlayer(addr, index));
            }
        }
        if !self.change_history.is_pending() && self.conn.is_ready() {
            if let Some(tab_idx) = self.active_tab {
                self.reload_tab(tab_idx);
            } else if self.active_page == Page::HumanManager {
                self.bridge.send(BridgeCmd::LoadHumanManager);
                self.manager_loading = true;
            }
        }
    }

    fn rollback_unsubmitted_edits(&mut self, actions: Vec<ActionDraft>) {
        // Widgets can touch the same bytes more than once within a frame.
        // Reverse submission order restores the original, not an intermediate value.
        for change in actions
            .into_iter()
            .flat_map(|a| a.changes)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            self.reconcile_write_buffers(&change.entry, false, false);
        }
        self.write_error = Some("Changes were not submitted: wait for a writable connection and a loaded, identity-bound profile.".into());
    }

    fn restore_tab_snapshot(&mut self, tab_idx: usize) -> bool {
        let Some(build) = self.attached_build else {
            return false;
        };
        let Some(tab) = self.open_tabs.get(tab_idx) else {
            return false;
        };
        match tab.entity_type {
            EntityTabType::Player => {
                self.selected_player =
                    self.player_snapshots.get(&tab.addr).cloned().or_else(|| {
                        self.entity_cache
                            .as_ref()?
                            .players
                            .iter()
                            .find(|p| p.addr == tab.addr)
                            .map(|cached| cached_player_record(cached, build))
                    });
                self.selected_player.is_some()
            }
            EntityTabType::Staff => {
                self.selected_staff = self.staff_snapshots.get(&tab.addr).cloned().or_else(|| {
                    self.entity_cache
                        .as_ref()?
                        .staff
                        .iter()
                        .find(|s| s.addr == tab.addr)
                        .map(|cached| cached_staff_view(cached, build))
                });
                self.selected_staff.is_some()
            }
            EntityTabType::Club => {
                self.selected_club = self.club_snapshots.get(&tab.addr).cloned();
                self.selected_club.is_some()
            }
        }
    }

    /// Dispatch the appropriate Load command for a tab, flipping
    /// `loading_entity` so the UI shows a spinner until the bridge replies.
    fn reload_tab(&mut self, tab_idx: usize) {
        if let Some(tab) = self.open_tabs.get(tab_idx) {
            if !self.conn.is_ready() {
                self.restore_tab_snapshot(tab_idx);
                self.loading_entity = false;
                return;
            }
            let cmd = match tab.entity_type {
                EntityTabType::Player => BridgeCmd::LoadPlayer(tab.addr, tab.index),
                EntityTabType::Staff => BridgeCmd::LoadStaff(tab.addr, tab.index),
                EntityTabType::Club => BridgeCmd::LoadClub(tab.addr, tab.index),
            };
            self.bridge.send(cmd);
            self.loading_entity = true;
        }
    }

    /// Reuse or reopen a visited entity, switching to its owning search page.
    fn activate_history_entry(&mut self, entry: HistoryEntry) {
        self.active_page = entry.entity_type.home_page();
        let existing = self
            .open_tabs
            .iter()
            .position(|t| t.entity_type == entry.entity_type && t.addr == entry.addr);
        let tab_idx = if let Some(idx) = existing {
            idx
        } else {
            self.open_tabs.push(EntityTab {
                entity_type: entry.entity_type,
                display_name: entry.display_name,
                addr: entry.addr,
                index: entry.index,
            });
            self.open_tabs.len() - 1
        };
        self.active_tab = Some(tab_idx);
        self.restore_tab_snapshot(tab_idx);
        self.reload_tab(tab_idx);
    }
}

// ---------------------------------------------------------------------------
// eframe::App
// ---------------------------------------------------------------------------

impl eframe::App for FmEditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply deferred window resize — must happen the frame AFTER
        // set_zoom_factor so the new pixels_per_point is already active.
        // Skip when maximized: the window already fills the screen, so only
        // the zoom matters — no InnerSize needed and it would un-maximize.
        if let Some(f) = self.pending_scale_resize.take() {
            let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            if !maximized {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                    1280.0 * f,
                    800.0 * f,
                )));
            }
        }

        // Background housekeeping (runs before UI so results are fresh)
        self.process_results();
        if self.names.take_load_request() {
            self.bridge.send(BridgeCmd::LoadNameIndex);
        }
        self.handle_auto_connect();
        let title = editor_title(self.attached_build, self.window_branch.as_deref());
        if ctx.input(|i| i.viewport().title.as_deref() != Some(title.as_str())) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
        // Let the fm-debug MCP server follow the game this window shows when
        // several are running.
        #[cfg(debug_assertions)]
        if let Some(ref mcp) = self.mcp_state {
            let pid = match self.conn {
                ConnState::Ready(pid) | ConnState::WaitingForSave(pid) => pid,
                ConnState::ReadOnly(_) | ConnState::Disconnected => 0,
            };
            mcp.editor_pid
                .store(pid, std::sync::atomic::Ordering::Relaxed);
        }
        self.handle_ping();
        ctx.request_repaint_after(Duration::from_secs(1));

        // Auto-load human manager when navigating to that page — only
        // when a save is actually loaded, not just when fm.exe is attached.
        if self.active_page == Page::HumanManager
            && self.conn.is_ready()
            && self.manager_view.is_none()
            && !self.manager_loading
        {
            self.bridge.send(BridgeCmd::LoadHumanManager);
            self.manager_loading = true;
        }

        // Publish active theme into egui temp data so widget code can read it
        // without needing an explicit parameter (used by value_color in field_widget).
        ctx.data_mut(|d| d.insert_temp(theme_data_id(), self.settings.theme));
        navigation::set_result_keyboard_enabled(
            ctx,
            !ctx.wants_keyboard_input() && !self.advanced_open,
        );

        // Collect deferred actions during UI rendering
        let mut attach_requested = false;
        let mut attach_pid_requested: Option<u32> = None;
        let mut undo_requested = !ctx.wants_keyboard_input()
            && ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Z));
        let mut edits: Vec<field_widget::FieldEdit> = Vec::new();
        let mut edit_entity: Option<EntityRef> = None;
        let mut history_open_entity: Option<EntityRef> = None;
        let mut policy_action = None;
        let mut load_tab: Option<usize> = None;
        let mut clicked_player: Option<usize> = None;
        let mut clicked_xi_player: Option<usize> = None;
        let mut clicked_staff: Option<usize> = None;
        let mut clicked_club: Option<usize> = None;
        // Sidebar nav history actions — handled post-closure so we don't
        // hold `self` borrows across UI rendering.
        let mut nav_back_clicked = false;
        let mut nav_forward_clicked = false;
        let mut nav_reload_clicked = false;
        let mut nav_page_clicked = false;
        let mut reload_save_clicked = false;

        // ===== Menu bar =====
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button(ui.tr("File"), |ui| {
                    if ui.button("Load Save").clicked() {
                        reload_save_clicked = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button(ui.tr("Settings"), |ui| {
                    ui.menu_button("Language", |ui| {
                        for lang in [
                            crate::i18n::Language::English,
                            crate::i18n::Language::Turkish,
                        ] {
                            if ui
                                .selectable_label(self.settings.language == lang, lang.label())
                                .clicked()
                            {
                                self.settings.language = lang;
                                crate::i18n::set_language(ctx, lang);
                                self.settings.save();
                                ui.close_menu();
                            }
                        }
                    });
                    ui.menu_button("Theme", |ui| {
                        for theme in [Theme::Dark, Theme::Light, Theme::DarkBlue] {
                            if ui
                                .selectable_label(
                                    self.settings.theme == theme,
                                    ui.tr(theme.label()),
                                )
                                .clicked()
                            {
                                self.settings.theme = theme;
                                ctx.set_visuals(theme.visuals());
                                self.settings.save();
                                ui.close_menu();
                            }
                        }
                    });
                    ui.menu_button("Scale", |ui| {
                        let mut idx = self.settings.scale.to_index() as i32;
                        let labels = ["100%", "125%", "150%"];
                        ui.label(labels[idx as usize]);
                        if ui
                            .add(egui::Slider::new(&mut idx, 0..=2).show_value(false))
                            .changed()
                        {
                            let new_scale = Scale::from_index(idx as u8);
                            self.settings.scale = new_scale;
                            let f = new_scale.zoom_factor();
                            ctx.set_zoom_factor(f);
                            self.pending_scale_resize = Some(f);
                            self.settings.save();
                        }
                    });
                    ui.menu_button("Wage Display", |ui| {
                        for period in [WagePeriod::Weekly, WagePeriod::Monthly, WagePeriod::Yearly]
                        {
                            if ui
                                .selectable_label(
                                    self.settings.wage_period == period,
                                    ui.tr(period.label()),
                                )
                                .clicked()
                            {
                                self.settings.wage_period = period;
                                self.settings.save();
                                ui.close_menu();
                            }
                        }
                    });
                    ui.separator();
                    let mut rts = self.settings.reload_on_tab_switch;
                    if ui.checkbox(&mut rts, "Reload on tab switch").changed() {
                        self.settings.reload_on_tab_switch = rts;
                        self.settings.save();
                    }
                    ui.menu_button("Face Pack", |ui| {
                        let current = self.settings.face_pack_path.as_deref().unwrap_or("(none)");
                        ui.label(current);
                        if ui.button("Set path\u{2026}").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                self.settings.face_pack_path =
                                    Some(path.to_string_lossy().into_owned());
                                self.settings.save();
                            }
                            ui.close_menu();
                        }
                        if self.settings.face_pack_path.is_some() && ui.button("Clear").clicked() {
                            self.settings.face_pack_path = None;
                            self.settings.save();
                            ui.close_menu();
                        }
                    });
                });
            });
        });

        // Clone the Arc so we can borrow entity data inside egui closures
        // without conflicting with the &mut self borrow the closures hold.
        let cache_arc = self.entity_cache.clone();
        navigation::publish_cache(ctx, cache_arc.clone());
        crate::i18n::set_language(ctx, self.settings.language);
        navigation::set_editing_enabled(
            ctx,
            self.editing_allowed() && !self.change_history.is_pending(),
        );
        let wage_period = self.settings.wage_period;
        let face_pack_path = self.settings.face_pack_path.clone();
        let schemas = self.attached_build.map(GameBuild::schemas);
        // Only filter the active page — running all three every frame on a
        // large save (50k+ players) causes visible lag and hangs.
        let player_filtered: Vec<&CachedPlayer> = if self.active_page == Page::PlayerSearch {
            if let (Some(c), Some(schemas)) = (&cache_arc, schemas) {
                pages::player_search::filter_with_clubs(
                    schemas,
                    self.game_date,
                    &c.players,
                    &self.player_search,
                    c.my_clubs.as_ref(),
                )
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        let staff_filtered: Vec<&CachedStaff> = if self.active_page == Page::StaffSearch {
            if let (Some(c), Some(schemas)) = (&cache_arc, schemas) {
                pages::staff_search::filter(schemas, self.game_date, &c.staff, &self.staff_search)
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        let club_filtered: Vec<&CachedClub> = if self.active_page == Page::ClubSearch {
            if let Some(c) = &cache_arc {
                pages::club_search::filter(&c.clubs, &self.club_search)
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // ===== Sidebar =====
        egui::SidePanel::left("sidebar")
            .default_width(196.0)
            .min_width(176.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.heading(editor_title(self.attached_build, None));
                ui.add_space(12.0);

                // History + reload row — sits right under the title so
                // Back / Forward / Reload are always the first thing the
                // eye hits in the sidebar.
                let can_back = self.can_nav_back();
                let can_forward = self.can_nav_forward();
                let has_current = self.active_tab.is_some()
                    || (self.active_page == Page::HumanManager && self.manager_view.is_some());
                ui.horizontal(|ui| {
                    let back_resp = ui.add_enabled(
                        can_back,
                        egui::Button::new(egui::RichText::new("\u{2B05}").size(14.0))
                            .min_size(egui::vec2(28.0, 22.0)),
                    );
                    if back_resp.clicked() {
                        nav_back_clicked = true;
                    }
                    if !can_back {
                        back_resp.on_disabled_hover_text("No earlier entry");
                    } else {
                        back_resp.on_hover_text("Back");
                    }

                    let fwd_resp = ui.add_enabled(
                        can_forward,
                        egui::Button::new(egui::RichText::new("\u{27A1}").size(14.0))
                            .min_size(egui::vec2(28.0, 22.0)),
                    );
                    if fwd_resp.clicked() {
                        nav_forward_clicked = true;
                    }
                    if !can_forward {
                        fwd_resp.on_disabled_hover_text("No later entry");
                    } else {
                        fwd_resp.on_hover_text("Forward");
                    }

                    let reload_resp = ui.add_enabled(
                        has_current,
                        egui::Button::new(egui::RichText::new("\u{21BB}").size(14.0))
                            .min_size(egui::vec2(28.0, 22.0)),
                    );
                    if reload_resp.clicked() {
                        nav_reload_clicked = true;
                    }
                    if !has_current {
                        reload_resp.on_disabled_hover_text("Nothing to reload");
                    } else {
                        reload_resp.on_hover_text("Reload from memory");
                    }
                });

                ui.separator();
                ui.add_space(8.0);
                ui.label(egui::RichText::new(ui.tr("BROWSE")).small().strong().weak());
                for (page, label) in [
                    (Page::BestXi, "Best XI"),
                    (Page::PlayerSearch, "Players"),
                    (Page::StaffSearch, "Staff"),
                    (Page::ClubSearch, "Clubs"),
                ] {
                    let selected = self.active_page == page;
                    if ui
                        .add_sized(
                            [ui.available_width(), 30.0],
                            egui::Button::new(ui.tr(label))
                                .selected(selected)
                                .frame(selected),
                        )
                        .clicked()
                    {
                        self.active_page = page;
                        nav_page_clicked = true;
                    }
                }
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(ui.tr("MANAGEMENT"))
                        .small()
                        .strong()
                        .weak(),
                );
                let selected = self.active_page == Page::HumanManager;
                if ui
                    .add_sized(
                        [ui.available_width(), 30.0],
                        egui::Button::new(ui.tr("Human Manager"))
                            .selected(selected)
                            .frame(selected),
                    )
                    .clicked()
                {
                    self.active_page = Page::HumanManager;
                    nav_page_clicked = true;
                }
                ui.add_space(20.0);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        egui::RichText::new(ui.tr("GAME CONNECTION"))
                            .small()
                            .strong()
                            .weak(),
                    );
                    ui.add_space(6.0);

                    // Connection status
                    match self.conn {
                        ConnState::Ready(_) => {
                            ui.colored_label(
                                egui::Color32::from_rgb(76, 175, 80),
                                format!("\u{25CF} {}", ui.tr("Connected")),
                            );
                        }
                        ConnState::WaitingForSave(_) => {
                            ui.colored_label(
                                egui::Color32::from_rgb(255, 193, 7),
                                format!("\u{25CF} {}", ui.tr("Waiting for save…")),
                            );
                        }
                        ConnState::ReadOnly(_) => {
                            ui.colored_label(
                                egui::Color32::from_rgb(255, 152, 0),
                                format!("\u{25CF} {}", ui.tr("Game not running")),
                            );
                            if ui.button(ui.tr("Reconnect")).clicked() {
                                attach_requested = true;
                            }
                        }
                        ConnState::Disconnected => {
                            ui.colored_label(
                                egui::Color32::from_rgb(244, 67, 54),
                                format!("\u{25CF} {}", ui.tr("Disconnected")),
                            );
                            if ui.button(ui.tr("Load Game")).clicked() {
                                attach_requested = true;
                            }
                        }
                    }
                    // Several fm.exe running: one button per game. Shown while no
                    // game is attached (including read-only after a game exits).
                    if matches!(self.conn, ConnState::Disconnected | ConnState::ReadOnly(_))
                        && !self.game_choices.is_empty()
                    {
                        ui.label(ui.tr("Attach:"));
                        for game in &self.game_choices {
                            if ui.button(game.to_string()).clicked() {
                                attach_pid_requested = Some(game.pid);
                            }
                        }
                    }
                    ui.checkbox(&mut self.auto_connect, &ui.tr("Auto-connect"));

                    // Reload Save button — available when connected + cache ready
                    if matches!(self.conn, ConnState::Ready(_))
                        && self.cache_load_state == CacheLoadState::Ready
                    {
                        if ui
                            .button(format!("\u{21BB} {}", ui.tr("Reload Save")))
                            .clicked()
                        {
                            reload_save_clicked = true;
                        }
                    }
                });
            });

        // ===== Status Bar =====
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            // Progress bar — shown while cache is loading
            if let CacheLoadState::Loading { done, total } = self.cache_load_state {
                let pct = if total > 0 {
                    done as f32 / total as f32
                } else {
                    0.0
                };
                ui.horizontal(|ui| {
                    ui.label("Loading save\u{2026}");
                    ui.add(
                        egui::ProgressBar::new(pct)
                            .show_percentage()
                            .desired_width(220.0),
                    );
                });
                return;
            }

            ui.horizontal(|ui| {
                match self.conn {
                    ConnState::Ready(pid) => {
                        ui.colored_label(
                            egui::Color32::from_rgb(76, 175, 80),
                            format!("\u{25CF} Connected to {}", self.game_label(pid)),
                        );
                    }
                    ConnState::WaitingForSave(pid) => {
                        ui.colored_label(
                            egui::Color32::from_rgb(255, 193, 7),
                            format!(
                                "\u{25CF} Attached to {} \u{2014} waiting for save",
                                self.game_label(pid)
                            ),
                        );
                    }
                    ConnState::ReadOnly(_) => {
                        ui.colored_label(
                            egui::Color32::from_rgb(255, 152, 0),
                            "\u{25CF} Game not running \u{2014} browse only",
                        );
                    }
                    ConnState::Disconnected => {
                        ui.colored_label(
                            egui::Color32::from_rgb(244, 67, 54),
                            "\u{25CF} Disconnected",
                        );
                    }
                }
                ui.separator();
                if let Some(ref c) = self.entity_counts {
                    ui.label(format!(
                        "{} {}  {} {}  {} {}",
                        ui.tr("Players:"),
                        c.players,
                        ui.tr("Staff:"),
                        c.staff,
                        ui.tr("Clubs:"),
                        c.clubs
                    ));
                }
                ui.separator();
                if let Some((ref msg, t)) = self.status_msg {
                    if t.elapsed().as_secs() < 8 {
                        ui.label(msg.as_str());
                    }
                }

                // Debug-only: MCP traffic LED — dim green when idle, bright when
                // a tool call was made in the last 300 ms (HDD-LED style).
                #[cfg(debug_assertions)]
                if let Some(ref mcp) = self.mcp_state {
                    use std::sync::atomic::Ordering;
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    let last_ms = mcp.last_activity_ms.load(Ordering::Relaxed);
                    let active = last_ms > 0 && now_ms.saturating_sub(last_ms) < 300;
                    let color = if active {
                        egui::Color32::from_rgb(0, 230, 100) // bright: recent activity
                    } else {
                        egui::Color32::from_rgb(0, 80, 40) // dim: idle
                    };
                    ui.separator();
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 5.0, color);
                    ui.label(format!("MCP :{}", mcp.port));
                }
            });
        });

        // Entity tabs now live inline inside the central panel, below each
        // search page's filter row — filtered to only show tabs matching
        // the active page's entity type, so e.g. a Player tab doesn't leak
        // into the Club Search view.

        // ===== Central Panel =====
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if self.change_history.is_pending() {
                    ui.spinner();
                    ui.label("Applying changes to game memory…");
                } else if self.attached_build == Some(GameBuild::Fm24Epic)
                    && !GameBuild::Fm24Epic.allows_writes()
                {
                    ui.label("Epic preview — read-only. Field readings are experimental; editing is disabled.");
                } else if self.editing_allowed() {
                    ui.label(ui.tr("Editing live — changes apply immediately. Ctrl+Z to undo."));
                } else {
                    ui.label(ui.tr("Read-only — editing is disabled until a game and save are connected."));
                }
                ui.separator();
                let undo_label = format!("{} ({})", ui.tr("Undo"), self.change_history.undo_len());
                let undo_reason = self.undo_unavailable_reason();
                let undo_hint = undo_reason.clone().unwrap_or_else(|| {
                    format!("Undo latest: {}. Current bytes will be checked before writing.", self.change_history.undo_label().unwrap_or_default())
                });
                if ui.add_enabled(undo_reason.is_none(), egui::Button::new(undo_label))
                    .on_hover_text(&undo_hint).on_disabled_hover_text(&undo_hint).clicked() {
                    undo_requested = true;
                }
                if ui.button(ui.tr("Change history")).clicked() {
                    self.history_ui.open = !self.history_ui.open;
                }
                self.player_comparison.toolbar(ui);
            });
            if let Some(error) = self.write_error.clone() {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                    if ui.small_button("Dismiss").clicked() { self.write_error = None; }
                });
            }
            ui.separator();
            let Some(schemas) = schemas else {
                ui.label(ui.tr("Waiting for fm.exe…"));
                return;
            };
            match self.active_page {
                Page::BestXi => {
                    clicked_xi_player = pages::best_xi::show(
                        ui, &mut self.best_xi, schemas, cache_arc.as_ref(),
                    );
                }
                // ---- Search pages: filter row always visible, results area swaps ----
                Page::PlayerSearch | Page::StaffSearch | Page::ClubSearch => {
                    // Filter row — always rendered, live filtering each frame.
                    match self.active_page {
                        Page::PlayerSearch => {
                            if pages::player_search::filter_row(
                                ui, &mut self.player_search, Some(schemas),
                                cache_arc.as_ref().and_then(|c| c.my_clubs.as_ref()),
                            )
                                && !self.advanced_open
                            {
                                self.advanced_snapshot = Some(self.player_search.clone());
                                self.advanced_open = true;
                            }
                        }
                        Page::StaffSearch  => pages::staff_search::filter_row(ui, &mut self.staff_search),
                        Page::ClubSearch   => pages::club_search::filter_row(ui, &mut self.club_search),
                        Page::HumanManager | Page::BestXi => unreachable!(),
                    }

                    ui.add_space(4.0);

                    // Results badge + inline entity tabs. The badge is
                    // always the leftmost item so the count stays visible
                    // while a detail panel is open. Clicking it clears
                    // the active tab and restores the results grid.
                    // Each open tab for this page renders as "[Name] ×".
                    // The count reflects the CURRENT search results, not
                    // the total entity count in the save.
                    let count_text = match self.active_page {
                        Page::PlayerSearch => format!("{} ({})", ui.tr("Results"), player_filtered.len()),
                        Page::StaffSearch  => format!("{} ({})", ui.tr("Results"), staff_filtered.len()),
                        Page::ClubSearch   => format!("{} ({})", ui.tr("Results"), club_filtered.len()),
                        Page::HumanManager | Page::BestXi => "Results".to_string(),
                    };
                    let mut close_idx: Option<usize> = None;
                    let mut results_badge_clicked = false;
                    ui.horizontal(|ui| {
                        let results_selected = self.active_tab.is_none();
                        if ui.selectable_label(results_selected, count_text).clicked() {
                            self.active_tab = None;
                            results_badge_clicked = true;
                        }
                        for i in 0..self.open_tabs.len() {
                            if self.open_tabs[i].entity_type.home_page()
                                != self.active_page
                            {
                                continue;
                            }
                            let selected = self.active_tab == Some(i);
                            let name = self.open_tabs[i].display_name.clone();
                            // Render name + × as a single Button so the
                            // two halves share one selection background.
                            // Click position tells us which half was hit:
                            // the rightmost ~18px is the close hotspot.
                            let text = format!("{name}   \u{2715}");
                            let resp = ui.add(
                                egui::Button::new(text)
                                    .selected(selected)
                                    .min_size(egui::vec2(0.0, 22.0)),
                            );
                            if resp.clicked() {
                                let close_zone = 18.0;
                                let clicked_close = resp
                                    .interact_pointer_pos()
                                    .map(|p| p.x > resp.rect.right() - close_zone)
                                    .unwrap_or(false);
                                if clicked_close {
                                    close_idx = Some(i);
                                } else if !selected {
                                    // Record the tab switch in history so
                                    // Back walks between the open tabs.
                                    // Snapshot fields before we mutate self
                                    // so the borrow checker stays happy.
                                    let entry = HistoryEntry {
                                        entity_type: self.open_tabs[i].entity_type,
                                        addr: self.open_tabs[i].addr,
                                        index: self.open_tabs[i].index,
                                        display_name: self.open_tabs[i].display_name.clone(),
                                    };
                                    self.active_tab = Some(i);
                                    load_tab = Some(i);
                                    self.push_history(entry);
                                }
                            }
                        }
                    });
                    if let Some(i) = close_idx {
                        self.open_tabs.remove(i);
                        match self.active_tab {
                            Some(idx) if idx == i => self.active_tab = None,
                            Some(idx) if idx > i => self.active_tab = Some(idx - 1),
                            _ => {}
                        }
                    }
                    // Clicking the Results badge should also sync the
                    // page's history idx to None so Back is disabled
                    // (we're now at the implicit "results" entry).
                    if results_badge_clicked || (close_idx.is_some() && self.active_tab.is_none()) {
                        self.loading_entity = false;
                        self.history.visit(Location::Results(self.active_page));
                    }
                    ui.add_space(4.0);

                    // Idle-state short-circuit.
                    let has_cache = matches!(self.cache_load_state, CacheLoadState::Ready);
                    if !self.conn.is_ready() && !has_cache {
                        ui.label(match self.conn {
                            ConnState::Disconnected => "Waiting for fm.exe\u{2026}",
                            ConnState::WaitingForSave(_) =>
                                "Attached to fm.exe \u{2014} load a save to search.",
                            ConnState::ReadOnly(_) => "Game not running \u{2014} loading\u{2026}",
                            ConnState::Ready(_) => "",
                        });
                        return;
                    }

                    // Cache still loading — progress bar shown in status bar;
                    // show a brief note in the results area too.
                    if matches!(self.cache_load_state, CacheLoadState::Loading { .. }) {
                        ui.label("Loading entity cache\u{2026}");
                        return;
                    }

                    // Results area: either a loading spinner (for tab refreshes),
                    // the entity detail panel, or the results grid.
                    if self.loading_entity {
                        ui.centered_and_justified(|ui| {
                            ui.spinner();
                        });
                        return;
                    }

                    // If there's an active tab that matches this page's
                    // entity type, render its detail panel in the results
                    // area. Otherwise fall through to the results grid.
                    let active_tab_matches = self
                        .active_tab
                        .and_then(|i| self.open_tabs.get(i))
                        .map(|t| t.entity_type.home_page() == self.active_page)
                        .unwrap_or(false);

                    if active_tab_matches {
                        let tab_idx = self.active_tab.unwrap();
                        let tab_type = self.open_tabs[tab_idx].entity_type;
                        let tab_addr = self.open_tabs[tab_idx].addr;
                        match tab_type {
                            EntityTabType::Player => {
                                if let Some(p) = self.selected_player.as_mut().filter(|p| p.addr == tab_addr) {
                                    let addr = p.addr;
                                    edit_entity = Some(history_player_entity(p));
                                    // Capture confirmed displayed data before any editable widgets run.
                                    self.player_comparison.controls(
                                        ui, p, self.game_date,
                                        !self.change_history.is_pending() && edits.is_empty(),
                                    );
                                    let e = player_panel::show(
                                        schemas, self.game_date, ui, p, &mut self.player_tab, addr,
                                        wage_period, face_pack_path.as_deref(), &mut self.names,
                                    );
                                    edits.extend(e);
                                } else {
                                    ui.label("Player details unavailable. Reload while the game is connected.");
                                }
                            }
                            EntityTabType::Staff => {
                                if let Some(s) = self.selected_staff.as_mut().filter(|s| s.record.addr == tab_addr) {
                                    edit_entity = Some(history_staff_entity(s, false));
                                    let e = staff_panel::show(schemas, self.game_date, ui, s, &mut self.staff_tab, &mut self.names);
                                    edits.extend(e);
                                } else {
                                    ui.label("Staff details unavailable. Reload while the game is connected.");
                                }
                            }
                            EntityTabType::Club => {
                                if let Some(c) = self.selected_club.as_mut().filter(|c| c.addr == tab_addr) {
                                    edit_entity = self.attached_build.map(|build| history_club_entity(c, build));
                                    let (e,action) = club_panel::show(schemas,ui,c,&mut self.club_tab,&mut self.policy_ui,
                                        self.attached_build.is_some_and(|b|b.version()==crate::game_version::GameVersion::Fm27),
                                        self.conn.is_writable() && self.attached_build.is_some_and(GameBuild::allows_writes) && !self.change_history.is_pending() && edits.is_empty());
                                    edits.extend(e);policy_action=action;
                                } else {
                                    ui.label("Club details unavailable. Reload while the game is connected.");
                                }
                            }
                        }
                    } else {
                        match self.active_page {
                            Page::PlayerSearch => {
                                clicked_player = pages::player_search::results_grid(
                                    schemas,
                                    self.game_date,
                                    ui,
                                    &mut self.player_search,
                                    &player_filtered,
                                );
                            }
                            Page::StaffSearch => {
                                clicked_staff = pages::staff_search::results_grid(
                                    schemas,
                                    self.game_date,
                                    ui,
                                    &mut self.staff_search,
                                    &staff_filtered,
                                );
                            }
                            Page::ClubSearch => {
                                clicked_club = pages::club_search::results_grid(
                                    ui,
                                    &club_filtered,
                                );
                            }
                            Page::HumanManager | Page::BestXi => unreachable!(),
                        }
                    }
                }

                // ---- Human manager: no filter row, dedicated panel ----
                Page::HumanManager => {
                    egui::ScrollArea::vertical().show(ui, |ui| match self.conn {
                        ConnState::Disconnected => {
                            ui.label("Waiting for fm.exe\u{2026}");
                        }
                        ConnState::WaitingForSave(_) => {
                            ui.label(
                                "Attached to fm.exe \u{2014} load a save to view your manager.",
                            );
                        }
                        ConnState::Ready(_) | ConnState::ReadOnly(_) => {
                            if self.manager_loading {
                                ui.spinner();
                            } else if let Some(ref mut mgr) = self.manager_view {
                                edit_entity = Some(history_staff_entity(mgr, true));
                                let e = staff_panel::show_manager(schemas, self.game_date, ui, mgr, &mut self.names);
                                if self.editing_allowed() {
                                    edits.extend(e);
                                }
                            } else {
                                ui.label("Loading human manager\u{2026}");
                            }
                        }
                    });
                }
            }
        });

        self.player_comparison.show(ctx);

        // Reports cannot write: only latest-action Undo and identity-checked navigation.
        let undo_reason = self.undo_unavailable_reason();
        let undo_label = self.change_history.undo_label();
        let history_action = self.history_ui.show(
            ctx,
            self.change_history.reports(),
            crate::ui::edit_history::HistoryUiContext {
                current_session: self.change_history.session_id(),
                current_entity: edit_entity.as_ref(),
                undo_label: undo_label.as_deref(),
                undo_reason: undo_reason.as_deref(),
                navigation_enabled: self.conn.is_ready()
                    && self.cache_load_state == CacheLoadState::Ready,
            },
        );
        match history_action {
            Some(crate::ui::edit_history::HistoryAction::UndoLatest) => undo_requested = true,
            Some(crate::ui::edit_history::HistoryAction::OpenEntity(target)) => {
                history_open_entity = Some(target)
            }
            None => {}
        }

        // ===== Advanced Search modal =====
        if self.advanced_open {
            use advanced_search_modal::ModalResult;
            let result =
                advanced_search_modal::show(ctx, &mut self.player_search, &mut self.advanced_open);
            match result {
                ModalResult::Ok => {
                    self.advanced_snapshot = None;
                }
                ModalResult::Cancel => {
                    if let Some(snapshot) = self.advanced_snapshot.take() {
                        self.player_search = snapshot;
                    }
                }
                ModalResult::ClearAll => {
                    self.player_search.clear_advanced();
                }
                ModalResult::None => {}
            }
        }

        // =================================================================
        // Post-UI action processing (after all closures have completed)
        // =================================================================

        if attach_requested {
            self.game_date = None;
            self.bridge.send(BridgeCmd::Attach(None));
        }
        if let Some(pid) = attach_pid_requested {
            self.game_date = None;
            self.bridge.send(BridgeCmd::Attach(Some(pid)));
        }

        // A write becomes undoable only after the worker confirms success.
        if let Some(action) = policy_action
            && self.editing_allowed()
            && !self.change_history.is_pending()
            && edits.is_empty()
        {
            match action {
                crate::ui::club_policy::Action::Prepare {
                    request_id,
                    identity,
                } => self.bridge.send(BridgeCmd::PrepareClubPolicy {
                    request_id,
                    identity,
                }),
                crate::ui::club_policy::Action::Apply(entry) => {
                    if let (Some(guard), Some(build)) =
                        (entry.policy_guard.as_ref(), self.attached_build)
                    {
                        let identity = guard.identity();
                        let name = self
                            .entity_cache
                            .as_ref()
                            .and_then(|cache| {
                                cache.clubs.iter().find(|c| {
                                    c.addr == identity.address
                                        && c.uid == identity.uid
                                        && c.index == identity.index
                                })
                            })
                            .map(|c| c.name.clone())
                            .unwrap_or_else(|| format!("Club UID {}", identity.uid));
                        let entity = EntityRef {
                            kind: EntityKind::Club,
                            build,
                            address: identity.address,
                            index: identity.index,
                            uid: Some(identity.uid),
                            name,
                        };
                        let presentation = EditPresentation::new(
                            "Part Time offer policy",
                            "Non-Contract offers only",
                            "Part Time and Non-Contract offers",
                        );
                        let action = ActionDraft {
                            label: presentation.field.clone(),
                            before: presentation.before.clone(),
                            after: presentation.after.clone(),
                            changes: vec![ChangeDraft {
                                entry,
                                presentation,
                            }],
                        };
                        for cmd in self.change_history.submit(entity, vec![action]) {
                            self.bridge.send(cmd);
                        }
                    } else {
                        let message = "Policy change refused: attachment or club identity is unavailable; no write was attempted.";
                        self.policy_ui
                            .finished(false, Some(message), false, Some(false));
                        self.write_error = Some(message.into());
                    }
                }
            }
        }
        if undo_requested && edits.is_empty() && self.undo_unavailable_reason().is_none() {
            match self.change_history.undo_latest() {
                Ok(commands) => {
                    for cmd in commands {
                        self.bridge.send(cmd);
                    }
                }
                Err(reason) => self.write_error = Some(reason),
            }
        }
        if !edits.is_empty() {
            let actions = history_actions(edits);
            if self.editing_allowed() && !self.change_history.is_pending() && edit_entity.is_some()
            {
                for cmd in self
                    .change_history
                    .submit(edit_entity.take().unwrap(), actions)
                {
                    self.bridge.send(cmd);
                }
            } else {
                // Never leave unsubmitted optimistic bytes editable or send an
                // identity-less edit to whichever tab is now active.
                self.rollback_unsubmitted_edits(actions);
            }
        }

        // Handle reload_save — clear stale cache and re-trigger bulk load.
        if reload_save_clicked {
            self.game_date = None;
            self.best_xi.clear();
            self.player_comparison.clear();
            self.entity_cache = None;
            self.cache_load_state = CacheLoadState::Loading { done: 0, total: 0 };
            self.bridge.send(BridgeCmd::LoadEntityCache);
        }

        if let Some(target) = history_open_entity {
            if self.open_edit_history_entity(target) {
                nav_page_clicked = false;
            }
        }
        if nav_page_clicked {
            self.active_tab = None;
            self.loading_entity = false;
            self.history.visit(Location::Results(self.active_page));
        }

        // (Cache-driven search needs no auto-submit — results appear immediately.)

        if let Some(cached) = clicked_xi_player.and_then(|i| cache_arc.as_ref()?.players.get(i)) {
            let entry = HistoryEntry {
                entity_type: EntityTabType::Player,
                addr: cached.addr,
                index: cached.index,
                display_name: cached.name.clone(),
            };
            self.history.visit(Location::Entity(entry.clone()));
            self.activate_history_entry(entry);
        }

        // Open player from search results — build PlayerRecord from cache (no bridge call)
        if let (Some(i), Some(build)) = (clicked_player, self.attached_build) {
            let cached = player_filtered[i];
            let name = if cached.name.is_empty() {
                "Unknown".to_string()
            } else {
                cached.name.clone()
            };
            let addr = cached.addr;
            let index = cached.index;
            let rec = cached_player_record(cached, build);
            let existing = self
                .open_tabs
                .iter()
                .position(|t| t.addr == addr && t.entity_type == EntityTabType::Player);
            if let Some(idx) = existing {
                self.active_tab = Some(idx);
            } else {
                self.open_tabs.push(EntityTab {
                    entity_type: EntityTabType::Player,
                    display_name: name.clone(),
                    addr,
                    index,
                });
                self.active_tab = Some(self.open_tabs.len() - 1);
            }
            self.selected_player = Some(rec);
            if !self.conn.is_ready() {
                self.restore_tab_snapshot(self.active_tab.unwrap());
            }
            // The cache holds only the struct bytes; load sub-records (contract,
            // club, nationality) so the panel can show them.
            if self.conn.is_ready() {
                self.bridge.send(BridgeCmd::LoadPlayer(addr, index));
            }
            if self.conn.is_ready() {
                self.loading_entity = true;
            }
            self.push_history(HistoryEntry {
                entity_type: EntityTabType::Player,
                addr,
                index,
                display_name: name,
            });
        }

        // Open staff from search results — build StaffView from cache (no bridge call)
        if let (Some(i), Some(build)) = (clicked_staff, self.attached_build) {
            let cached = staff_filtered[i];
            let name = if cached.name.is_empty() {
                "Unknown".to_string()
            } else {
                cached.name.clone()
            };
            let addr = cached.addr;
            let index = cached.index;
            let view = cached_staff_view(cached, build);
            let existing = self
                .open_tabs
                .iter()
                .position(|t| t.addr == addr && t.entity_type == EntityTabType::Staff);
            if let Some(idx) = existing {
                self.active_tab = Some(idx);
            } else {
                self.open_tabs.push(EntityTab {
                    entity_type: EntityTabType::Staff,
                    display_name: name.clone(),
                    addr,
                    index,
                });
                self.active_tab = Some(self.open_tabs.len() - 1);
            }
            self.selected_staff = Some(view);
            if !self.conn.is_ready() {
                self.restore_tab_snapshot(self.active_tab.unwrap());
            }
            if self.conn.is_ready() {
                self.loading_entity = true;
            }
            // The cache holds only the struct bytes; load sub-records
            // (contract, club, nationality) so the panel can show them.
            if self.conn.is_ready() {
                self.bridge.send(BridgeCmd::LoadStaff(addr, index));
            }
            self.push_history(HistoryEntry {
                entity_type: EntityTabType::Staff,
                addr,
                index,
                display_name: name,
            });
        }

        // Open club from search results — still uses bridge (club detail not cached)
        if let Some(i) = clicked_club {
            let cached = club_filtered[i];
            let name = cached.name.clone();
            let addr = cached.addr;
            let index = cached.index;
            let existing = self
                .open_tabs
                .iter()
                .position(|t| t.addr == addr && t.entity_type == EntityTabType::Club);
            if let Some(idx) = existing {
                self.active_tab = Some(idx);
            } else {
                self.open_tabs.push(EntityTab {
                    entity_type: EntityTabType::Club,
                    display_name: name.clone(),
                    addr,
                    index,
                });
                self.active_tab = Some(self.open_tabs.len() - 1);
            }
            if let Some(tab_idx) = self.active_tab {
                self.reload_tab(tab_idx);
            }
            self.push_history(HistoryEntry {
                entity_type: EntityTabType::Club,
                addr,
                index,
                display_name: name,
            });
        }

        // Tab switch — reload entity data from memory (gated on setting)
        if let Some(tab_idx) = load_tab {
            let restored = self.restore_tab_snapshot(tab_idx);
            if self.settings.reload_on_tab_switch || !restored {
                self.reload_tab(tab_idx);
            }
        }

        // Related names open through the same path as tabs and history.
        if let Some(target) = navigation::take_club_target(ctx) {
            let entry = HistoryEntry {
                entity_type: EntityTabType::Club,
                addr: target.addr,
                index: target.index,
                display_name: target.name,
            };
            self.push_history(entry.clone());
            self.activate_history_entry(entry);
        }
        if nav_back_clicked {
            if let Some(location) = self.history.back() {
                self.activate_location(location);
            }
        }
        if nav_forward_clicked {
            if let Some(location) = self.history.forward() {
                self.activate_location(location);
            }
        }
        if nav_reload_clicked {
            if let Some(tab_idx) = self.active_tab {
                self.reload_tab(tab_idx);
            } else if self.active_page == Page::HumanManager && self.conn.is_ready() {
                self.bridge.send(BridgeCmd::LoadHumanManager);
                self.manager_loading = true;
            }
        }
    }
}

/// Patch only complete, address-checked ranges; never truncate or wrap a write.
fn patch_buffer(raw: &mut [u8], base: u64, addr: u64, bytes: &[u8]) {
    let Some(start) = addr.checked_sub(base).and_then(|n| usize::try_from(n).ok()) else {
        return;
    };
    let Some(end) = start.checked_add(bytes.len()) else {
        return;
    };
    if let Some(dst) = raw.get_mut(start..end) {
        dst.copy_from_slice(bytes);
    }
}

fn patch_player_buffer(
    raw: &mut [u8],
    base: u64,
    addr: u64,
    bytes: &[u8],
    person: usize,
    shift: usize,
) {
    if shift == 0 {
        patch_buffer(raw, base, addr, bytes);
    } else if person <= raw.len() {
        let (prefix, person_raw) = raw.split_at_mut(person);
        patch_buffer(prefix, base, addr, bytes);
        if let Some(person_base) = base
            .checked_add(person as u64)
            .and_then(|p| p.checked_add(shift as u64))
        {
            patch_buffer(person_raw, person_base, addr, bytes);
        }
    }
}

fn is_player_list_write(player: &PlayerRecord, addr: u64, len: usize) -> bool {
    use crate::player::ListState;
    matches!(&player.injuries, ListState::Items(items) if len == 4 && items.iter().any(|item| item.days_addr == addr))
        || matches!(&player.absences, ListState::Items(items) if len == 2 && items.iter().any(|item| item.days_addr == addr))
        || matches!(&player.clauses, ListState::Items(items) if len == 4 && items.iter().any(|item| item.amount_addr == addr))
}

/// List edits are not speculative: update every retained snapshot by the exact
/// acknowledged address, using the same chosen bytes for success, failure and Undo.
fn patch_player_list_buffers(player: &mut PlayerRecord, addr: u64, bytes: &[u8]) {
    use crate::player::ListState;
    if let ListState::Items(items) = &mut player.injuries {
        for injury in items {
            let mut raw = [
                injury.full_days.to_le_bytes(),
                injury.light_days.to_le_bytes(),
            ]
            .concat();
            patch_buffer(&mut raw, injury.days_addr, addr, bytes);
            injury.full_days = u16::from_le_bytes([raw[0], raw[1]]);
            injury.light_days = u16::from_le_bytes([raw[2], raw[3]]);
        }
    }
    if let ListState::Items(items) = &mut player.absences {
        for absence in items {
            let mut raw = absence.days.to_le_bytes();
            patch_buffer(&mut raw, absence.days_addr, addr, bytes);
            absence.days = u16::from_le_bytes(raw);
        }
    }
    if let ListState::Items(items) = &mut player.clauses {
        for clause in items {
            let mut raw = clause.amount.to_le_bytes();
            patch_buffer(&mut raw, clause.amount_addr, addr, bytes);
            clause.amount = i32::from_le_bytes(raw);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn list_acknowledgements_and_undo_patch_only_matching_addresses() {
        use crate::player::{Absence, Clause, GameDate, Injury, ListState};
        let date = GameDate::from_raw((2026u32 << 16) | 194).unwrap();
        let mut record = PlayerRecord {
            build: GameBuild::Fm27Steam,
            addr: 0x20000,
            index: 0,
            name: None,
            raw: vec![],
            person_shift: 0,
            contract_raw: None,
            contract_addr: None,
            club_name: None,
            club_addr: None,
            nationality: None,
            birthplace: None,
            languages: None,
            game_date: Some(date),
            relations: ListState::Unmapped,
            injuries: ListState::Items(vec![Injury {
                addr: 0x30000,
                name: "a ruptured quad".into(),
                start: date,
                full_days: 236,
                light_days: 206,
                days_addr: 0x30028,
            }]),
            absences: ListState::Items(vec![Absence {
                addr: 0x40000,
                kind: 0x19,
                start: date,
                days: 595,
                days_addr: 0x40016,
            }]),
            clauses: ListState::Items(vec![Clause {
                addr: 0x50000,
                amount_addr: 0x50000,
                amount: 35_000,
                value: -1,
                clause_type: 0x20,
            }]),
        };
        assert!(is_player_list_write(&record, 0x30028, 4));
        assert!(is_player_list_write(&record, 0x40016, 2));
        assert!(is_player_list_write(&record, 0x50000, 4));
        assert!(!is_player_list_write(&record, 0x20000, 4));
        assert!(!is_player_list_write(&record, 0x40016, 4));
        assert!(!is_player_list_write(&record, 0x30029, 4));
        patch_player_list_buffers(&mut record, 0x30028, &[194, 0, 194, 0]);
        let ListState::Items(items) = &record.injuries else {
            panic!()
        };
        assert_eq!((items[0].full_days, items[0].light_days), (194, 194));
        // The old bytes selected for Undo restore both durations.
        patch_player_list_buffers(&mut record, 0x30028, &[236, 0, 206, 0]);
        patch_player_list_buffers(&mut record, 0x40016, &428u16.to_le_bytes());
        patch_player_list_buffers(&mut record, 0x50000, &50_000i32.to_le_bytes());
        // Invalid/unrelated/overlong addresses must not change an item.
        patch_player_list_buffers(&mut record, u64::MAX, &[0; 4]);
        patch_player_list_buffers(&mut record, 0x40016, &[0; 4]);
        let ListState::Items(items) = &record.injuries else {
            panic!()
        };
        assert_eq!((items[0].full_days, items[0].light_days), (236, 206));
        let ListState::Items(items) = &record.absences else {
            panic!()
        };
        assert_eq!(items[0].days, 428);
        let ListState::Items(items) = &record.clauses else {
            panic!()
        };
        assert_eq!(items[0].amount, 50_000);
    }

    #[test]
    fn acknowledgement_patches_only_complete_address_ranges() {
        let mut raw = [1, 2, 3, 4];
        patch_buffer(&mut raw, 100_000, 100_001, &[8, 9]);
        assert_eq!(raw, [1, 8, 9, 4]);
        patch_buffer(&mut raw, 100_000, 99_999, &[0]);
        patch_buffer(&mut raw, 100_000, 100_003, &[0, 0]);
        patch_buffer(&mut raw, 100_000, u64::MAX, &[0]);
        assert_eq!(raw, [1, 8, 9, 4]);
    }
    #[test]
    fn acknowledgements_preserve_shifted_player_coach_layout() {
        let mut raw = [1, 2, 3, 4, 5, 6];
        patch_player_buffer(&mut raw, 100_000, 100_001, &[8], 3, 10);
        patch_player_buffer(&mut raw, 100_000, 100_014, &[9], 3, 10);
        assert_eq!(raw, [1, 8, 3, 4, 9, 6]);
        // A physical address in the inserted block cannot patch normalized person bytes.
        patch_player_buffer(&mut raw, 100_000, 100_004, &[0], 3, 10);
        assert_eq!(raw, [1, 8, 3, 4, 9, 6]);
        patch_player_buffer(&mut raw, 100_000, 100_004, &[7], 3, 0);
        assert_eq!(raw, [1, 8, 3, 4, 7, 6]);
    }
}

fn cached_player_record(cached: &CachedPlayer, build: GameBuild) -> PlayerRecord {
    PlayerRecord {
        build,
        addr: cached.addr,
        index: cached.index,
        name: Some(cached.name.clone()),
        raw: cached.raw.clone(),
        person_shift: cached.person_shift,
        contract_raw: None,
        contract_addr: None,
        club_name: None,
        club_addr: None,
        nationality: None,
        birthplace: None,
        languages: None,
        game_date: None,
        injuries: crate::player::ListState::Unmapped,
        absences: crate::player::ListState::Unmapped,
        relations: crate::player::ListState::Unmapped,
        clauses: crate::player::ListState::Unmapped,
    }
}

fn cached_staff_view(cached: &CachedStaff, build: GameBuild) -> StaffView {
    StaffView {
        record: StaffRecord {
            build,
            addr: cached.addr,
            index: cached.index,
            raw: cached.raw.clone(),
            identity_shift: cached.identity_shift,
        },
        name: Some(cached.name.clone()),
        roles: Some(cached.roles),
        role_ptr: cached.role_ptr,
        nationality: None,
        club_name: None,
        club_addr: None,
        birthplace: None,
        languages: None,
        contract_raw: None,
        contract_addr: None,
    }
}

fn editor_title(build: Option<GameBuild>, branch: Option<&str>) -> String {
    let base = build.map_or_else(
        || "FM Editor".to_string(),
        |v| format!("{} Editor", v.display_name()),
    );
    match branch.filter(|b| !b.is_empty()) {
        Some(branch) => format!("{base} [{branch}]"),
        None => base,
    }
}

#[cfg(test)]
mod title_tests {
    use super::*;

    #[test]
    fn connected_epic_enables_editing_after_live_verification() {
        let mut app = FmEditorApp::from_settings(AppSettings::default());
        app.auto_connect = false;
        app.conn = ConnState::Ready(123);
        app.attached_build = Some(GameBuild::Fm24Epic);
        assert!(app.editing_allowed());
        assert_ne!(
            app.undo_unavailable_reason(),
            Some("Epic preview is read-only; editing and Undo are disabled.".into())
        );
        app.attached_build = Some(GameBuild::Fm24Steam);
        assert!(app.editing_allowed());
        app.conn = ConnState::Disconnected;
        assert!(!app.editing_allowed());
    }

    #[test]
    fn attached_version_and_worktree_are_both_visible() {
        assert_eq!(editor_title(None, None), "FM Editor");
        for version in GameBuild::ALL {
            assert_eq!(
                editor_title(Some(version), None),
                format!("{} Editor", version.display_name())
            );
            assert_eq!(
                editor_title(Some(version), Some("preview")),
                format!("{} Editor [preview]", version.display_name())
            );
        }
        assert_eq!(editor_title(None, Some("preview")), "FM Editor [preview]");
    }
}

fn history_player_entity(player: &PlayerRecord) -> EntityRef {
    EntityRef {
        kind: EntityKind::Player,
        build: player.build,
        address: player.addr,
        index: player.index,
        uid: player.field_u32("p_uid").filter(|id| *id != 0),
        name: player
            .name
            .clone()
            .unwrap_or_else(|| "Unknown player".into()),
    }
}

fn history_staff_entity(staff: &StaffView, manager: bool) -> EntityRef {
    EntityRef {
        kind: if manager || staff.record.identity_shift != 0 {
            EntityKind::HumanManager
        } else {
            EntityKind::Staff
        },
        build: staff.record.build,
        address: staff.record.addr,
        index: staff.record.index,
        uid: staff.record.field_u32("s_id").filter(|id| *id != 0),
        name: staff
            .name
            .clone()
            .unwrap_or_else(|| "Unknown staff member".into()),
    }
}

fn history_club_entity(club: &ClubView, build: GameBuild) -> EntityRef {
    EntityRef {
        kind: EntityKind::Club,
        build,
        address: club.addr,
        index: club.index,
        uid: (club.uid != 0).then_some(club.uid),
        name: club.full_name.clone(),
    }
}

/// A descriptive report address is never sufficient navigation authority.
fn history_navigation_target(
    cache: &EntityCache,
    build: Option<GameBuild>,
    target: &EntityRef,
) -> Option<HistoryEntry> {
    if build != Some(target.build) || !crate::player::is_valid_ptr(target.address) {
        return None;
    }
    let uid = target.uid.filter(|id| *id != 0)?;
    let (entity_type, name) =
        match target.kind {
            EntityKind::Player => {
                let p = cache.players.iter().find(|p| {
                    p.addr == target.address && p.index == target.index && p.uid == uid
                })?;
                (EntityTabType::Player, p.name.clone())
            }
            EntityKind::Staff | EntityKind::HumanManager => {
                let s = cache.staff.iter().find(|s| {
                    s.addr == target.address && s.index == target.index && s.uid == uid
                })?;
                (EntityTabType::Staff, s.name.clone())
            }
            EntityKind::Club => {
                let c = cache.clubs.iter().find(|c| {
                    c.addr == target.address && c.index == target.index && c.uid == uid
                })?;
                (EntityTabType::Club, c.name.clone())
            }
        };
    Some(HistoryEntry {
        entity_type,
        addr: target.address,
        index: target.index,
        display_name: name,
    })
}

fn history_raw_value(bytes: &[u8]) -> String {
    let value = bytes
        .iter()
        .take(32)
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "Raw bytes: {value}{}",
        if bytes.len() > 32 { " …" } else { "" }
    )
}

/// Only source-tagged parts share an action. Two unrelated controls committed
/// in one frame remain separate chronological actions.
fn history_actions(edits: Vec<field_widget::FieldEdit>) -> Vec<ActionDraft> {
    let mut actions: Vec<ActionDraft> = Vec::new();
    let mut groups: HashMap<(String, String, String, String), usize> = HashMap::new();
    for edit in edits {
        let field_widget::FieldEdit {
            addr,
            old_raw,
            new_raw,
            description,
            presentation,
        } = edit;
        let presentation = presentation.unwrap_or_else(|| {
            EditPresentation::new(
                description.clone(),
                history_raw_value(&old_raw),
                history_raw_value(&new_raw),
            )
        });
        let index = if let Some(group) = &presentation.group {
            let key = (
                group.key.clone(),
                group.label.clone(),
                group.before.clone(),
                group.after.clone(),
            );
            if let Some(index) = groups.get(&key) {
                *index
            } else {
                let index = actions.len();
                actions.push(ActionDraft {
                    label: group.label.clone(),
                    before: group.before.clone(),
                    after: group.after.clone(),
                    changes: Vec::new(),
                });
                groups.insert(key, index);
                index
            }
        } else {
            let index = actions.len();
            actions.push(ActionDraft {
                label: presentation.field.clone(),
                before: presentation.before.clone(),
                after: presentation.after.clone(),
                changes: Vec::new(),
            });
            index
        };
        actions[index].changes.push(ChangeDraft {
            entry: UndoEntry {
                description,
                addr,
                old_raw,
                new_raw,
                timestamp: Instant::now(),
                policy_guard: None,
            },
            presentation,
        });
    }
    actions
}

#[cfg(test)]
mod edit_history_integration_tests {
    use super::*;
    use crate::edit_history::{ActionStatus, EditGroup};

    fn test_app(bytes: &[u8]) -> FmEditorApp {
        // No eframe context, disk preferences, attach, reads or writes to a game.
        let mut app = FmEditorApp::from_settings(AppSettings::default());
        app.auto_connect = false;
        app.active_page = Page::ClubSearch;
        app.conn = ConnState::Ready(123);
        app.attached_build = Some(GameBuild::Fm27Steam);
        let club = ClubView {
            addr: 0x20000,
            index: 7,
            raw: bytes.to_vec(),
            uid: 42,
            full_name: "Club".into(),
            short_name: "Club".into(),
            info_raw: None,
            info_addr: None,
            finance_raw: None,
            finance_addr: None,
            stadium_raw: None,
            senior_team_raw: None,
            senior_team_addr: None,
            budget_raw: None,
            budget_addr: None,
            nickname: None,
            stadium_entity_raw: None,
            stadium_name: None,
            stadium_name2: None,
            nation: None,
        };
        app.club_snapshots.insert(club.addr, club.clone());
        app.selected_club = Some(club);
        app
    }

    fn club_identity() -> EntityRef {
        EntityRef {
            kind: EntityKind::Club,
            build: GameBuild::Fm27Steam,
            address: 0x20000,
            index: 7,
            uid: Some(42),
            name: "Club".into(),
        }
    }

    fn submit_edits(app: &mut FmEditorApp, edits: Vec<field_widget::FieldEdit>) -> Vec<u64> {
        app.change_history
            .submit(club_identity(), history_actions(edits))
            .into_iter()
            .map(|cmd| match cmd {
                BridgeCmd::WriteBytes { id, .. } => id,
                _ => panic!("expected write command"),
            })
            .collect()
    }

    fn assert_club_bytes(app: &FmEditorApp, expected: &[u8]) {
        assert_eq!(app.selected_club.as_ref().unwrap().raw, expected);
        assert_eq!(app.club_snapshots[&0x20000].raw, expected);
    }

    #[test]
    fn game_closed_restores_queued_scalar_and_ignores_late_acks_across_sessions() {
        use crate::edit_history::AttemptOutcome;
        let mut app = test_app(&[15]);
        let id = submit_edits(&mut app, vec![change(0x20000, "Scalar", None)])[0];
        app.handle_game_closed();
        assert!(matches!(app.conn, ConnState::ReadOnly(123)));
        assert_club_bytes(&app, &[12]);
        assert!(!app.manager_loading);
        assert!(!app.loading_entity);
        assert!(!app.change_history.is_pending());
        assert_eq!(app.change_history.undo_len(), 0);
        let report = &app.change_history.reports()[0];
        assert_eq!(report.status, ActionStatus::Refused);
        assert_eq!(
            report.changes[0].attempts[0].outcome,
            AttemptOutcome::Refused
        );
        assert!(report.session_closed.is_some());
        assert!(app.write_error.as_ref().unwrap().contains("queued"));
        let summary = app.status_msg.as_ref().unwrap().0.clone();
        app.complete_write(id, false, Some("No process".into()), Some(false));
        app.complete_write(id, true, None, None);
        assert_club_bytes(&app, &[12]);
        assert_eq!(app.status_msg.as_ref().unwrap().0, summary);
        // A new session may use the same object address, but never the old ID.
        app.conn = ConnState::Ready(456);
        let next = submit_edits(&mut app, vec![change(0x20000, "Next", None)])[0];
        assert!(next > id);
        app.complete_write(id, true, None, None);
        assert!(app.change_history.is_pending());
        assert_eq!(
            app.change_history.reports()[0].status,
            ActionStatus::Refused
        );
        assert_club_bytes(&app, &[12]);
    }

    #[test]
    fn game_closed_preserves_confirmed_part_of_mixed_group() {
        use crate::edit_history::AttemptOutcome;
        let mut app = test_app(&[15, 15]);
        let group = EditGroup::new("group", "Both", 12, 15);
        let ids = submit_edits(
            &mut app,
            vec![
                change(0x20000, "First", Some(group.clone())),
                change(0x20001, "Second", Some(group)),
            ],
        );
        app.complete_write(ids[0], true, None, None);
        app.handle_game_closed();
        assert_club_bytes(&app, &[15, 12]);
        let report = &app.change_history.reports()[0];
        assert_eq!(report.status, ActionStatus::PartiallyApplied);
        assert_eq!(
            report.changes[0].attempts[0].outcome,
            AttemptOutcome::Confirmed
        );
        assert_eq!(
            report.changes[1].attempts[0].outcome,
            AttemptOutcome::Refused
        );
        assert!(report.session_closed.is_some());
        assert_eq!(app.change_history.undo_len(), 0);
    }

    #[test]
    fn game_closed_refuses_pending_undo_without_rolling_back_confirmed_display() {
        use crate::edit_history::{AttemptOutcome, Operation};
        let mut app = test_app(&[15]);
        let id = submit_edits(&mut app, vec![change(0x20000, "Scalar", None)])[0];
        app.complete_write(id, true, None, None);
        assert_eq!(app.change_history.undo_len(), 1);
        let commands = app.change_history.undo_latest().unwrap();
        let BridgeCmd::WriteBytes { id: undo_id, .. } = commands[0] else {
            panic!()
        };
        app.handle_game_closed();
        assert_club_bytes(&app, &[15]);
        let report = &app.change_history.reports()[0];
        assert_eq!(report.status, ActionStatus::UndoRefused);
        let attempt = report.changes[0].attempts.last().unwrap();
        assert_eq!(attempt.operation, Operation::Undo);
        assert_eq!(attempt.outcome, AttemptOutcome::Refused);
        assert!(report.session_closed.is_some());
        assert_eq!(app.change_history.undo_len(), 0);
        app.complete_write(undo_id, true, None, None);
        assert_club_bytes(&app, &[15]);
        assert_eq!(
            app.change_history.reports()[0].status,
            ActionStatus::UndoRefused
        );
    }

    #[test]
    fn game_closed_rolls_back_overlapping_queued_edits_in_reverse_id_order() {
        let mut app = test_app(&[18]);
        let first = change(0x20000, "First", None);
        let mut second = change(0x20000, "Second", None);
        second.old_raw = vec![15];
        second.new_raw = vec![18];
        submit_edits(&mut app, vec![first, second]);
        app.handle_game_closed();
        assert_club_bytes(&app, &[12]);
        assert!(
            app.change_history
                .reports()
                .iter()
                .all(|r| r.status == ActionStatus::Refused)
        );
    }

    #[test]
    fn generic_session_clear_keeps_unknown_audit_and_discards_selected_snapshots() {
        use crate::edit_history::AttemptOutcome;
        for reason in [
            "Save unloaded or replaced",
            "Reconnected or attached to a process/build",
            "Lost connection",
        ] {
            let mut app = test_app(&[15]);
            let id = submit_edits(&mut app, vec![change(0x20000, "Scalar", None)])[0];
            app.clear_entity_state(reason);
            assert!(app.selected_club.is_none());
            assert!(app.selected_player.is_none());
            assert!(app.selected_staff.is_none());
            assert!(app.club_snapshots.is_empty());
            assert!(app.player_snapshots.is_empty());
            assert!(app.staff_snapshots.is_empty());
            assert_eq!(app.change_history.undo_len(), 0);
            assert!(!app.change_history.is_pending());
            let report = &app.change_history.reports()[0];
            assert_eq!(report.status, ActionStatus::Unconfirmed);
            assert_eq!(
                report.changes[0].attempts[0].outcome,
                AttemptOutcome::Interrupted
            );
            assert_eq!(report.session_closed.as_deref(), Some(reason));
            app.complete_write(id, false, Some("No process".into()), Some(false));
            assert_eq!(
                app.change_history.reports()[0].changes[0].attempts[0].outcome,
                AttemptOutcome::Interrupted
            );
        }
    }

    #[test]
    fn unsubmitted_optimistic_edits_restore_original_bytes_without_audit_or_authority() {
        let mut app = test_app(&[18]);
        let first = change(0x20000, "First", None);
        let mut second = change(0x20000, "Second", None);
        second.old_raw = vec![15];
        second.new_raw = vec![18];
        app.rollback_unsubmitted_edits(history_actions(vec![first, second]));
        assert_club_bytes(&app, &[12]);
        assert!(app.change_history.reports().is_empty());
        assert!(!app.change_history.is_pending());
        assert_eq!(app.change_history.undo_len(), 0);
        assert!(app.write_error.as_ref().unwrap().contains("not submitted"));
    }

    fn change(addr: u64, field: &str, group: Option<EditGroup>) -> field_widget::FieldEdit {
        field_widget::FieldEdit {
            addr,
            old_raw: vec![12],
            new_raw: vec![15],
            description: field.into(),
            presentation: Some(EditPresentation {
                field: field.into(),
                before: "12".into(),
                after: "15".into(),
                group,
            }),
        }
    }

    #[test]
    fn explicit_groups_are_one_action_but_unrelated_controls_are_not_frame_grouped() {
        let group = EditGroup::new("facility:123:youth", "Youth recruitment", 12, 15);
        let actions = history_actions(vec![
            change(0x20000, "Primary", Some(group.clone())),
            change(0x30000, "Mirror", Some(group)),
            change(0x40000, "Passing", None),
            change(0x50000, "Finishing", None),
        ]);
        assert_eq!(actions.len(), 3);
        assert_eq!(actions[0].changes.len(), 2);
        assert_eq!(actions[0].label, "Youth recruitment");
        assert_eq!(actions[1].label, "Passing");
        assert_eq!(actions[2].label, "Finishing");
        let mut history = ChangeHistory::new();
        let entity = EntityRef {
            kind: EntityKind::Club,
            build: GameBuild::Fm27Steam,
            address: 0x20000,
            index: 0,
            uid: Some(42),
            name: "Club".into(),
        };
        let commands = history.submit(entity, actions);
        for command in commands {
            let BridgeCmd::WriteBytes { id, .. } = command else {
                panic!()
            };
            history.complete(id, true, None, None).unwrap();
        }
        assert_eq!(history.undo_len(), 3);
        assert_eq!(history.reports()[0].status, ActionStatus::Applied);
        // Undo the two independent controls first, then both facility copies.
        for expected_parts in [1, 1, 2] {
            let commands = history.undo_latest().unwrap();
            assert_eq!(commands.len(), expected_parts);
            for command in commands {
                let BridgeCmd::WriteBytes { id, .. } = command else {
                    panic!()
                };
                history.complete(id, true, None, None).unwrap();
            }
        }
        assert_eq!(history.undo_len(), 0);
        assert_eq!(history.reports().len(), 3);
        assert!(
            history
                .reports()
                .iter()
                .all(|r| r.status == ActionStatus::Undone)
        );
    }

    #[test]
    fn unformatted_fallback_is_labelled_raw_and_inconsistent_group_summaries_do_not_merge() {
        let mut raw = change(0x20000, "Custom", None);
        raw.presentation = None;
        let first = EditGroup::new("same key", "Date", "1 Jan 2026", "2 Jan 2026");
        let other = EditGroup::new("same key", "Date", "2 Jan 2026", "3 Jan 2026");
        let actions = history_actions(vec![
            raw,
            change(0x30000, "Day", Some(first)),
            change(0x30002, "Year", Some(other)),
        ]);
        assert_eq!(actions.len(), 3);
        assert_eq!(actions[0].before, "Raw bytes: 0C");
        assert_eq!(actions[0].after, "Raw bytes: 0F");
    }

    #[test]
    fn history_navigation_requires_current_build_and_exact_cached_object_identity() {
        let cache = Arc::new(EntityCache {
            players: vec![],
            staff: vec![],
            my_clubs: None,
            clubs: vec![CachedClub {
                addr: 0x20000,
                index: 7,
                uid: 42,
                name: "Current name".into(),
                full_name: "Full current name".into(),
                gender: 0,
                transfer_budget: None,
                nation_id: None,
                status: None,
                reputation: None,
            }],
            club_names: HashMap::new(),
        });
        let target = EntityRef {
            kind: EntityKind::Club,
            build: GameBuild::Fm27Steam,
            address: 0x20000,
            index: 7,
            uid: Some(42),
            name: "Old name".into(),
        };
        let resolved = history_navigation_target(&cache, Some(target.build), &target).unwrap();
        assert_eq!(resolved.display_name, "Current name");
        let mut app = test_app(&[12]);
        app.entity_cache = Some(cache.clone());
        for state in [
            ConnState::ReadOnly(123),
            ConnState::WaitingForSave(123),
            ConnState::Disconnected,
        ] {
            app.conn = state;
            assert!(!app.open_edit_history_entity(target.clone()));
            assert!(app.open_tabs.is_empty());
            assert!(app.active_tab.is_none());
            assert!(!app.loading_entity);
            assert_club_bytes(&app, &[12]);
        }
        let wrong = EntityRef {
            address: target.address + 8,
            ..target.clone()
        };
        assert!(history_navigation_target(&cache, Some(target.build), &wrong).is_none());
        assert!(history_navigation_target(&cache, Some(GameBuild::Fm26Steam), &target).is_none());
        for (uid, index, kind) in [
            (Some(43), 7, EntityKind::Club),
            (None, 7, EntityKind::Club),
            (Some(42), 8, EntityKind::Club),
            (Some(42), 7, EntityKind::Player),
        ] {
            let wrong = EntityRef {
                uid,
                index,
                kind,
                ..target.clone()
            };
            assert!(history_navigation_target(&cache, Some(target.build), &wrong).is_none());
        }
    }
}
