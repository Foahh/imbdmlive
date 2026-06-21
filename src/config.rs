use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// File name written next to the host `.exe`.
const CONFIG_FILE: &str = "imbdmlive.toml";

/// Corner / edge of the screen the overlay window is pinned to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Anchor {
    #[default]
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Anchor {
    /// Compute the overlay window's top-left position in pixels.
    pub fn window_pos(self, display: [f32; 2], size: [f32; 2], offset: [f32; 2]) -> [f32; 2] {
        use Anchor::*;
        let x = match self {
            TopLeft | Left | BottomLeft => offset[0],
            Top | Center | Bottom => (display[0] - size[0]) / 2.0 + offset[0],
            TopRight | Right | BottomRight => display[0] - size[0] - offset[0],
        };
        let y = match self {
            TopLeft | Top | TopRight => offset[1],
            Left | Center | Right => (display[1] - size[1]) / 2.0 + offset[1],
            BottomLeft | Bottom | BottomRight => display[1] - size[1] - offset[1],
        };
        [x, y]
    }
}

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
    /// Corner / edge the overlay window is pinned to.
    pub anchor: Anchor,
    /// Pixel offset from the anchored edge/corner (inward).
    pub offset: [f32; 2],
    /// Overlay window size in pixels.
    pub size: [f32; 2],

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
            anchor: Anchor::TopLeft,
            offset: [16.0, 16.0],
            size: [420.0, 320.0],
            log_level: "info".to_string(),
        }
    }
}

impl Config {
    /// Directory of the host executable, where the config file is stored.
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

