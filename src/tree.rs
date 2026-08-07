//! Workspace file tree with collapsible directories.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub depth: usize,
}

pub struct Tree {
    pub root: PathBuf,
    pub entries: Vec<Entry>,
    pub expanded: HashSet<PathBuf>,
    pub selected: usize,
}

const IGNORED: &[&str] = &[".git", "target", "node_modules", ".DS_Store"];

impl Tree {
    pub fn open(root: &Path) -> std::io::Result<Self> {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let mut entries = Vec::new();
        scan(&root, 0, &mut entries)?;
        let expanded = entries
            .iter()
            .filter(|e| e.is_dir && e.depth == 0)
            .map(|e| e.path.clone())
            .collect();
        Ok(Self { root, entries, expanded, selected: 0 })
    }

    pub fn name(&self) -> String {
        self.root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.root.display().to_string())
    }

    pub fn toggle(&mut self, path: &Path) {
        if !self.expanded.remove(path) {
            self.expanded.insert(path.to_path_buf());
        }
    }

    /// Rescan the workspace from disk, keeping the expand/collapse state.
    pub fn reload(&mut self) {
        let mut entries = Vec::new();
        let _ = scan(&self.root, 0, &mut entries);
        self.entries = entries;
    }

    /// Index of `path` within the currently-visible list, if present.
    pub fn visible_index(&self, path: &Path) -> Option<usize> {
        self.visible().iter().position(|e| e.path == path)
    }

    /// Visible entries given expand/collapse state (depth-first order).
    pub fn visible(&self) -> Vec<&Entry> {
        let mut out = Vec::new();
        let mut hidden_at: Option<usize> = None;
        for e in &self.entries {
            if let Some(d) = hidden_at {
                if e.depth > d {
                    continue;
                }
                hidden_at = None;
            }
            out.push(e);
            if e.is_dir && !self.expanded.contains(&e.path) {
                hidden_at = Some(e.depth);
            }
        }
        out
    }

    /// All files (for quick-open / palette), as (name, path).
    pub fn files(&self) -> Vec<(&str, &Path)> {
        self.entries
            .iter()
            .filter(|e| !e.is_dir)
            .map(|e| (e.name.as_str(), e.path.as_path()))
            .collect()
    }
}

fn scan(dir: &Path, depth: usize, out: &mut Vec<Entry>) -> std::io::Result<()> {
    if depth > 12 {
        return Ok(());
    }
    let mut children: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter(|e| !IGNORED.contains(&e.file_name().to_string_lossy().as_ref()))
        .collect();
    children.sort_by_key(|e| {
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        (!is_dir, e.file_name().to_string_lossy().to_lowercase())
    });
    for child in children {
        let path = child.path();
        let is_dir = child.file_type().map(|t| t.is_dir()).unwrap_or(false);
        out.push(Entry {
            name: child.file_name().to_string_lossy().to_string(),
            path: path.clone(),
            is_dir,
            depth,
        });
        if is_dir {
            let _ = scan(&path, depth + 1, out);
        }
    }
    Ok(())
}
