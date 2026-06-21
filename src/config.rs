use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// File name written next to the host `.exe`.
const CONFIG_FILE: &str = "imbdmlive.toml";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Bilibili live room id to connect to.
    pub room_id: String,
    /// Optional cookie string.
    /// When `None`, blivedm attempts browser auto-detection;
    pub cookies: Option<String>,

    /// Font size in pixels.
    pub font_size: f32,

    /// Overlay window background opacity (0.0–1.0).
    pub opacity: f32,
    /// Overlay window top-left position in pixels.
    pub pos: [f32; 2],
    /// Overlay window size in pixels.
    pub size: [f32; 2],

    /// Key that toggles the config window.
    /// One of the names in [`toggle_key_to_imgui`] (default `Insert`).
    pub toggle_key: String,

    /// Global log level. One of: off, error, warn, info, debug, trace.
    pub log_level: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            room_id: "1597760".to_string(),
            cookies: None,
            font_size: 20.0,
            opacity: 0.55,
            pos: [16.0, 16.0],
            size: [420.0, 320.0],
            toggle_key: "Insert".to_string(),
            log_level: "info".to_string(),
        }
    }
}

impl Config {
    /// Directory of the host executable; the config lives here.
    fn dir() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub fn path() -> PathBuf {
        Self::dir().join(CONFIG_FILE)
    }

    /// Load the config with default fallback
    pub fn load() -> Self {
        let path = Self::path();
        match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str::<Config>(&text) {
                Ok(cfg) => cfg,
                Err(e) => {
                    log::warn!("Invalid {}: {e}; using defaults", path.display());
                    Config::default()
                }
            },
            Err(_) => {
                let cfg = Config::default();
                let _ = cfg.save();
                cfg
            }
        }
    }

    /// Persist the config to disk.
    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))
    }

    pub fn log_level_filter(&self) -> log::LevelFilter {
        parse_log_level(&self.log_level).unwrap_or(log::LevelFilter::Info)
    }
}

pub fn parse_log_level(level: &str) -> Option<log::LevelFilter> {
    match level.trim().to_ascii_lowercase().as_str() {
        "off" => Some(log::LevelFilter::Off),
        "error" => Some(log::LevelFilter::Error),
        "warn" | "warning" => Some(log::LevelFilter::Warn),
        "info" => Some(log::LevelFilter::Info),
        "debug" => Some(log::LevelFilter::Debug),
        "trace" => Some(log::LevelFilter::Trace),
        _ => None,
    }
}

/// Map a configured key name to an imgui [`hudhook::imgui::Key`].
/// Returns `None` for unknown names.
pub fn toggle_key_to_imgui(name: &str) -> Option<hudhook::imgui::Key> {
    use hudhook::imgui::Key;
    let key = match name.trim().to_ascii_uppercase().as_str() {
        "F1" => Key::F1,
        "F2" => Key::F2,
        "F3" => Key::F3,
        "F4" => Key::F4,
        "F5" => Key::F5,
        "F6" => Key::F6,
        "F7" => Key::F7,
        "F8" => Key::F8,
        "F9" => Key::F9,
        "F10" => Key::F10,
        "F11" => Key::F11,
        "F12" => Key::F12,
        "INSERT" | "INS" => Key::Insert,
        "HOME" => Key::Home,
        "END" => Key::End,
        "PAGEUP" | "PGUP" => Key::PageUp,
        "PAGEDOWN" | "PGDN" => Key::PageDown,
        _ => return None,
    };
    Some(key)
}
