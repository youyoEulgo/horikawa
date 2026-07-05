//! Application settings management
//!
//! Handles persistent settings for features like Discord Rich Presence and SMTC.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Application settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Enable Discord Rich Presence integration
    pub discord_enabled: bool,

    /// Enable System Media Transport Controls (Windows)
    pub smtc_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            discord_enabled: true,
            smtc_enabled: true,
        }
    }
}

impl Settings {
    /// Returns the path to the settings file.
    fn settings_path() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("horikawa").join("settings.json"))
    }

    /// Loads settings from disk, or returns defaults if not found.
    pub fn load() -> Self {
        let path = match Self::settings_path() {
            Some(p) => p,
            None => return Self::default(),
        };

        if !path.exists() {
            return Self::default();
        }

        match fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(e) => {
                tracing::warn!("Failed to read settings: {}", e);
                Self::default()
            }
        }
    }}
