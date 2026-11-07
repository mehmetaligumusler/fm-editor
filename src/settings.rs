//! Persistent user preferences: theme, UI scale, wage display, and face packs.
//!
//! Stored at `%APPDATA%/fm-editor/settings.json` on Windows.
//! Falls back to defaults on missing or corrupt file.

use eframe::egui;
use serde::{Deserialize, Serialize};

// ─── Theme ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Theme {
    #[default]
    Dark,
    Light,
    DarkBlue,
}

impl Theme {
    pub fn label(self) -> &'static str {
        match self {
            Theme::Dark => "Dark",
            Theme::Light => "Light",
            Theme::DarkBlue => "Dark Blue",
        }
    }

    pub fn visuals(self) -> egui::Visuals {
        match self {
            Theme::Dark => egui::Visuals::dark(),
            Theme::Light => egui::Visuals::light(),
            Theme::DarkBlue => dark_blue_visuals(),
        }
    }
}

/// Navy/steel-blue dark theme.
fn dark_blue_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.window_fill = egui::Color32::from_rgb(15, 25, 45);
    v.panel_fill = egui::Color32::from_rgb(20, 32, 56);
    v.faint_bg_color = egui::Color32::from_rgb(25, 40, 70);
    v.extreme_bg_color = egui::Color32::from_rgb(10, 18, 35);
    v
}

// ─── Scale ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Scale {
    #[default]
    S100,
    S125,
    S150,
}

impl Scale {
    pub fn zoom_factor(self) -> f32 {
        match self {
            Scale::S100 => 1.0,
            Scale::S125 => 1.1,
            Scale::S150 => 1.2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Scale::S100 => "100%",
            Scale::S125 => "125%",
            Scale::S150 => "150%",
        }
    }

    pub fn from_index(i: u8) -> Self {
        match i {
            1 => Scale::S125,
            2 => Scale::S150,
            _ => Scale::S100,
        }
    }

    pub fn to_index(self) -> u8 {
        match self {
            Scale::S100 => 0,
            Scale::S125 => 1,
            Scale::S150 => 2,
        }
    }
}

// ─── Currency ─────────────────────────────────────────────────────────────────

/// Display currency. FM stores all money as GBP internally; the Euro rate is
/// the fixed conversion witnessed in the user's game (2026-11-07:
/// 4,100,645 internal → 4,782,290 € on the finance screen, i.e. ×1.1662286).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Currency {
    #[default]
    Gbp,
    Eur,
}

impl Currency {
    pub fn label(self) -> &'static str {
        match self {
            Currency::Gbp => "£ GBP",
            Currency::Eur => "€ EUR",
        }
    }

    /// Internal GBP → display currency multiplier.
    pub fn factor(self) -> f64 {
        match self {
            Currency::Gbp => 1.0,
            Currency::Eur => 1.166_228_8,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Currency::Gbp => "£",
            Currency::Eur => "€",
        }
    }
}

// ─── Wage Period ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WagePeriod {
    #[default]
    Weekly,
    Monthly,
    Yearly,
}

impl WagePeriod {
    /// Weekly = 1; monthly ≈ ×4.333; yearly = ×52 (matches format_wage).
    pub fn factor(self) -> f64 {
        match self {
            WagePeriod::Weekly => 1.0,
            WagePeriod::Monthly => 4.333,
            WagePeriod::Yearly => 52.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            WagePeriod::Weekly => "Weekly",
            WagePeriod::Monthly => "Monthly",
            WagePeriod::Yearly => "Yearly",
        }
    }
}

/// Format a weekly wage value according to the chosen display period.
/// FM stores wages as weekly integers. Monthly ≈ ×4.333, yearly ≈ ×52.
pub fn format_wage(
    weekly: u32,
    period: WagePeriod,
    lang: crate::i18n::Language,
    currency: Currency,
) -> String {
    let amount = (weekly as f64 * period.factor() * currency.factor()).round() as u64;
    let suffix = crate::i18n::wage_suffix(period, lang);
    format!(
        "{} {}{}",
        crate::i18n::thousands(amount),
        currency.symbol(),
        suffix
    )
}

// ─── AppSettings ─────────────────────────────────────────────────────────────

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppSettings {
    pub theme: Theme,
    pub scale: Scale,
    /// UI language. English by default; Türkçe follows FM's own Turkish
    /// game terms (see `i18n`).
    #[serde(default)]
    pub language: crate::i18n::Language,
    #[serde(default)]
    pub wage_period: WagePeriod,
    /// Money display currency (memory values stay GBP).
    #[serde(default)]
    pub currency: Currency,
    /// Directory containing `<uid>.png` face pack images.
    /// None = face images disabled.
    #[serde(default)]
    pub face_pack_path: Option<String>,
    /// Re-read entity data from memory each time a tab is switched to.
    /// Default on — disable if the spinner is distracting on a slow save.
    #[serde(default = "default_true")]
    pub reload_on_tab_switch: bool,
}

impl AppSettings {
    /// Load from disk, falling back to defaults on any error.
    pub fn load() -> Self {
        let Some(path) = settings_path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    /// Save to disk. Silently ignores I/O errors.
    pub fn save(&self) {
        let Some(path) = settings_path() else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }
}

fn settings_path() -> Option<std::path::PathBuf> {
    Some(dirs::config_dir()?.join("fm-editor").join("settings.json"))
}

// ─── Egui data key ────────────────────────────────────────────────────────────

/// Key used to store the active `Theme` in egui's per-frame temp data so
/// widget code can read it without needing an explicit parameter.
pub fn theme_data_id() -> egui::Id {
    egui::Id::new("fm_active_theme")
}
