//! User settings, persisted to `~/.config/rusty/config.toml`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Spaces inserted when pressing Tab.
    pub tab_size: usize,
    /// Show the line-number gutter.
    pub line_numbers: bool,
    /// syntect theme name for syntax highlighting.
    pub syntax_theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self { tab_size: 4, line_numbers: true, syntax_theme: "base16-ocean.dark".to_string() }
    }
}

impl Settings {
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|c| c.join("rusty").join("config.toml"))
    }

    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = toml::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }
}
