//! Application state and input handling.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::widgets::ListState;

use crate::buffer::Buffer;
use crate::config::Settings;
use crate::git;
use crate::highlight::Highlighter;
use crate::lsp::{self, Lsp};
use crate::shell::Shell;
use crate::tree::{Entry, Tree};

/// The GitHub repo this build installs/updates from.
pub const REPO_URL: &str = "https://github.com/codingsushi79/rusty";
/// Number of rows in the settings screen (for selection wrap).
pub const SETTINGS_COUNT: usize = 3;

/// A diagnostic (error/warning) from the language server.
pub struct Diagnostic {
    pub line: usize,
    pub severity: u8, // 1=error, 2=warning, 3=info, 4=hint
    pub message: String,
}

/// An active completion popup.
pub struct Completion {
    pub items: Vec<(String, String)>, // (label, insert text)
    pub sel: usize,
}

fn uri_of(path: &Path) -> String {
    format!("file://{}", path.display())
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Focus {
    Editor,
    Tree,
    Palette,
    Find,
    Prompt,
    Search,
    Shell,
    Settings,
}

/// One workspace-search hit: (file, 0-indexed line, trimmed line text).
pub type Hit = (PathBuf, usize, String);

/// Screen rectangles from the last frame, used to map mouse coordinates.
#[derive(Default, Clone)]
pub struct LayoutInfo {
    pub tree: Option<Rect>,
    pub tabs: Rect,
    pub text: Rect,
    pub gutter: u16,
    pub tab_spans: Vec<(u16, u16, usize)>,
}

fn hit(r: Rect, col: u16, row: u16) -> bool {
    col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
}

#[derive(Clone)]
pub enum Action {
    Open(PathBuf),
    Save,
    Close,
    NewFile,
    ToggleTree,
    StageFile,
    StageAll,
    CommitPrompt,
    GitInit,
    AddRemotePrompt,
    Push,
    Pull,
    Fetch,
    Settings,
    Update,
    Quit,
}

pub enum PromptKind {
    NewFile(PathBuf),
    Commit,
    Delete(PathBuf),
    AddRemote,
}

/// A one-line text prompt (create a file, or a commit message).
pub struct Prompt {
    pub label: String,
    pub input: String,
    pub kind: PromptKind,
}

pub struct App {
    pub tree: Tree,
    pub tree_state: ListState,
    pub buffers: Vec<Buffer>,
    pub active: usize,
    pub focus: Focus,
    pub show_tree: bool,
    pub hl: Highlighter,
    pub branch: Option<String>,
    pub status: String,
    pub quit: bool,

    pub palette_query: String,
    pub palette_sel: usize,
    pub find_query: String,
    pub prompt: Option<Prompt>,
    pub search_query: String,
    pub search_replace: String,
    pub search_in_replace: bool,
    pub search_results: Vec<Hit>,
    pub search_sel: usize,

    pub shell: Option<Shell>,
    shell_rx: Option<Receiver<String>>,
    pub shell_output: String,
    pub shell_input: String,
    pub shell_open: bool,

    pub settings: Settings,
    pub settings_sel: usize,

    clipboard: String,
    sys_clip: Option<arboard::Clipboard>,

    lsp: Option<Lsp>,
    lsp_started: bool,
    lsp_sent: HashMap<String, u64>,
    pub diagnostics: HashMap<String, Vec<Diagnostic>>,
    pub completion: Option<Completion>,
    pending_completion: Option<i64>,

    pub layout: LayoutInfo,
    pub editor_height: usize,
    pub editor_width: usize,
}

impl App {
    pub fn new(root: &Path) -> anyhow::Result<Self> {
        let tree = Tree::open(root)?;
        let branch = git::branch(root);
        let mut tree_state = ListState::default();
        tree_state.select(Some(0));
        let settings = Settings::load();
        let mut hl = Highlighter::new();
        hl.set_theme(&settings.syntax_theme);
        Ok(Self {
            tree,
            tree_state,
            buffers: Vec::new(),
            active: 0,
            focus: Focus::Editor,
            show_tree: true,
            hl,
            branch,
            status: "^P open · ^N new file · ^B files · ^Q quit".to_string(),
            quit: false,
            palette_query: String::new(),
            palette_sel: 0,
            find_query: String::new(),
            prompt: None,
            search_query: String::new(),
            search_replace: String::new(),
            search_in_replace: false,
            search_results: Vec::new(),
            search_sel: 0,
            shell: None,
            shell_rx: None,
            shell_output: String::new(),
            shell_input: String::new(),
            shell_open: false,
            settings,
            settings_sel: 0,
            clipboard: String::new(),
            sys_clip: arboard::Clipboard::new().ok(),
            lsp: None,
            lsp_started: false,
            lsp_sent: HashMap::new(),
            diagnostics: HashMap::new(),
            completion: None,
            pending_completion: None,
            layout: LayoutInfo::default(),
            editor_height: 20,
            editor_width: 80,
        })
    }

    pub fn buf(&self) -> Option<&Buffer> {
        self.buffers.get(self.active)
    }

    pub fn prepare_active(&mut self, height: usize, width: usize) {
        let hl = &self.hl;
        if let Some(b) = self.buffers.get_mut(self.active) {
            b.scroll_into_view(height, width);
            b.rehighlight(hl);
            b.compute_marks();
        }
    }

    pub fn open_path(&mut self, path: &Path) {
        if let Some(i) = self.buffers.iter().position(|b| b.path.as_deref() == Some(path)) {
            self.active = i;
        } else {
            match Buffer::from_file(path) {
                Ok(mut b) => {
                    b.set_head(git::head_text(path));
                    self.buffers.push(b);
                    self.active = self.buffers.len() - 1;
                    self.status = format!("Opened {}", path.display());
                    self.lsp_open(path);
                }
                Err(e) => self.status = format!("Open failed: {e}"),
            }
        }
        self.focus = Focus::Editor;
    }

    /// Start a language server (once) for this file's language and open it.
    fn lsp_open(&mut self, path: &Path) {
        let ext = path.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
        let Some((cmd, lang)) = lsp::server_for(&ext) else { return };
        if self.lsp.is_none() && !self.lsp_started {
            self.lsp_started = true;
            self.lsp = Lsp::spawn(cmd, &self.tree.root);
            if self.lsp.is_none() {
                self.status = format!("{cmd} not found — language features disabled");
            }
        }
        let uri = uri_of(path);
        let (text, edits) = match self.buffers.iter().find(|b| b.path.as_deref() == Some(path)) {
            Some(b) => (b.text(), b.edits),
            None => return,
        };
        if let Some(l) = &mut self.lsp {
            l.did_open(&uri, lang, &text);
            self.lsp_sent.insert(uri, edits);
        }
    }

    /// Push buffer changes to the server (called each tick).
    pub fn lsp_sync(&mut self) {
        let Some(b) = self.buffers.get(self.active) else { return };
        let Some(path) = b.path.clone() else { return };
        let uri = uri_of(&path);
        let edits = b.edits;
        if self.lsp_sent.get(&uri) == Some(&edits) {
            return;
        }
        let text = b.text();
        if let Some(l) = &mut self.lsp {
            l.did_change(&uri, edits as i64 + 1, &text);
            self.lsp_sent.insert(uri, edits);
        }
    }

    /// Drain server messages (diagnostics, completion responses). Returns true
    /// if anything changed that warrants a redraw.
    pub fn lsp_poll(&mut self) -> bool {
        let Some(l) = &self.lsp else { return false };
        let msgs = l.drain();
        if msgs.is_empty() {
            return false;
        }
        for v in msgs {
            if v.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics") {
                let p = &v["params"];
                let uri = p["uri"].as_str().unwrap_or("").to_string();
                let mut ds = Vec::new();
                if let Some(arr) = p["diagnostics"].as_array() {
                    for d in arr {
                        ds.push(Diagnostic {
                            line: d["range"]["start"]["line"].as_u64().unwrap_or(0) as usize,
                            severity: d["severity"].as_u64().unwrap_or(1) as u8,
                            message: d["message"].as_str().unwrap_or("").lines().next().unwrap_or("").to_string(),
                        });
                    }
                }
                self.diagnostics.insert(uri, ds);
            } else if let Some(id) = v.get("id").and_then(|i| i.as_i64()) {
                if Some(id) == self.pending_completion {
                    self.pending_completion = None;
                    let result = &v["result"];
                    let arr = if result.is_array() { result.clone() } else { result["items"].clone() };
                    let mut items = Vec::new();
                    if let Some(list) = arr.as_array() {
                        for it in list.iter().take(80) {
                            let label = it["label"].as_str().unwrap_or("").to_string();
                            let insert = it["insertText"]
                                .as_str()
                                .or_else(|| it["textEdit"]["newText"].as_str())
                                .unwrap_or(&label)
                                .to_string();
                            if !label.is_empty() {
                                items.push((label, insert));
                            }
                        }
                    }
                    if items.is_empty() {
                        self.status = "No completions".to_string();
                    } else {
                        self.completion = Some(Completion { items, sel: 0 });
                    }
                }
            }
        }
        true
    }

    fn trigger_completion(&mut self) {
        let Some(b) = self.buffers.get(self.active) else { return };
        let Some(path) = b.path.clone() else { return };
        let (line, col) = (b.row, b.col);
        let uri = uri_of(&path);
        if let Some(l) = &mut self.lsp {
            let id = l.completion(&uri, line, col);
            self.pending_completion = Some(id);
            self.status = "Completing…".to_string();
        }
    }

    fn accept_completion(&mut self) {
        if let Some(c) = self.completion.take() {
            if let Some((_, insert)) = c.items.get(c.sel).cloned() {
                if let Some(b) = self.buffers.get_mut(self.active) {
                    b.complete(&insert);
                }
            }
        }
    }

    /// Refresh branch + the active buffer's HEAD text (after stage/commit).
    fn refresh_head(&mut self) {
        self.branch = git::branch(&self.tree.root);
        if let Some(p) = self.buffers.get(self.active).and_then(|b| b.path.clone()) {
            let h = git::head_text(&p);
            if let Some(b) = self.buffers.get_mut(self.active) {
                b.set_head(h);
            }
        }
    }

    fn save(&mut self) {
        let Some(buf) = self.buffers.get_mut(self.active) else { return };
        if buf.path.is_none() {
            self.status = "No filename".to_string();
            return;
        }
        match buf.save() {
            Ok(()) => self.status = format!("Saved {}", buf.name()),
            Err(e) => self.status = format!("Save failed: {e}"),
        }
    }

    fn close_active(&mut self) {
        if self.buffers.is_empty() {
            return;
        }
        self.buffers.remove(self.active);
        if self.buffers.is_empty() {
            self.active = 0;
            self.focus = if self.show_tree { Focus::Tree } else { Focus::Editor };
        } else {
            self.active = self.active.min(self.buffers.len() - 1);
        }
    }

    fn set_clip(&mut self, s: String) {
        if let Some(c) = &mut self.sys_clip {
            let _ = c.set_text(s.clone());
        }
        self.clipboard = s;
    }

    fn get_clip(&mut self) -> String {
        if let Some(c) = &mut self.sys_clip {
            if let Ok(t) = c.get_text() {
                return t;
            }
        }
        self.clipboard.clone()
    }

    fn new_file_prompt(&mut self, dir: PathBuf) {
        let rel = dir.strip_prefix(&self.tree.root).unwrap_or(&dir);
        let label = if rel.as_os_str().is_empty() {
            "New file:".to_string()
        } else {
            format!("New file in {}/:", rel.display())
        };
        self.prompt = Some(Prompt { label, input: String::new(), kind: PromptKind::NewFile(dir) });
        self.focus = Focus::Prompt;
    }

    fn delete_prompt(&mut self, path: PathBuf, is_dir: bool, name: String) {
        let kind = if is_dir { "folder" } else { "file" };
        self.prompt = Some(Prompt {
            label: format!("Delete {kind} '{name}'?"),
            input: String::new(),
            kind: PromptKind::Delete(path),
        });
        self.focus = Focus::Prompt;
    }

    fn confirm_prompt(&mut self) {
        let Some(p) = self.prompt.take() else { return };
        let input = p.input.trim().to_string();
        match p.kind {
            PromptKind::NewFile(dir) => {
                self.focus = Focus::Tree;
                if input.is_empty() {
                    return;
                }
                let path = dir.join(&input);
                if !path.exists() {
                    if let Err(e) = std::fs::write(&path, "") {
                        self.status = format!("Create failed: {e}");
                        return;
                    }
                }
                self.tree.reload();
                self.tree.expanded.insert(dir);
                self.open_path(&path);
                if let Some(i) = self.tree.visible_index(&path) {
                    self.tree.selected = i;
                }
                self.status = format!("Created {}", path.display());
            }
            PromptKind::Commit => {
                self.focus = Focus::Editor;
                if input.is_empty() {
                    self.status = "Commit cancelled (empty message)".to_string();
                    return;
                }
                match git::commit(&self.tree.root, &input) {
                    Ok(oid) => self.status = format!("Committed {}", &oid.to_string()[..7]),
                    Err(e) => self.status = format!("Commit failed: {e}"),
                }
                self.refresh_head();
            }
            PromptKind::AddRemote => {
                self.focus = Focus::Editor;
                if input.is_empty() {
                    self.status = "Add remote cancelled (empty URL)".to_string();
                    return;
                }
                match git::add_remote(&self.tree.root, "origin", &input) {
                    Ok(()) => self.status = format!("Set remote 'origin' → {input}"),
                    Err(e) => self.status = format!("Add remote failed: {e}"),
                }
            }
            PromptKind::Delete(path) => {
                self.focus = Focus::Tree;
                let res = if path.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };
                match res {
                    Ok(()) => {
                        // Close any open buffers for the deleted path (or under it).
                        self.buffers.retain(|b| {
                            b.path.as_deref().map_or(true, |p| !p.starts_with(&path))
                        });
                        self.active = self.active.min(self.buffers.len().saturating_sub(1));
                        self.tree.reload();
                        let vis = self.tree.visible().len();
                        self.tree.selected = self.tree.selected.min(vis.saturating_sub(1));
                        self.status = format!("Deleted {}", path.display());
                    }
                    Err(e) => self.status = format!("Delete failed: {e}"),
                }
            }
        }
    }

    // --- input -------------------------------------------------------------

    pub fn on_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl {
            match key.code {
                KeyCode::Char('q') => {
                    self.quit = true;
                    return;
                }
                KeyCode::Char('s') => {
                    self.save();
                    return;
                }
                KeyCode::Char('p') => {
                    self.focus = Focus::Palette;
                    self.palette_query.clear();
                    self.palette_sel = 0;
                    return;
                }
                KeyCode::Char('f') => {
                    if self.buf().is_some() {
                        self.focus = Focus::Find;
                        self.find_query.clear();
                    }
                    return;
                }
                KeyCode::Char('b') => {
                    if !self.show_tree {
                        self.show_tree = true;
                        self.focus = Focus::Tree;
                    } else if self.focus != Focus::Tree {
                        self.focus = Focus::Tree;
                    } else {
                        self.show_tree = false;
                        self.focus = Focus::Editor;
                    }
                    return;
                }
                KeyCode::Char('w') => {
                    self.close_active();
                    return;
                }
                KeyCode::Char('n') => {
                    self.new_file_prompt(self.tree.root.clone());
                    return;
                }
                KeyCode::Char('g') => {
                    self.focus = Focus::Search;
                    self.search_query.clear();
                    self.search_replace.clear();
                    self.search_in_replace = false;
                    self.search_results.clear();
                    self.search_sel = 0;
                    return;
                }
                KeyCode::Char('r') if self.focus == Focus::Search => {
                    self.replace_all();
                    return;
                }
                KeyCode::Char('j') => {
                    self.toggle_shell();
                    return;
                }
                KeyCode::Char(',') => {
                    self.focus = Focus::Settings;
                    self.settings_sel = 0;
                    return;
                }
                KeyCode::Char('z') => {
                    if let Some(b) = self.buffers.get_mut(self.active) {
                        b.undo();
                    }
                    return;
                }
                KeyCode::Char('y') => {
                    if let Some(b) = self.buffers.get_mut(self.active) {
                        b.redo();
                    }
                    return;
                }
                KeyCode::Char('c') => {
                    if let Some(t) = self.buffers.get(self.active).and_then(|b| b.selected_text()) {
                        self.set_clip(t);
                        self.status = "Copied".to_string();
                    }
                    return;
                }
                KeyCode::Char('x') => {
                    if let Some(t) = self.buffers.get(self.active).and_then(|b| b.selected_text()) {
                        self.set_clip(t);
                        if let Some(b) = self.buffers.get_mut(self.active) {
                            b.delete_selection();
                        }
                    }
                    return;
                }
                KeyCode::Char('v') => {
                    let t = self.get_clip();
                    if let Some(b) = self.buffers.get_mut(self.active) {
                        b.insert_str(&t);
                    }
                    return;
                }
                KeyCode::Char(' ') => {
                    self.trigger_completion();
                    return;
                }
                _ => {}
            }
        }
        match self.focus {
            Focus::Editor => self.editor_key(key),
            Focus::Tree => self.tree_key(key),
            Focus::Palette => self.palette_key(key),
            Focus::Find => self.find_key(key),
            Focus::Prompt => self.prompt_key(key),
            Focus::Search => self.search_key(key),
            Focus::Shell => self.shell_key(key),
            Focus::Settings => self.settings_key(key),
        }
    }

    fn settings_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.settings.save();
                self.focus = Focus::Editor;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.settings_sel = self.settings_sel.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.settings_sel = (self.settings_sel + 1).min(SETTINGS_COUNT - 1)
            }
            KeyCode::Left => self.adjust_setting(-1),
            KeyCode::Right | KeyCode::Enter | KeyCode::Char(' ') => self.adjust_setting(1),
            _ => {}
        }
    }

    fn adjust_setting(&mut self, dir: i32) {
        match self.settings_sel {
            0 => {
                let n = (self.settings.tab_size as i32 + dir).clamp(1, 8) as usize;
                self.settings.tab_size = n;
            }
            1 => self.settings.line_numbers = !self.settings.line_numbers,
            2 => {
                let names = self.hl.theme_names();
                if !names.is_empty() {
                    let cur = names.iter().position(|n| n == self.hl.theme_name()).unwrap_or(0);
                    let next = (cur as i32 + dir).rem_euclid(names.len() as i32) as usize;
                    self.apply_theme(&names[next].clone());
                }
            }
            _ => {}
        }
        self.settings.save();
    }

    fn apply_theme(&mut self, name: &str) {
        if self.hl.set_theme(name) {
            self.settings.syntax_theme = name.to_string();
            for b in &mut self.buffers {
                b.force_rehighlight();
            }
        }
    }

    /// Open/focus the terminal pane and run a command in it.
    fn run_in_shell(&mut self, cmd: &str) {
        if self.shell.is_none() {
            let (s, rx) = Shell::new(self.tree.root.clone());
            self.shell = Some(s);
            self.shell_rx = Some(rx);
        }
        self.shell_open = true;
        self.focus = Focus::Shell;
        if let Some(s) = &mut self.shell {
            s.run(cmd);
        }
    }

    fn toggle_shell(&mut self) {
        if !self.shell_open {
            self.shell_open = true;
            self.focus = Focus::Shell;
            if self.shell.is_none() {
                let (s, rx) = Shell::new(self.tree.root.clone());
                self.shell = Some(s);
                self.shell_rx = Some(rx);
            }
        } else if self.focus != Focus::Shell {
            self.focus = Focus::Shell;
        } else {
            self.shell_open = false;
            self.focus = Focus::Editor;
        }
    }

    fn shell_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.focus = Focus::Editor,
            KeyCode::Enter => {
                let cmd = std::mem::take(&mut self.shell_input);
                if cmd.trim() == "clear" {
                    self.shell_output.clear();
                } else if let Some(s) = &mut self.shell {
                    s.run(&cmd);
                }
            }
            KeyCode::Backspace => {
                self.shell_input.pop();
            }
            KeyCode::Char(c) if !is_control_combo(&key) => self.shell_input.push(c),
            _ => {}
        }
    }

    /// Drain shell output into the pane. Returns true if anything changed.
    pub fn shell_poll(&mut self) -> bool {
        let mut changed = false;
        if let Some(rx) = &self.shell_rx {
            for chunk in rx.try_iter() {
                self.shell_output.push_str(&chunk);
                changed = true;
            }
        }
        if changed && self.shell_output.len() > 100_000 {
            let cut = self.shell_output.len() - 80_000;
            self.shell_output = self.shell_output.split_off(cut);
        }
        changed
    }

    /// Replace every exact occurrence of the search query across matched files.
    fn replace_all(&mut self) {
        if self.search_query.is_empty() {
            return;
        }
        let q = self.search_query.clone();
        let r = self.search_replace.clone();
        let files: HashSet<PathBuf> = self.search_results.iter().map(|(p, _, _)| p.clone()).collect();
        let (mut nfiles, mut nrepl) = (0usize, 0usize);
        for path in &files {
            if let Ok(text) = std::fs::read_to_string(path) {
                let count = text.matches(&q).count();
                if count > 0 && std::fs::write(path, text.replace(&q, &r)).is_ok() {
                    nfiles += 1;
                    nrepl += count;
                }
            }
        }
        // Reload any open buffers for the changed files.
        for i in 0..self.buffers.len() {
            if let Some(p) = self.buffers[i].path.clone() {
                if files.contains(&p) {
                    if let Ok(mut nb) = Buffer::from_file(&p) {
                        nb.set_head(git::head_text(&p));
                        self.buffers[i] = nb;
                    }
                }
            }
        }
        self.recompute_search();
        self.status = format!("Replaced {nrepl} occurrence(s) in {nfiles} file(s)");
    }

    fn search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.focus = Focus::Editor,
            KeyCode::Tab => self.search_in_replace = !self.search_in_replace,
            KeyCode::Up => self.search_sel = self.search_sel.saturating_sub(1),
            KeyCode::Down => {
                self.search_sel = (self.search_sel + 1).min(self.search_results.len().saturating_sub(1))
            }
            KeyCode::Enter => {
                if let Some((path, line, _)) = self.search_results.get(self.search_sel).cloned() {
                    self.open_path(&path);
                    if let Some(b) = self.buffers.get_mut(self.active) {
                        b.set_cursor(line, 0);
                    }
                }
            }
            KeyCode::Backspace => {
                if self.search_in_replace {
                    self.search_replace.pop();
                } else {
                    self.search_query.pop();
                    self.recompute_search();
                }
            }
            KeyCode::Char(c) if !is_control_combo(&key) => {
                if self.search_in_replace {
                    self.search_replace.push(c);
                } else {
                    self.search_query.push(c);
                    self.recompute_search();
                }
            }
            _ => {}
        }
    }

    fn recompute_search(&mut self) {
        self.search_results.clear();
        self.search_sel = 0;
        let q = self.search_query.trim().to_lowercase();
        if q.len() < 2 {
            return;
        }
        'files: for (_, path) in self.tree.files() {
            let Ok(text) = std::fs::read_to_string(path) else { continue };
            for (i, line) in text.lines().enumerate() {
                if line.to_lowercase().contains(&q) {
                    let trimmed = line.trim_start();
                    self.search_results.push((path.to_path_buf(), i, trimmed.chars().take(120).collect()));
                    if self.search_results.len() >= 300 {
                        break 'files;
                    }
                }
            }
        }
    }

    pub fn on_mouse(&mut self, m: MouseEvent) {
        let (col, row) = (m.column, m.row);
        match m.kind {
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                // File tree.
                if let Some(tr) = self.layout.tree {
                    if self.show_tree && hit(tr, col, row) {
                        let idx = self.tree_state.offset() + (row - tr.y) as usize;
                        let vis: Vec<Entry> = self.tree.visible().into_iter().cloned().collect();
                        if idx < vis.len() {
                            self.focus = Focus::Tree;
                            self.tree.selected = idx;
                            let e = &vis[idx];
                            if e.is_dir {
                                let p = e.path.clone();
                                self.tree.toggle(&p);
                            } else {
                                let p = e.path.clone();
                                self.open_path(&p);
                            }
                        }
                        return;
                    }
                }
                // Tabs.
                if hit(self.layout.tabs, col, row) {
                    for (a, b, i) in &self.layout.tab_spans {
                        if col >= *a && col < *b {
                            self.active = *i;
                            self.focus = Focus::Editor;
                            return;
                        }
                    }
                    return;
                }
                // Editor text.
                if hit(self.layout.text, col, row) {
                    self.focus = Focus::Editor;
                    self.place_from_mouse(col, row, false);
                }
            }
            MouseEventKind::Drag(crossterm::event::MouseButton::Left) => {
                if hit(self.layout.text, col, row) {
                    self.place_from_mouse(col, row, true);
                }
            }
            MouseEventKind::ScrollDown => self.scroll_mouse(col, row, 1),
            MouseEventKind::ScrollUp => self.scroll_mouse(col, row, -1),
            _ => {}
        }
    }

    fn place_from_mouse(&mut self, col: u16, row: u16, extend: bool) {
        let t = self.layout.text;
        let g = self.layout.gutter;
        if let Some(b) = self.buffers.get_mut(self.active) {
            let r = b.top + row.saturating_sub(t.y) as usize;
            let cx = (col as i32 - t.x as i32 - g as i32).max(0) as usize;
            let c = b.left + cx;
            b.place_cursor(r, c, extend);
        }
    }

    fn scroll_mouse(&mut self, col: u16, row: u16, dir: i32) {
        let over_tree = self.layout.tree.map_or(false, |tr| self.show_tree && hit(tr, col, row));
        if over_tree {
            let vis = self.tree.visible().len();
            for _ in 0..3 {
                if dir > 0 {
                    self.tree.selected = (self.tree.selected + 1).min(vis.saturating_sub(1));
                } else {
                    self.tree.selected = self.tree.selected.saturating_sub(1);
                }
            }
            self.tree_state.select(Some(self.tree.selected));
        } else if let Some(b) = self.buffers.get_mut(self.active) {
            for _ in 0..3 {
                if dir > 0 {
                    b.down();
                } else {
                    b.up();
                }
            }
        }
    }

    fn editor_key(&mut self, key: KeyEvent) {
        // Completion popup intercepts navigation/accept keys.
        if let Some(c) = &mut self.completion {
            match key.code {
                KeyCode::Up => {
                    c.sel = c.sel.saturating_sub(1);
                    return;
                }
                KeyCode::Down => {
                    c.sel = (c.sel + 1).min(c.items.len().saturating_sub(1));
                    return;
                }
                KeyCode::Enter | KeyCode::Tab => {
                    self.accept_completion();
                    return;
                }
                KeyCode::Esc => {
                    self.completion = None;
                    return;
                }
                _ => self.completion = None, // any other key dismisses, then types
            }
        }
        if key.code == KeyCode::Esc {
            if self.show_tree {
                self.focus = Focus::Tree;
            }
            return;
        }
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let h = self.editor_height;
        let tab = self.settings.tab_size;
        let Some(buf) = self.buffers.get_mut(self.active) else { return };
        match key.code {
            KeyCode::Left => {
                buf.begin_move(shift);
                buf.left();
            }
            KeyCode::Right => {
                buf.begin_move(shift);
                buf.right();
            }
            KeyCode::Up => {
                buf.begin_move(shift);
                buf.up();
            }
            KeyCode::Down => {
                buf.begin_move(shift);
                buf.down();
            }
            KeyCode::Home => {
                buf.begin_move(shift);
                buf.home();
            }
            KeyCode::End => {
                buf.begin_move(shift);
                buf.end();
            }
            KeyCode::PageUp => {
                buf.begin_move(shift);
                buf.page(-1, h);
            }
            KeyCode::PageDown => {
                buf.begin_move(shift);
                buf.page(1, h);
            }
            KeyCode::Enter => buf.insert_newline(),
            KeyCode::Backspace => buf.backspace(),
            KeyCode::Delete => buf.delete(),
            KeyCode::Tab => (0..tab).for_each(|_| buf.insert_char(' ')),
            KeyCode::Char(c) if !is_control_combo(&key) => buf.insert_char(c),
            _ => {}
        }
    }

    fn tree_key(&mut self, key: KeyEvent) {
        let vis: Vec<Entry> = self.tree.visible().into_iter().cloned().collect();
        if vis.is_empty() {
            if key.code == KeyCode::Char('n') {
                self.new_file_prompt(self.tree.root.clone());
            }
            return;
        }
        let sel = self.tree.selected.min(vis.len() - 1);
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.tree.selected = sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.tree.selected = (sel + 1).min(vis.len() - 1),
            KeyCode::Enter | KeyCode::Right => {
                let e = &vis[sel];
                if e.is_dir {
                    self.tree.toggle(&e.path);
                } else {
                    let p = e.path.clone();
                    self.open_path(&p);
                }
            }
            KeyCode::Left => {
                let e = &vis[sel];
                if e.is_dir && self.tree.expanded.contains(&e.path) {
                    self.tree.toggle(&e.path);
                }
            }
            KeyCode::Char('n') => {
                let e = &vis[sel];
                let dir = if e.is_dir {
                    e.path.clone()
                } else {
                    e.path.parent().map(Path::to_path_buf).unwrap_or_else(|| self.tree.root.clone())
                };
                self.new_file_prompt(dir);
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                let e = &vis[sel];
                self.delete_prompt(e.path.clone(), e.is_dir, e.name.clone());
            }
            KeyCode::Esc | KeyCode::Tab => self.focus = Focus::Editor,
            _ => {}
        }
        let len = self.tree.visible().len().max(1);
        self.tree.selected = self.tree.selected.min(len - 1);
        self.tree_state.select(Some(self.tree.selected));
    }

    fn palette_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.focus = Focus::Editor,
            KeyCode::Enter => {
                let items = self.palette_items();
                if let Some((_, action)) = items.into_iter().nth(self.palette_sel) {
                    self.focus = Focus::Editor;
                    self.run(action);
                }
            }
            KeyCode::Up => self.palette_sel = self.palette_sel.saturating_sub(1),
            KeyCode::Down => {
                let n = self.palette_items().len();
                self.palette_sel = (self.palette_sel + 1).min(n.saturating_sub(1));
            }
            KeyCode::Backspace => {
                self.palette_query.pop();
                self.palette_sel = 0;
            }
            KeyCode::Char(c) if !is_control_combo(&key) => {
                self.palette_query.push(c);
                self.palette_sel = 0;
            }
            _ => {}
        }
    }

    fn find_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.focus = Focus::Editor,
            KeyCode::Enter => self.find_next(),
            KeyCode::Backspace => {
                self.find_query.pop();
            }
            KeyCode::Char(c) if !is_control_combo(&key) => self.find_query.push(c),
            _ => {}
        }
    }

    fn prompt_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.prompt = None;
                self.focus = Focus::Tree;
            }
            KeyCode::Enter => self.confirm_prompt(),
            KeyCode::Backspace => {
                if let Some(p) = &mut self.prompt {
                    p.input.pop();
                }
            }
            KeyCode::Char(c) if !is_control_combo(&key) => {
                if let Some(p) = &mut self.prompt {
                    p.input.push(c);
                }
            }
            _ => {}
        }
    }

    fn find_next(&mut self) {
        if self.find_query.is_empty() {
            return;
        }
        let q = self.find_query.clone();
        let Some(buf) = self.buffers.get_mut(self.active) else { return };
        let start_row = buf.row;
        let n = buf.lines.len();
        for step in 0..=n {
            let r = (start_row + step) % n;
            let from = if step == 0 { buf.col + 1 } else { 0 };
            let line = &buf.lines[r];
            let hay: String = line.chars().skip(from).collect();
            if let Some(byte) = hay.find(&q) {
                let col = from + hay[..byte].chars().count();
                buf.set_cursor(r, col);
                self.status = format!("Found '{q}'");
                return;
            }
        }
        self.status = format!("No matches for '{q}'");
    }

    pub fn palette_items(&self) -> Vec<(String, Action)> {
        let cmds: [(&str, &str, Action); 15] = [
            ("Save File", "save", Action::Save),
            ("New File", "new file create", Action::NewFile),
            ("Close Buffer", "close", Action::Close),
            ("Toggle File Tree", "tree files", Action::ToggleTree),
            ("Preferences: Settings", "settings preferences config", Action::Settings),
            ("Rusty: Update (reinstall latest)", "update upgrade", Action::Update),
            ("Git: Initialize Repository", "git initialize repository init create", Action::GitInit),
            ("Git: Add Remote…", "git add remote origin url", Action::AddRemotePrompt),
            ("Git: Stage Current File", "git stage current file add", Action::StageFile),
            ("Git: Stage All Changes", "git stage all changes add bulk", Action::StageAll),
            ("Git: Commit…", "git commit", Action::CommitPrompt),
            ("Git: Push", "git push upload origin", Action::Push),
            ("Git: Pull", "git pull update origin", Action::Pull),
            ("Git: Fetch", "git fetch origin", Action::Fetch),
            ("Quit", "quit exit", Action::Quit),
        ];
        let q = self.palette_query.trim();
        let mut scored: Vec<(i32, String, Action)> = Vec::new();
        for (label, key, action) in cmds {
            if let Some(s) = fuzzy(q, key) {
                scored.push((s, format!("> {label}"), action));
            }
        }
        for (name, path) in self.tree.files() {
            if let Some(s) = fuzzy(q, name) {
                scored.push((s, name.to_string(), Action::Open(path.to_path_buf())));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().take(200).map(|(_, l, a)| (l, a)).collect()
    }

    fn run(&mut self, action: Action) {
        match action {
            Action::Open(p) => self.open_path(&p),
            Action::Save => self.save(),
            Action::Close => self.close_active(),
            Action::ToggleTree => self.show_tree = !self.show_tree,
            Action::NewFile => self.new_file_prompt(self.tree.root.clone()),
            Action::StageFile => {
                if let Some(p) = self.buffers.get(self.active).and_then(|b| b.path.clone()) {
                    match git::stage(&p) {
                        Ok(()) => self.status = format!("Staged {}", p.display()),
                        Err(e) => self.status = format!("Stage failed: {e}"),
                    }
                    self.refresh_head();
                } else {
                    self.status = "No file to stage".to_string();
                }
            }
            Action::GitInit => {
                match git::init(&self.tree.root) {
                    Ok(()) => self.status = "Initialized empty git repository".to_string(),
                    Err(e) => self.status = format!("git init failed: {e}"),
                }
                self.refresh_head();
            }
            Action::StageAll => {
                match git::stage_all(&self.tree.root) {
                    Ok(()) => self.status = "Staged all changes".to_string(),
                    Err(e) => self.status = format!("Stage all failed: {e}"),
                }
                self.refresh_head();
            }
            Action::AddRemotePrompt => {
                self.prompt = Some(Prompt {
                    label: "Remote URL (origin):".to_string(),
                    input: String::new(),
                    kind: PromptKind::AddRemote,
                });
                self.focus = Focus::Prompt;
            }
            Action::CommitPrompt => {
                self.prompt = Some(Prompt {
                    label: "Commit message:".to_string(),
                    input: String::new(),
                    kind: PromptKind::Commit,
                });
                self.focus = Focus::Prompt;
            }
            Action::Settings => {
                self.focus = Focus::Settings;
                self.settings_sel = 0;
            }
            Action::Push => {
                // Uses the user's configured git credentials via the shell.
                self.run_in_shell("git push -u origin HEAD");
                self.status = "Pushing to origin…".to_string();
            }
            Action::Pull => {
                self.run_in_shell("git pull");
                self.status = "Pulling from origin…".to_string();
            }
            Action::Fetch => {
                self.run_in_shell("git fetch --all");
                self.status = "Fetching…".to_string();
            }
            Action::Update => {
                self.run_in_shell(&format!("cargo install --git {REPO_URL} --force"));
                self.status = "Updating Rusty… restart when it finishes".to_string();
            }
            Action::Quit => self.quit = true,
        }
    }
}

fn is_control_combo(key: &KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) || key.modifiers.contains(KeyModifiers::ALT)
}

pub fn fuzzy(query: &str, cand: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let q: Vec<char> = query.to_lowercase().chars().collect();
    let c: Vec<char> = cand.to_lowercase().chars().collect();
    let mut qi = 0;
    let mut score = 0;
    let mut last: Option<usize> = None;
    for (i, ch) in c.iter().enumerate() {
        if qi < q.len() && *ch == q[qi] {
            if i == 0 {
                score += 10;
            }
            if matches!(last, Some(l) if i == l + 1) {
                score += 5;
            }
            score += 1;
            last = Some(i);
            qi += 1;
        }
    }
    (qi == q.len()).then_some(score)
}
