//! An editable text buffer: lines, a cursor, selection, undo/redo, viewport
//! scrolling, and a cached syntax-highlight of its contents.

use crate::highlight::{HlLine, Highlighter};
use similar::{ChangeTag, TextDiff};
use std::path::{Path, PathBuf};

/// Per-line VCS change marker for the diff gutter.
#[derive(Clone, Copy, PartialEq)]
pub enum GitMark {
    Added,
    Modified,
    Deleted,
}

#[derive(PartialEq, Clone, Copy)]
enum Edit {
    None,
    Insert,
    Delete,
    Other,
}

struct Snapshot {
    lines: Vec<String>,
    row: usize,
    col: usize,
}

pub struct Buffer {
    pub path: Option<PathBuf>,
    pub lines: Vec<String>,
    pub row: usize,
    pub col: usize,
    desired: usize,
    pub top: usize,
    pub left: usize,
    pub modified: bool,
    /// Selection anchor (the fixed end); the cursor is the moving end.
    pub anchor: Option<(usize, usize)>,
    pub hl: Vec<HlLine>,
    hl_dirty: bool,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_edit: Edit,
    head_text: Option<String>,
    pub marks: Vec<Option<GitMark>>,
    marks_dirty: bool,
    /// Monotonic edit counter, used as the LSP document version.
    pub edits: u64,
}

const MAX_UNDO: usize = 500;

impl Buffer {
    pub fn from_file(path: &Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let mut lines: Vec<String> = text.split('\n').map(String::from).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        Ok(Self::from_lines(Some(path.to_path_buf()), lines))
    }

    fn from_lines(path: Option<PathBuf>, lines: Vec<String>) -> Self {
        Self {
            path,
            lines,
            row: 0,
            col: 0,
            desired: 0,
            top: 0,
            left: 0,
            modified: false,
            anchor: None,
            hl: Vec::new(),
            hl_dirty: true,
            undo: Vec::new(),
            redo: Vec::new(),
            last_edit: Edit::None,
            head_text: None,
            marks: Vec::new(),
            marks_dirty: true,
            edits: 0,
        }
    }

    /// Provide the HEAD version of this file so the diff gutter can be computed.
    pub fn set_head(&mut self, head: Option<String>) {
        self.head_text = head;
        self.marks_dirty = true;
    }

    /// Recompute per-line diff markers against HEAD (cheap; on change only).
    pub fn compute_marks(&mut self) {
        if !self.marks_dirty {
            return;
        }
        self.marks_dirty = false;
        let mut marks = vec![None; self.lines.len()];
        if let Some(head) = &self.head_text {
            let new = self.text();
            let diff = TextDiff::from_lines(head.as_str(), new.as_str());
            let mut deletes = 0usize; // pending deleted lines not yet matched by an insert
            let mut last_new = 0usize;
            for change in diff.iter_all_changes() {
                match change.tag() {
                    ChangeTag::Equal => {
                        if deletes > 0 {
                            if let Some(i) = change.new_index() {
                                if i < marks.len() {
                                    marks[i] = Some(GitMark::Deleted);
                                }
                            }
                            deletes = 0;
                        }
                        if let Some(i) = change.new_index() {
                            last_new = i;
                        }
                    }
                    ChangeTag::Delete => deletes += 1,
                    ChangeTag::Insert => {
                        if let Some(i) = change.new_index() {
                            if i < marks.len() {
                                marks[i] = Some(if deletes > 0 {
                                    deletes -= 1;
                                    GitMark::Modified
                                } else {
                                    GitMark::Added
                                });
                            }
                            last_new = i;
                        }
                    }
                }
            }
            // A trailing deletion at end of file: flag the last line.
            if deletes > 0 && !marks.is_empty() {
                let i = last_new.min(marks.len() - 1);
                if marks[i].is_none() {
                    marks[i] = Some(GitMark::Deleted);
                }
            }
        }
        self.marks = marks;
    }

    pub fn name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "untitled".to_string())
    }

    pub fn extension(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.extension())
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    pub fn language(&self) -> &'static str {
        match self.extension().as_str() {
            "rs" => "Rust",
            "py" => "Python",
            "js" | "mjs" | "cjs" => "JavaScript",
            "ts" | "tsx" | "jsx" => "TypeScript",
            "json" => "JSON",
            "toml" => "TOML",
            "md" => "Markdown",
            "html" => "HTML",
            "css" => "CSS",
            "go" => "Go",
            "c" | "h" => "C",
            "sh" | "bash" => "Shell",
            "" => "Plain Text",
            other => Box::leak(other.to_uppercase().into_boxed_str()),
        }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    fn line_chars(&self, row: usize) -> usize {
        self.lines.get(row).map(|l| l.chars().count()).unwrap_or(0)
    }

    fn byte_of(line: &str, col: usize) -> usize {
        line.char_indices().nth(col).map(|(i, _)| i).unwrap_or(line.len())
    }

    fn touched(&mut self) {
        self.modified = true;
        self.hl_dirty = true;
        self.marks_dirty = true;
        self.edits += 1;
    }

    // --- undo/redo ---------------------------------------------------------

    fn snapshot(&self) -> Snapshot {
        Snapshot { lines: self.lines.clone(), row: self.row, col: self.col }
    }

    /// Record a pre-edit snapshot, coalescing consecutive edits of the same kind.
    fn begin(&mut self, kind: Edit) {
        if self.last_edit != kind || kind == Edit::Other {
            self.undo.push(self.snapshot());
            if self.undo.len() > MAX_UNDO {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        self.last_edit = kind;
    }

    fn apply(&mut self, s: Snapshot) {
        self.lines = s.lines;
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.row = s.row.min(self.lines.len() - 1);
        self.col = s.col.min(self.line_chars(self.row));
        self.desired = self.col;
        self.anchor = None;
        self.touched();
    }

    pub fn undo(&mut self) {
        if let Some(s) = self.undo.pop() {
            let cur = self.snapshot();
            self.redo.push(cur);
            self.apply(s);
            self.last_edit = Edit::None;
        }
    }

    pub fn redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            let cur = self.snapshot();
            self.undo.push(cur);
            self.apply(s);
            self.last_edit = Edit::None;
        }
    }

    // --- selection ---------------------------------------------------------

    /// If `extend`, start/keep a selection anchor; otherwise clear it.
    pub fn begin_move(&mut self, extend: bool) {
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some((self.row, self.col));
            }
        } else {
            self.anchor = None;
        }
    }

    pub fn selection(&self) -> Option<((usize, usize), (usize, usize))> {
        let a = self.anchor?;
        let b = (self.row, self.col);
        if a == b {
            return None;
        }
        Some(if a <= b { (a, b) } else { (b, a) })
    }

    pub fn selected_text(&self) -> Option<String> {
        let ((sr, sc), (er, ec)) = self.selection()?;
        if sr == er {
            Some(self.lines[sr].chars().skip(sc).take(ec - sc).collect())
        } else {
            let mut out: String = self.lines[sr].chars().skip(sc).collect();
            for r in (sr + 1)..er {
                out.push('\n');
                out.push_str(&self.lines[r]);
            }
            out.push('\n');
            let tail: String = self.lines[er].chars().take(ec).collect();
            out.push_str(&tail);
            Some(out)
        }
    }

    fn delete_sel_raw(&mut self) {
        let Some(((sr, sc), (er, ec))) = self.selection() else { return };
        if sr == er {
            let a = Self::byte_of(&self.lines[sr], sc);
            let b = Self::byte_of(&self.lines[sr], ec);
            self.lines[sr].replace_range(a..b, "");
        } else {
            let head: String = self.lines[sr].chars().take(sc).collect();
            let tail: String = self.lines[er].chars().skip(ec).collect();
            self.lines.drain(sr..=er);
            self.lines.insert(sr, format!("{head}{tail}"));
        }
        self.row = sr;
        self.col = sc;
        self.desired = sc;
        self.anchor = None;
        self.touched();
    }

    pub fn delete_selection(&mut self) {
        if self.selection().is_some() {
            self.begin(Edit::Other);
            self.delete_sel_raw();
        }
    }

    // --- raw edits (no undo bookkeeping) -----------------------------------

    fn raw_insert_char(&mut self, c: char) {
        let b = Self::byte_of(&self.lines[self.row], self.col);
        self.lines[self.row].insert(b, c);
        self.col += 1;
        self.desired = self.col;
        self.touched();
    }

    fn raw_newline(&mut self) {
        let b = Self::byte_of(&self.lines[self.row], self.col);
        let rest = self.lines[self.row].split_off(b);
        self.lines.insert(self.row + 1, rest);
        self.row += 1;
        self.col = 0;
        self.desired = 0;
        self.touched();
    }

    // --- public edits ------------------------------------------------------

    pub fn insert_char(&mut self, c: char) {
        let had_sel = self.selection().is_some();
        self.begin(if had_sel { Edit::Other } else { Edit::Insert });
        if had_sel {
            self.delete_sel_raw();
        }
        self.raw_insert_char(c);
    }

    pub fn insert_newline(&mut self) {
        self.begin(Edit::Other);
        if self.selection().is_some() {
            self.delete_sel_raw();
        }
        self.raw_newline();
    }

    /// Accept a completion: replace the identifier prefix before the cursor
    /// with `insert`.
    pub fn complete(&mut self, insert: &str) {
        self.begin(Edit::Other);
        let chars: Vec<char> = self.lines[self.row].chars().collect();
        let mut n = 0;
        let mut i = self.col;
        while i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
            i -= 1;
            n += 1;
        }
        for _ in 0..n {
            let b = Self::byte_of(&self.lines[self.row], self.col - 1);
            self.lines[self.row].remove(b);
            self.col -= 1;
        }
        for ch in insert.chars() {
            if ch == '\n' {
                self.raw_newline();
            } else if ch != '\r' {
                self.raw_insert_char(ch);
            }
        }
        self.anchor = None;
        self.desired = self.col;
        self.touched();
    }

    pub fn insert_str(&mut self, s: &str) {
        self.begin(Edit::Other);
        if self.selection().is_some() {
            self.delete_sel_raw();
        }
        for ch in s.chars() {
            if ch == '\n' {
                self.raw_newline();
            } else if ch != '\r' {
                self.raw_insert_char(ch);
            }
        }
    }

    pub fn backspace(&mut self) {
        if self.selection().is_some() {
            self.begin(Edit::Other);
            self.delete_sel_raw();
            return;
        }
        self.begin(Edit::Delete);
        if self.col > 0 {
            let b = Self::byte_of(&self.lines[self.row], self.col - 1);
            self.lines[self.row].remove(b);
            self.col -= 1;
        } else if self.row > 0 {
            let cur = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.line_chars(self.row);
            self.lines[self.row].push_str(&cur);
        } else {
            return;
        }
        self.desired = self.col;
        self.touched();
    }

    pub fn delete(&mut self) {
        if self.selection().is_some() {
            self.begin(Edit::Other);
            self.delete_sel_raw();
            return;
        }
        self.begin(Edit::Delete);
        if self.col < self.line_chars(self.row) {
            let b = Self::byte_of(&self.lines[self.row], self.col);
            self.lines[self.row].remove(b);
            self.touched();
        } else if self.row + 1 < self.lines.len() {
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&next);
            self.touched();
        }
    }

    // --- movement ----------------------------------------------------------

    fn break_run(&mut self) {
        self.last_edit = Edit::None;
    }

    pub fn left(&mut self) {
        self.break_run();
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.line_chars(self.row);
        }
        self.desired = self.col;
    }

    pub fn right(&mut self) {
        self.break_run();
        if self.col < self.line_chars(self.row) {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
        self.desired = self.col;
    }

    pub fn up(&mut self) {
        self.break_run();
        if self.row > 0 {
            self.row -= 1;
            self.col = self.desired.min(self.line_chars(self.row));
        }
    }

    pub fn down(&mut self) {
        self.break_run();
        if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = self.desired.min(self.line_chars(self.row));
        }
    }

    pub fn home(&mut self) {
        self.break_run();
        self.col = 0;
        self.desired = 0;
    }

    pub fn end(&mut self) {
        self.break_run();
        self.col = self.line_chars(self.row);
        self.desired = self.col;
    }

    pub fn page(&mut self, delta: isize, height: usize) {
        self.break_run();
        let target = (self.row as isize + delta * height as isize)
            .clamp(0, self.lines.len() as isize - 1) as usize;
        self.row = target;
        self.col = self.desired.min(self.line_chars(self.row));
    }

    pub fn set_cursor(&mut self, row: usize, col: usize) {
        self.break_run();
        self.anchor = None;
        self.row = row.min(self.lines.len().saturating_sub(1));
        self.col = col.min(self.line_chars(self.row));
        self.desired = self.col;
    }

    /// Place the cursor (mouse click/drag). `extend` keeps/starts a selection.
    pub fn place_cursor(&mut self, row: usize, col: usize, extend: bool) {
        self.break_run();
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some((self.row, self.col));
            }
        } else {
            self.anchor = None;
        }
        self.row = row.min(self.lines.len().saturating_sub(1));
        self.col = col.min(self.line_chars(self.row));
        self.desired = self.col;
    }

    pub fn scroll_into_view(&mut self, height: usize, width: usize) {
        if self.row < self.top {
            self.top = self.row;
        } else if height > 0 && self.row >= self.top + height {
            self.top = self.row + 1 - height;
        }
        if self.col < self.left {
            self.left = self.col;
        } else if width > 0 && self.col >= self.left + width {
            self.left = self.col + 1 - width;
        }
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        if let Some(path) = &self.path {
            std::fs::write(path, self.text())?;
            self.modified = false;
        }
        Ok(())
    }

    /// Force the next `rehighlight` to recompute (e.g. after a theme change).
    pub fn force_rehighlight(&mut self) {
        self.hl_dirty = true;
    }

    pub fn rehighlight(&mut self, hl: &Highlighter) {
        if !self.hl_dirty && !self.hl.is_empty() {
            return;
        }
        let ext = self.extension();
        let mut lines = hl.highlight(&self.text(), &ext);
        lines.resize_with(self.lines.len(), Vec::new);
        self.hl = lines;
        self.hl_dirty = false;
    }
}
