use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub hints: HintConfig,
    pub colors: ColorConfig,
    pub behavior: BehaviorConfig,
    pub scroll: ScrollConfig,
}

/// Hint display configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HintConfig {
    /// Characters used for hints (in priority order)
    pub chars: String,
    /// Font size in pixels
    pub font_size: u32,
    /// Font family
    pub font_family: String,
    /// Padding inside hint box
    pub padding: u32,
}

/// Color configuration (hex strings like "#RRGGBB" or "#RRGGBBAA")
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorConfig {
    /// Overlay background color
    pub background: String,
    /// Hint box background
    pub hint_bg: String,
    /// Hint text color (unmatched portion)
    pub hint_text: String,
    /// Matched prefix color
    pub hint_text_matched: String,
    /// Input display background
    pub input_bg: String,
    /// Input display text
    pub input_text: String,
}

/// Behavior configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BehaviorConfig {
    /// Auto-select when only one element matches
    pub auto_select: bool,
    /// Exit after clicking (vs stay for another action)
    pub exit_on_click: bool,
    /// Default action mode
    pub default_mode: ActionMode,
    /// Show element names in hints
    pub show_element_names: bool,
}

/// Scroll mode configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScrollConfig {
    /// Pixels to scroll per hjkl press
    pub scroll_step: i32,
    /// Pixels to scroll per Ctrl+d/u
    pub page_step: i32,
    /// Smooth scrolling (multiple small steps)
    pub smooth: bool,
}

/// Action modes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ActionMode {
    /// Normal click mode
    #[default]
    Click,
    /// Right-click mode
    RightClick,
    /// Middle-click mode
    MiddleClick,
    /// Scroll mode
    Scroll,
    /// Focus text fields
    Text,
    /// Drag mode
    Drag,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hints: HintConfig::default(),
            colors: ColorConfig::default(),
            behavior: BehaviorConfig::default(),
            scroll: ScrollConfig::default(),
        }
    }
}

impl Default for HintConfig {
    fn default() -> Self {
        Self {
            chars: "asdfghjklqwertyuiopzxcvbnm".to_string(),
            font_size: 14,
            font_family: "monospace".to_string(),
            padding: 4,
        }
    }
}

impl Default for ColorConfig {
    fn default() -> Self {
        Self {
            background: "#00000080".to_string(),
            hint_bg: "#ffffff".to_string(),
            hint_text: "#000000".to_string(),
            hint_text_matched: "#888888".to_string(),
            input_bg: "#ffffffee".to_string(),
            input_text: "#000000".to_string(),
        }
    }
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            auto_select: true,
            exit_on_click: true,
            default_mode: ActionMode::Click,
            show_element_names: false,
        }
    }
}

impl Default for ScrollConfig {
    fn default() -> Self {
        Self {
            scroll_step: 50,
            page_step: 500,
            smooth: true,
        }
    }
}

impl Config {
    /// Load config from default location or return defaults
    pub fn load() -> Self {
        Self::load_from_path(Self::config_path()).unwrap_or_default()
    }

    /// Load config from specific path
    pub fn load_from_path(path: PathBuf) -> Result<Self> {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config from {:?}", path))?;
        toml::from_str(&content).context("Failed to parse config file")
    }

    /// Get the default config file path
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("vimium-linux")
            .join("config.toml")
    }

    /// Save config to default location
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        fs::write(&path, content)?;
        Ok(())
    }

    /// Create a default config file if it doesn't exist
    pub fn ensure_default_exists() -> Result<()> {
        let path = Self::config_path();
        if !path.exists() {
            Config::default().save()?;
        }
        Ok(())
    }
}

/// Parse a hex color string to RGBA components (0-255)
pub fn parse_color(hex: &str) -> (u8, u8, u8, u8) {
    let hex = hex.trim_start_matches('#');
    let len = hex.len();

    if len == 6 {
        // RGB
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
        (r, g, b, 255)
    } else if len == 8 {
        // RGBA
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
        let a = u8::from_str_radix(&hex[6..8], 16).unwrap_or(255);
        (r, g, b, a)
    } else {
        // Invalid, return black
        (0, 0, 0, 255)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_color_rgb() {
        assert_eq!(parse_color("#ff0000"), (255, 0, 0, 255));
        assert_eq!(parse_color("#00ff00"), (0, 255, 0, 255));
        assert_eq!(parse_color("#0000ff"), (0, 0, 255, 255));
        assert_eq!(parse_color("ffffff"), (255, 255, 255, 255));
    }

    #[test]
    fn test_parse_color_rgba() {
        assert_eq!(parse_color("#ff000080"), (255, 0, 0, 128));
        assert_eq!(parse_color("#000000b4"), (0, 0, 0, 180));
    }

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.behavior.auto_select);
        assert_eq!(config.hints.font_size, 14);
    }
}
