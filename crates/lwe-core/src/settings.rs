//! Application settings (GUI-managed)
//!
//! Settings for the GUI application, stored in ~/.config/wayvid/settings.yaml.
//! This is the new GUI-first configuration format.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::types::{LayoutMode, RenderBackend};

/// Application settings (new GUI-first format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// Start application on login
    #[serde(default)]
    pub autostart: bool,

    /// Minimize to system tray when closing window
    #[serde(default = "default_minimize_to_tray")]
    pub minimize_to_tray: bool,

    /// UI language (e.g., "en", "zh-CN")
    #[serde(default = "default_language")]
    pub language: String,

    /// UI theme
    #[serde(default)]
    pub theme: Theme,

    /// Render backend selection
    #[serde(default)]
    pub render_backend: RenderBackend,

    /// FPS limit (0 = unlimited)
    #[serde(default)]
    pub fps_limit: Option<u32>,

    /// Pause wallpaper when a fullscreen app is detected
    #[serde(default = "default_pause_on_fullscreen")]
    pub pause_on_fullscreen: bool,

    /// Pause wallpaper when on battery power
    #[serde(default)]
    pub pause_on_battery: bool,

    /// Wallpaper folders to scan
    #[serde(default)]
    pub wallpaper_folders: Vec<PathBuf>,

    /// Enable Steam Workshop scanning
    #[serde(default = "default_steam_workshop_enabled")]
    pub steam_workshop_enabled: bool,

    /// Custom Steam path (if not auto-detected)
    #[serde(default)]
    pub steam_path: Option<PathBuf>,

    /// Currently active wallpapers per output
    #[serde(default)]
    pub active_wallpapers: HashMap<String, ActiveWallpaper>,

    /// Window state (for restoration)
    #[serde(default)]
    pub window_state: WindowState,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            autostart: false,
            minimize_to_tray: true,
            language: default_language(),
            theme: Theme::default(),
            render_backend: RenderBackend::default(),
            fps_limit: None,
            pause_on_fullscreen: true,
            pause_on_battery: false,
            wallpaper_folders: Vec::new(),
            steam_workshop_enabled: true,
            steam_path: None,
            active_wallpapers: HashMap::new(),
            window_state: WindowState::default(),
        }
    }
}

impl AppSettings {
    /// Get the settings file path
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("wayvid")
            .join("settings.yaml")
    }

    /// Load settings from file, or return default if not exists
    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            match Self::from_file(&path) {
                Ok(settings) => return settings,
                Err(e) => {
                    eprintln!("Warning: Failed to load settings: {}", e);
                }
            }
        }
        Self::default()
    }

    /// Load settings from a specific file
    pub fn from_file(path: &PathBuf) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Cannot read settings file: {:?}", path))?;

        let settings: Self = serde_yaml::from_str(&content)
            .with_context(|| format!("Invalid YAML in settings file: {:?}", path))?;

        Ok(settings)
    }

    /// Save settings to file
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();

        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory: {:?}", parent))?;
        }

        let content = serde_yaml::to_string(self).context("Failed to serialize settings")?;
        fs::write(&path, content)
            .with_context(|| format!("Failed to write settings file: {:?}", path))?;

        Ok(())
    }

    /// Set active wallpaper for an output
    pub fn set_active_wallpaper(&mut self, output: &str, wallpaper: ActiveWallpaper) {
        self.active_wallpapers.insert(output.to_string(), wallpaper);
    }

    /// Remove active wallpaper for an output
    pub fn clear_active_wallpaper(&mut self, output: &str) {
        self.active_wallpapers.remove(output);
    }

    /// Get active wallpaper for an output
    pub fn get_active_wallpaper(&self, output: &str) -> Option<&ActiveWallpaper> {
        self.active_wallpapers.get(output)
    }
}

/// Active wallpaper configuration for a specific output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveWallpaper {
    /// Wallpaper ID (from library) or source path
    pub source_id: String,

    /// Fallback source path (if library DB is unavailable)
    pub source_path: String,

    /// Layout mode
    #[serde(default)]
    pub layout: LayoutMode,

    /// Volume level (0.0 - 1.0)
    #[serde(default)]
    pub volume: f32,

    /// Playback rate (speed)
    #[serde(default = "default_playback_rate")]
    pub playback_rate: f32,

    /// Whether audio is muted
    #[serde(default = "default_muted")]
    pub muted: bool,
}

impl Default for ActiveWallpaper {
    fn default() -> Self {
        Self {
            source_id: String::new(),
            source_path: String::new(),
            layout: LayoutMode::Fill,
            volume: 0.0,
            playback_rate: 1.0,
            muted: true,
        }
    }
}

/// UI theme
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Follow system theme
    #[default]
    System,
    /// Light theme
    Light,
    /// Dark theme
    Dark,
}

impl Theme {
    pub fn display_name(&self) -> &'static str {
        match self {
            Theme::System => "System",
            Theme::Light => "Light",
            Theme::Dark => "Dark",
        }
    }
}

/// Window state for restoration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    /// Window position (x, y)
    pub position: Option<(i32, i32)>,

    /// Window size (width, height)
    pub size: Option<(u32, u32)>,

    /// Whether window was maximized
    pub maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            position: None,
            size: Some((1200, 800)),
            maximized: false,
        }
    }
}

// Default value functions
fn default_minimize_to_tray() -> bool {
    true
}

fn default_language() -> String {
    // Try to detect system locale
    if let Some(locale) = sys_locale::get_locale()
        && locale.starts_with("zh")
    {
        return "zh-CN".to_string();
    }
    "en".to_string()
}

fn default_pause_on_fullscreen() -> bool {
    true
}

fn default_steam_workshop_enabled() -> bool {
    true
}

fn default_playback_rate() -> f32 {
    1.0
}

fn default_muted() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let settings = AppSettings::default();
        assert!(!settings.autostart);
        assert!(settings.minimize_to_tray);
        assert!(settings.steam_workshop_enabled);
    }

    #[test]
    fn test_settings_serialization() {
        let settings = AppSettings::default();
        let yaml = serde_yaml::to_string(&settings).unwrap();
        let loaded: AppSettings = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(loaded.autostart, settings.autostart);
    }
}
