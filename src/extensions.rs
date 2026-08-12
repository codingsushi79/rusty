//! Extensions — external tools declared as small TOML manifests.
//!
//! Each `*.toml` in `~/.config/rusty/extensions/` or `<project>/.rusty/extensions/`
//! defines a command that appears in the palette as `Ext: <name>`. When run, the
//! command executes with `RUSTY_FILE`, `RUSTY_LINE`, `RUSTY_COL`, and
//! `RUSTY_ROOT` in the environment, either in the background or in the terminal
//! pane. (A richer scripting/WASM API is future work; this covers the common
//! "run a tool on the current file" case.)

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Clone)]
pub struct Extension {
    pub name: String,
    /// Shell command to run (used when `wasm` is empty).
    #[serde(default)]
    pub command: String,
    /// Path to a `.wasm` plugin, relative to the manifest (takes priority).
    #[serde(default)]
    pub wasm: String,
    /// Run the command in the interactive terminal pane instead of background.
    #[serde(default)]
    pub terminal: bool,
    /// Directory of the manifest (set at discovery; not from TOML).
    #[serde(skip)]
    pub dir: PathBuf,
}

fn manifest_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(c) = dirs::config_dir() {
        dirs.push(c.join("rusty").join("extensions"));
    }
    dirs.push(root.join(".rusty").join("extensions"));
    dirs
}

pub fn discover(root: &Path) -> Vec<Extension> {
    let mut exts = Vec::new();
    for dir in manifest_dirs(root) {
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        for entry in read.flatten() {
            let p = entry.path();
            if p.extension().map_or(false, |e| e == "toml") {
                if let Ok(text) = std::fs::read_to_string(&p) {
                    if let Ok(mut ext) = toml::from_str::<Extension>(&text) {
                        ext.dir = p.parent().map(Path::to_path_buf).unwrap_or_default();
                        exts.push(ext);
                    }
                }
            }
        }
    }
    exts.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    exts
}
