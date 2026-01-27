//! # Configuration Module
//!
//! Handles user configuration persistence and defines the available Bing regional markets.
//!
//! ## Configuration Storage
//! User preferences are stored as JSON in:
//! `~/.config/cosmic-bing-wallpaper/config.json`
//!
//! ## Bing Markets
//! Bing provides different daily images for different regional markets. This module
//! defines 21 supported markets across North America, Europe, Asia, and beyond.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Get the app config directory path.
///
/// In Flatpak, we use the exposed host config directory rather than XDG_CONFIG_HOME
/// because Flatpak remaps XDG_CONFIG_HOME to ~/.var/app/APP_ID/config/.
///
/// Returns `~/.config/cosmic-bing-wallpaper/` in both native and Flatpak environments.
pub fn app_config_dir() -> Option<PathBuf> {
    if std::path::Path::new("/.flatpak-info").exists() {
        // In Flatpak, use the exposed host config directory
        dirs::home_dir().map(|h| h.join(".config/cosmic-bing-wallpaper"))
    } else {
        // Native: use standard XDG config directory
        dirs::config_dir().map(|d| d.join("cosmic-bing-wallpaper"))
    }
}

/// Represents a Bing regional market.
///
/// Bing serves different "Image of the Day" content based on geographic region.
/// Each market has a unique code (e.g., "en-US") used in API requests.
#[derive(Debug, Clone, Copy)]
pub struct Market {
    /// The market code used in Bing API requests (e.g., "en-US", "de-DE")
    pub code: &'static str,
    /// Human-readable market name for display in the UI
    pub name: &'static str,
}

/// All supported Bing regional markets (sorted alphabetically by name).
///
/// Each market may show a different daily image. The US market ("en-US") is the
/// default and typically has the most consistent image availability.
pub const MARKETS: &[Market] = &[
    Market { code: "en-AU", name: "Australia" },
    Market { code: "pt-BR", name: "Brazil" },
    Market { code: "en-CA", name: "Canada" },
    Market { code: "zh-CN", name: "China" },
    Market { code: "da-DK", name: "Denmark" },
    Market { code: "fi-FI", name: "Finland" },
    Market { code: "fr-FR", name: "France" },
    Market { code: "de-DE", name: "Germany" },
    Market { code: "en-IN", name: "India" },
    Market { code: "it-IT", name: "Italy" },
    Market { code: "ja-JP", name: "Japan" },
    Market { code: "nl-NL", name: "Netherlands" },
    Market { code: "en-NZ", name: "New Zealand" },
    Market { code: "nb-NO", name: "Norway" },
    Market { code: "pl-PL", name: "Poland" },
    Market { code: "ru-RU", name: "Russia" },
    Market { code: "ko-KR", name: "South Korea" },
    Market { code: "es-ES", name: "Spain" },
    Market { code: "sv-SE", name: "Sweden" },
    Market { code: "en-GB", name: "United Kingdom" },
    Market { code: "en-US", name: "United States" },
];

/// User configuration for the application.
///
/// Persisted to `~/.config/cosmic-bing-wallpaper/config.json` as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Directory where downloaded wallpapers are stored.
    /// Defaults to `~/Pictures/BingWallpapers/`
    pub wallpaper_dir: String,
    /// Selected Bing market code (e.g., "en-US").
    /// Determines which regional image is fetched.
    pub market: String,
    /// Whether automatic daily updates are enabled.
    /// Note: This flag is stored but the actual timer state is in timer_state.json.
    pub auto_update: bool,
    /// Number of days to keep old wallpapers before automatic cleanup.
    /// Set to 0 to keep wallpapers forever. Cleanup runs after each download.
    pub keep_days: u32,
    /// Whether to automatically fetch today's image when the app starts.
    /// Disable for metered connections or manual-only operation.
    #[serde(default = "default_fetch_on_startup")]
    pub fetch_on_startup: bool,
}

fn default_fetch_on_startup() -> bool {
    true
}

impl Default for Config {
    /// Creates a default configuration.
    ///
    /// - `wallpaper_dir`: `~/Pictures/BingWallpapers/`
    /// - `market`: "en-US" (United States)
    /// - `auto_update`: false
    /// - `keep_days`: 30
    fn default() -> Self {
        let wallpaper_dir = dirs::picture_dir()
            .unwrap_or_else(|| PathBuf::from("~"))
            .join("BingWallpapers")
            .to_string_lossy()
            .to_string();

        Self {
            wallpaper_dir,
            market: "en-US".to_string(),
            auto_update: false,
            keep_days: 30,
            fetch_on_startup: true,
        }
    }
}

/// Expands a leading tilde (~) in a path to the user's home directory.
///
/// This allows users to manually edit config.json with paths like "~/Pictures/..."
fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return format!("{}{}", home.display(), &path[1..]);
        }
    } else if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home.to_string_lossy().to_string();
        }
    }
    path.to_string()
}

impl Config {
    /// Returns the path to the configuration file.
    ///
    /// The config is stored at `~/.config/cosmic-bing-wallpaper/config.json`
    fn config_path() -> Option<PathBuf> {
        app_config_dir().map(|p| p.join("config.json"))
    }

    /// Loads the configuration from disk.
    ///
    /// If the config file doesn't exist or cannot be parsed, returns default values.
    /// This ensures the application always starts with valid configuration.
    /// Tilde (~) in paths will be expanded to the user's home directory.
    pub fn load() -> Self {
        let mut config: Config = Self::config_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default();

        // Expand tilde in wallpaper_dir for users who manually edit the config
        config.wallpaper_dir = expand_tilde(&config.wallpaper_dir);
        config
    }

    /// Persists the current configuration to disk.
    ///
    /// Creates the config directory if it doesn't exist. The configuration is
    /// stored as pretty-printed JSON for easy manual editing if needed.
    ///
    /// # Errors
    /// Returns an error message if:
    /// - The config directory cannot be determined
    /// - Directory creation fails
    /// - JSON serialization fails
    /// - File write fails
    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path()
            .ok_or("Could not determine config path")?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config dir: {}", e))?;
        }

        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        std::fs::write(path, content)
            .map_err(|e| format!("Failed to write config: {}", e))?;

        Ok(())
    }
}
