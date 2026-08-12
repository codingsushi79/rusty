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
    /// Enable Vim-style modal editing (Normal/Insert modes + motions).
    pub vim_mode: bool,
    /// syntect theme name for syntax highlighting.
    pub syntax_theme: String,
    /// Opt-in local AI. Off by default; no network unless enabled.
    pub ai_enabled: bool,
    pub ai_endpoint: String,
    pub ai_model: String,
    /// Bring-your-own-token for cloud providers (empty for local servers).
    pub ai_api_key: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            tab_size: 4,
            line_numbers: true,
            vim_mode: false,
            syntax_theme: "base16-ocean.dark".to_string(),
            ai_enabled: false,
            ai_endpoint: "http://localhost:11434/v1".to_string(),
            ai_model: "llama3.2".to_string(),
            ai_api_key: String::new(),
        }
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
