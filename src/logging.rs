//! Lightweight file logging. In a full-screen TUI there's no visible stderr,
//! so errors are appended to a log file the user can open (`Rusty: Open Log`).

use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Path to the log file (`~/.cache/rusty/rusty.log` or the platform equivalent).
pub fn path() -> Option<PathBuf> {
    let base = dirs::cache_dir().or_else(dirs::config_dir)?;
    Some(base.join("rusty").join("rusty.log"))
}

fn stamp() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let t = secs % 86_400;
    format!("{:02}:{:02}:{:02}", t / 3600, (t % 3600) / 60, t % 60)
}

fn write(level: &str, msg: &str) {
    let Some(path) = path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{} [{level}] {msg}", stamp());
    }
}

pub fn error(msg: &str) {
    write("ERROR", msg);
}

pub fn info(msg: &str) {
    write("INFO", msg);
}
