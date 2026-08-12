//! Application state and input handling.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::widgets::ListState;

use crate::buffer::Buffer;
use crate::config::Settings;
use crate::git;
use crate::highlight::Highlighter;
use crate::lsp::{self, Lsp};
use crate::term::Terminal;
use crate::tree::{Entry, Tree};

/// The GitHub repo this build installs/updates from.
pub const REPO_URL: &str = "https://github.com/codingsushi79/rusty";
/// Number of rows in the settings screen (for selection wrap).
pub const SETTINGS_COUNT: usize = 8;

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

/// Which kind of LSP request a pending response id belongs to.
#[derive(Clone, Copy)]
pub enum Req {
    Completion,
    Definition,
    Hover,
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
    Branches,
}

/// One workspace-search hit: (file, 0-indexed line, trimmed line text).
pub type Hit = (PathBuf, usize, String);

/// Editing mode when Vim mode is enabled.
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Mode {
    Insert,
    Normal,
}

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
    Discard,
    GotoDef,
    Hover,
    GotoLinePrompt,
    RunExtension(usize),
    AiEnable,
    AiAskPrompt,
    AiExplain,
    AiSetModelPrompt,
    AiSetEndpointPrompt,
    AiSetTokenPrompt,
    AiToggle,
    CommitPrompt,
    GitInit,
    AddRemotePrompt,
    Push,
    Pull,
    Fetch,
    SwitchBranch,
    RenameBranchPrompt,
    DeleteBranchPrompt,
    PublishBranch,
    Settings,
    Update,
    OpenLog,
    ToggleVim,
    Quit,
}

pub enum PromptKind {
    NewFile(PathBuf),
    Commit,
    Delete(PathBuf),
    AddRemote,
    Discard(PathBuf),
    RenameBranch,
    DeleteBranch,
    GotoLine,
    AiAsk,
    AiModel,
    AiEndpoint,
    AiToken,
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

    pub term: Option<Terminal>,
    pub term_open: bool,

    pub extensions: Vec<crate::extensions::Extension>,
    pub ai_open: bool,
    pub ai_output: String,
    ai_rx: Option<Receiver<String>>,

    pub settings: Settings,
    pub settings_sel: usize,

    pub mode: Mode,
    vim_pending: Option<char>,

    pub branch_list: Vec<String>,
    pub branch_query: String,
    pub branch_sel: usize,

    // Background jobs (hidden terminal) → each yields a final (ok, message).
    jobs: Vec<Receiver<(bool, String)>>,
    last_status: String,
    status_time: Instant,
    last_tree_scan: Instant,

    clipboard: String,
    sys_clip: Option<arboard::Clipboard>,

    lsp: Option<Lsp>,
    lsp_started: bool,
    lsp_sent: HashMap<String, u64>,
    pub diagnostics: HashMap<String, Vec<Diagnostic>>,
    pub completion: Option<Completion>,
    pending: HashMap<i64, Req>,
    pub git_status_map: HashMap<PathBuf, char>,

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
        let start_mode = if settings.vim_mode { Mode::Normal } else { Mode::Insert };
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
            term: None,
            term_open: false,
            extensions: crate::extensions::discover(root),
            ai_open: false,
            ai_output: String::new(),
            ai_rx: None,
            settings,
            settings_sel: 0,
            mode: start_mode,
            vim_pending: None,
            branch_list: Vec::new(),
            branch_query: String::new(),
            branch_sel: 0,
            jobs: Vec::new(),
            last_status: String::new(),
            status_time: Instant::now(),
            last_tree_scan: Instant::now(),
            clipboard: String::new(),
            sys_clip: arboard::Clipboard::new().ok(),
            lsp: None,
            lsp_started: false,
            lsp_sent: HashMap::new(),
            diagnostics: HashMap::new(),
            completion: None,
            pending: HashMap::new(),
            git_status_map: git::statuses(root),
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
                Err(e) => self.err(format!("Open failed: {e}")),
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
        // Drain first so we don't hold a borrow of `self.lsp` while dispatching.
        let msgs = match &self.lsp {
            Some(l) => l.drain(),
            None => return false,
        };
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
                match self.pending.remove(&id) {
                    Some(Req::Completion) => self.handle_completion(&v["result"]),
                    Some(Req::Definition) => self.handle_definition(v["result"].clone()),
                    Some(Req::Hover) => self.handle_hover(&v["result"]),
                    None => {}
                }
            }
        }
        true
    }

    fn handle_completion(&mut self, result: &serde_json::Value) {
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

    fn handle_hover(&mut self, result: &serde_json::Value) {
        let contents = &result["contents"];
        let text = if let Some(s) = contents.as_str() {
            s.to_string()
        } else if let Some(s) = contents["value"].as_str() {
            s.to_string()
        } else if let Some(arr) = contents.as_array() {
            arr.first()
                .and_then(|c| c.as_str().or_else(|| c["value"].as_str()))
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        };
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty() && !l.starts_with("```")).collect();
        // Prefer a signature-like line (has a space) over a bare module path.
        let line = lines.iter().find(|l| l.contains(' ')).or_else(|| lines.first()).copied().unwrap_or("");
        self.status = if line.is_empty() { "No hover info".to_string() } else { line.trim().to_string() };
    }

    fn handle_definition(&mut self, result: serde_json::Value) {
        let loc = if result.is_array() { result.get(0).cloned().unwrap_or(serde_json::Value::Null) } else { result };
        let uri = loc["uri"].as_str().or_else(|| loc["targetUri"].as_str());
        let range = if loc.get("range").is_some() { &loc["range"] } else { &loc["targetSelectionRange"] };
        if let Some(uri) = uri {
            let line = range["start"]["line"].as_u64().unwrap_or(0) as usize;
            let ch = range["start"]["character"].as_u64().unwrap_or(0) as usize;
            let path = PathBuf::from(uri.strip_prefix("file://").unwrap_or(uri));
            self.open_path(&path);
            if let Some(b) = self.buffers.get_mut(self.active) {
                b.set_cursor(line, ch);
            }
        } else {
            self.status = "No definition found".to_string();
        }
    }

    fn trigger_completion(&mut self) {
        if let Some(id) = self.lsp_request_at_cursor(|l, uri, line, col| l.completion(uri, line, col)) {
            self.pending.insert(id, Req::Completion);
            self.status = "Completing…".to_string();
        }
    }

    fn trigger_definition(&mut self) {
        if let Some(id) = self.lsp_request_at_cursor(|l, uri, line, col| l.definition(uri, line, col)) {
            self.pending.insert(id, Req::Definition);
            self.status = "Go to definition…".to_string();
        }
    }

    fn trigger_hover(&mut self) {
        if let Some(id) = self.lsp_request_at_cursor(|l, uri, line, col| l.hover(uri, line, col)) {
            self.pending.insert(id, Req::Hover);
        }
    }

    /// Issue an LSP request positioned at the current cursor; returns its id.
    fn lsp_request_at_cursor(
        &mut self,
        f: impl FnOnce(&mut Lsp, &str, usize, usize) -> i64,
    ) -> Option<i64> {
        let b = self.buffers.get(self.active)?;
        let path = b.path.clone()?;
        let (line, col) = (b.row, b.col);
        let uri = uri_of(&path);
        let l = self.lsp.as_mut()?;
        Some(f(l, &uri, line, col))
    }

    pub fn refresh_git_status(&mut self) {
        self.git_status_map = git::statuses(&self.tree.root);
    }

    /// Branches matching the filter query.
    pub fn branches_filtered(&self) -> Vec<String> {
        let q = self.branch_query.trim().to_lowercase();
        self.branch_list.iter().filter(|b| q.is_empty() || b.to_lowercase().contains(&q)).cloned().collect()
    }

    fn branches_key(&mut self, key: KeyEvent) {
        let filtered = self.branches_filtered();
        match key.code {
            KeyCode::Esc => self.focus = Focus::Editor,
            KeyCode::Up => self.branch_sel = self.branch_sel.saturating_sub(1),
            KeyCode::Down => self.branch_sel = (self.branch_sel + 1).min(filtered.len().saturating_sub(1)),
            KeyCode::Backspace => {
                self.branch_query.pop();
                self.branch_sel = 0;
            }
            KeyCode::Char(c) if !is_control_combo(&key) => {
                self.branch_query.push(c);
                self.branch_sel = 0;
            }
            KeyCode::Enter => {
                self.focus = Focus::Editor;
                if let Some(name) = filtered.get(self.branch_sel).cloned() {
                    match git::checkout_branch(&self.tree.root, &name) {
                        Ok(()) => {
                            self.reload_after_git();
                            self.status = format!("Switched to branch '{name}'");
                        }
                        Err(e) => self.status = format!("Checkout failed: {e}"),
                    }
                } else if !self.branch_query.trim().is_empty() {
                    let name = self.branch_query.trim().to_string();
                    match git::create_branch(&self.tree.root, &name) {
                        Ok(()) => {
                            self.reload_after_git();
                            self.status = format!("Created and switched to '{name}'");
                        }
                        Err(e) => self.status = format!("Create branch failed: {e}"),
                    }
                }
            }
            _ => {}
        }
    }

    /// After a checkout/branch change, reload buffers, tree, and git state.
    fn reload_after_git(&mut self) {
        for i in 0..self.buffers.len() {
            if let Some(p) = self.buffers[i].path.clone() {
                if p.exists() {
                    if let Ok(mut nb) = Buffer::from_file(&p) {
                        nb.set_head(git::head_text(&p));
                        self.buffers[i] = nb;
                    }
                }
            }
        }
        self.tree.reload();
        self.refresh_head();
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
        self.refresh_git_status();
    }

    fn save(&mut self) {
        let Some(buf) = self.buffers.get_mut(self.active) else { return };
        if buf.path.is_none() {
            self.status = "No filename".to_string();
            return;
        }
        match buf.save() {
            Ok(()) => self.status = format!("Saved {}", buf.name()),
            Err(e) => self.err(format!("Save failed: {e}")),
        }
        self.refresh_git_status();
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
                self.refresh_git_status();
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
            PromptKind::GotoLine => {
                self.focus = Focus::Editor;
                if let Ok(n) = input.trim().parse::<usize>() {
                    if let Some(b) = self.buffers.get_mut(self.active) {
                        b.set_cursor(n.saturating_sub(1), 0);
                    }
                }
            }
            PromptKind::AiAsk => {
                self.focus = Focus::Editor;
                if !input.is_empty() {
                    let ctx = self.buffers.get(self.active).map(|b| b.text()).unwrap_or_default();
                    let prompt = if ctx.trim().is_empty() {
                        input.clone()
                    } else {
                        format!("Current file:\n```\n{ctx}\n```\n\nQuestion: {input}")
                    };
                    self.ai_ask(&prompt);
                }
            }
            PromptKind::AiModel => {
                self.focus = Focus::Editor;
                if !input.is_empty() {
                    self.settings.ai_model = input.clone();
                    self.settings.save();
                    self.status = format!("AI model set to '{input}'");
                }
            }
            PromptKind::AiEndpoint => {
                self.focus = Focus::Editor;
                if !input.is_empty() {
                    self.settings.ai_endpoint = input.clone();
                    self.settings.save();
                    self.status = format!("AI endpoint set to '{input}'");
                }
            }
            PromptKind::AiToken => {
                self.focus = Focus::Editor;
                // Blank input clears the token.
                self.settings.ai_api_key = input.clone();
                self.settings.save();
                self.status = if input.is_empty() {
                    "AI token cleared".to_string()
                } else {
                    "AI token saved".to_string()
                };
            }
            PromptKind::RenameBranch => {
                self.focus = Focus::Editor;
                if !input.is_empty() {
                    match git::rename_branch(&self.tree.root, &input) {
                        Ok(()) => self.status = format!("Renamed branch to '{input}'"),
                        Err(e) => self.status = format!("Rename failed: {e}"),
                    }
                    self.refresh_head();
                }
            }
            PromptKind::DeleteBranch => {
                self.focus = Focus::Editor;
                if !input.is_empty() {
                    match git::delete_branch(&self.tree.root, &input) {
                        Ok(()) => self.status = format!("Deleted branch '{input}'"),
                        Err(e) => self.status = format!("Delete branch failed: {e}"),
                    }
                }
            }
            PromptKind::Discard(path) => {
                self.focus = Focus::Editor;
                match git::discard(&path) {
                    Ok(()) => {
                        // Reload the buffer from disk to reflect the restored file.
                        if let Some(i) = self.buffers.iter().position(|b| b.path.as_deref() == Some(&path)) {
                            if let Ok(mut nb) = Buffer::from_file(&path) {
                                nb.set_head(git::head_text(&path));
                                self.buffers[i] = nb;
                            }
                        }
                        self.status = "Discarded changes".to_string();
                    }
                    Err(e) => self.status = format!("Discard failed: {e}"),
                }
                self.refresh_git_status();
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
                        self.refresh_git_status();
                        self.status = format!("Deleted {}", path.display());
                    }
                    Err(e) => self.status = format!("Delete failed: {e}"),
                }
            }
        }
    }

    // --- input -------------------------------------------------------------

    pub fn on_key(&mut self, key: KeyEvent) {
        // When the terminal is focused, it captures all input.
        if self.focus == Focus::Shell {
            self.terminal_key(key);
            return;
        }
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
                KeyCode::Char('k') => {
                    self.trigger_hover();
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
            Focus::Shell => {} // handled above
            Focus::Settings => self.settings_key(key),
            Focus::Branches => self.branches_key(key),
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
            3 => {
                self.settings.vim_mode = !self.settings.vim_mode;
                self.mode = if self.settings.vim_mode { Mode::Normal } else { Mode::Insert };
            }
            4 => self.settings.ai_enabled = !self.settings.ai_enabled,
            // AI text fields: Enter/Right opens an edit prompt.
            5 if dir > 0 => self.open_edit_prompt(
                format!("API endpoint (current: {}):", self.settings.ai_endpoint),
                PromptKind::AiEndpoint,
            ),
            6 if dir > 0 => self.open_edit_prompt(
                format!("Model name (current: {}):", self.settings.ai_model),
                PromptKind::AiModel,
            ),
            7 if dir > 0 => self.open_edit_prompt(
                "API token (stored in config; blank clears):".to_string(),
                PromptKind::AiToken,
            ),
            _ => {}
        }
        self.settings.save();
    }

    fn open_edit_prompt(&mut self, label: String, kind: PromptKind) {
        self.prompt = Some(Prompt { label, input: String::new(), kind });
        self.focus = Focus::Prompt;
    }

    fn apply_theme(&mut self, name: &str) {
        if self.hl.set_theme(name) {
            self.settings.syntax_theme = name.to_string();
            for b in &mut self.buffers {
                b.force_rehighlight();
            }
        }
    }

    /// Run a WASM plugin extension and apply its effects.
    fn run_wasm_extension(&mut self, e: &crate::extensions::Extension) {
        let path = {
            let p = std::path::Path::new(&e.wasm);
            if p.is_absolute() { p.to_path_buf() } else { e.dir.join(&e.wasm) }
        };
        let (text, line, col) = self
            .buffers
            .get(self.active)
            .map(|b| (b.text(), (b.row + 1) as i32, (b.col + 1) as i32))
            .unwrap_or_default();
        match crate::wasmext::run(&path, text, line, col) {
            Ok(host) => {
                for l in &host.logs {
                    crate::logging::info(&format!("[ext {}] {l}", e.name));
                }
                if let Some(b) = self.buffers.get_mut(self.active) {
                    for ins in &host.inserts {
                        b.insert_str(ins);
                    }
                }
                if let Some(s) = host.status {
                    self.status = s;
                } else {
                    self.status = format!("Ran {}", e.name);
                }
            }
            Err(err) => self.err(format!("Ext {} failed: {err}", e.name)),
        }
    }

    /// Run a command in a hidden background terminal; the result lands in the
    /// status bar. `label` is shown while it runs (e.g. "Pushing to origin").
    fn run_hidden(&mut self, label: &str, cmd: &str) {
        self.set_status(format!("{label}…"));
        let (tx, rx) = channel();
        let cwd = self.tree.root.clone();
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let script = format!("{cmd} 2>&1");
        let label = label.to_string();
        // Expose editor context to the command (used by extensions).
        let file = self.buffers.get(self.active).and_then(|b| b.path.clone());
        let env_file = file.map(|p| p.display().to_string()).unwrap_or_default();
        let (env_line, env_col) = self
            .buffers
            .get(self.active)
            .map(|b| (b.row + 1, b.col + 1))
            .unwrap_or((1, 1));
        let env_root = cwd.display().to_string();
        std::thread::spawn(move || {
            let msg = match Command::new(shell)
                .arg("-c")
                .arg(&script)
                .current_dir(&cwd)
                .env("RUSTY_FILE", &env_file)
                .env("RUSTY_LINE", env_line.to_string())
                .env("RUSTY_COL", env_col.to_string())
                .env("RUSTY_ROOT", &env_root)
                .output()
            {
                Ok(o) => {
                    let text = String::from_utf8_lossy(&o.stdout);
                    let last = text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
                    if o.status.success() {
                        (true, if last.is_empty() { format!("{label} ✓") } else { format!("{label}: {last}") })
                    } else {
                        (false, format!("{label} failed: {}", if last.is_empty() { "see git output".into() } else { last }))
                    }
                }
                Err(e) => (false, format!("{label} failed: {e}")),
            };
            let _ = tx.send(msg);
        });
        self.jobs.push(rx);
    }

    fn set_status(&mut self, s: String) {
        self.status = s;
    }

    /// Set the status to an error message and record it in the log file.
    fn err(&mut self, msg: String) {
        crate::logging::error(&msg);
        self.status = msg;
    }

    /// Ask the local model in a background thread; the answer shows in the AI panel.
    fn ai_ask(&mut self, prompt: &str) {
        if !self.settings.ai_enabled {
            self.status = "Enable Local AI first (palette → AI: Enable)".to_string();
            return;
        }
        self.ai_open = true;
        self.ai_output.clear();
        let (tx, rx) = channel();
        let endpoint = self.settings.ai_endpoint.clone();
        let model = self.settings.ai_model.clone();
        let key = self.settings.ai_api_key.clone();
        let prompt = prompt.to_string();
        std::thread::spawn(move || {
            let tx2 = tx.clone();
            let result = crate::ai::generate_stream(&endpoint, &model, &key, &prompt, move |chunk| {
                let _ = tx2.send(chunk.to_string());
            });
            if let Err(e) = result {
                crate::logging::error(&format!("AI request failed: {e}"));
                let _ = tx.send(format!(
                    "AI error: {e}\n\nCheck the endpoint/model/token, or start a local server (ollama serve)."
                ));
            }
        });
        self.ai_rx = Some(rx);
    }

    /// Append streamed AI chunks into the panel. Returns true if it changed.
    pub fn ai_poll(&mut self) -> bool {
        let mut changed = false;
        let mut disconnected = false;
        if let Some(rx) = &self.ai_rx {
            loop {
                match rx.try_recv() {
                    Ok(chunk) => {
                        self.ai_output.push_str(&chunk);
                        changed = true;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        if disconnected {
            self.ai_rx = None;
        }
        changed
    }

    /// Per-tick housekeeping: collect finished jobs and expire stale status.
    /// Returns true if a redraw is warranted.
    pub fn tick(&mut self) -> bool {
        let mut dirty = false;
        let mut i = 0;
        while i < self.jobs.len() {
            match self.jobs[i].try_recv() {
                Ok((ok, msg)) => {
                    if !ok {
                        crate::logging::error(&msg);
                    }
                    self.status = msg;
                    self.jobs.remove(i);
                    dirty = true;
                }
                Err(TryRecvError::Empty) => i += 1,
                Err(TryRecvError::Disconnected) => {
                    self.jobs.remove(i);
                }
            }
        }
        // Reset the timer whenever the status text changes.
        if self.status != self.last_status {
            self.last_status = self.status.clone();
            self.status_time = Instant::now();
            dirty = true;
        }
        // Fade the status after a few idle seconds (but keep it while a job runs).
        if !self.status.is_empty() && self.jobs.is_empty() && self.status_time.elapsed().as_secs() >= 4 {
            self.status.clear();
            self.last_status.clear();
            dirty = true;
        }

        // Periodically rescan the workspace so files added/removed outside the
        // editor appear. Only redraw when the tree actually changed.
        if self.tree.root.is_dir() && self.last_tree_scan.elapsed() >= Duration::from_millis(1500) {
            self.last_tree_scan = Instant::now();
            let before: Vec<PathBuf> = self.tree.entries.iter().map(|e| e.path.clone()).collect();
            let sel_path = self.tree.visible().get(self.tree.selected).map(|e| e.path.clone());
            self.tree.reload();
            let after: Vec<PathBuf> = self.tree.entries.iter().map(|e| e.path.clone()).collect();
            if before != after {
                if let Some(p) = sel_path {
                    if let Some(i) = self.tree.visible_index(&p) {
                        self.tree.selected = i;
                    }
                }
                let vis = self.tree.visible().len().max(1);
                self.tree.selected = self.tree.selected.min(vis - 1);
                self.refresh_git_status();
                dirty = true;
            }
        }
        dirty
    }

    fn toggle_shell(&mut self) {
        if !self.term_open {
            self.term_open = true;
            self.focus = Focus::Shell;
            if self.term.is_none() {
                match Terminal::spawn(self.tree.root.clone(), 24, 80) {
                    Ok(t) => self.term = Some(t),
                    Err(e) => {
                        self.err(format!("Could not start terminal: {e}"));
                        self.term_open = false;
                        self.focus = Focus::Editor;
                    }
                }
            }
        } else if self.focus != Focus::Shell {
            self.focus = Focus::Shell;
        } else {
            self.term_open = false;
            self.focus = Focus::Editor;
        }
    }

    /// While the terminal is focused, keys go straight to the PTY (so `vim`,
    /// `Ctrl+C`, arrows all work); `Ctrl+J` is the escape hatch back to the editor.
    fn terminal_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('j') {
            self.toggle_shell();
            return;
        }
        if let Some(t) = &mut self.term {
            t.key(key);
        }
    }

    /// Drain PTY output into the emulator. Returns true if anything changed.
    pub fn term_poll(&mut self) -> bool {
        if let Some(t) = &mut self.term {
            t.poll()
        } else {
            false
        }
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

    fn buf_do(&mut self, f: impl FnOnce(&mut Buffer)) {
        if let Some(b) = self.buffers.get_mut(self.active) {
            f(b);
        }
    }

    /// Vim Normal-mode keys: motions, operators, and mode changes.
    fn normal_key(&mut self, key: KeyEvent) {
        use KeyCode::*;
        // Operator-pending (dd, yy, gg).
        if let Some(op) = self.vim_pending.take() {
            match (op, key.code) {
                ('d', Char('d')) => {
                    if let Some(b) = self.buffers.get_mut(self.active) {
                        let t = b.delete_line();
                        self.set_clip(t);
                    }
                }
                ('y', Char('y')) => {
                    if let Some(t) = self.buffers.get(self.active).map(|b| format!("{}\n", b.lines[b.row])) {
                        self.set_clip(t);
                    }
                }
                ('g', Char('g')) => self.buf_do(|b| b.goto_top()),
                _ => {}
            }
            return;
        }
        let h = self.editor_height;
        match key.code {
            Char('h') | Left => self.buf_do(|b| b.left()),
            Char('l') | Right => self.buf_do(|b| b.right()),
            Char('j') | Down => self.buf_do(|b| b.down()),
            Char('k') | Up => self.buf_do(|b| b.up()),
            Char('0') | Home => self.buf_do(|b| b.home()),
            Char('$') | End => self.buf_do(|b| b.end()),
            Char('^') => self.buf_do(|b| b.first_non_blank()),
            Char('w') => self.buf_do(|b| b.next_word()),
            Char('b') => self.buf_do(|b| b.prev_word()),
            Char('G') => self.buf_do(|b| b.goto_bottom()),
            PageUp => self.buf_do(|b| b.page(-1, h)),
            PageDown => self.buf_do(|b| b.page(1, h)),
            Char('x') | Delete => self.buf_do(|b| b.delete()),
            Char('D') => self.buf_do(|b| b.delete_to_eol()),
            Char('u') => self.buf_do(|b| b.undo()),
            Char('p') => {
                let t = self.get_clip();
                self.buf_do(|b| b.insert_str(&t));
            }
            Char('i') => self.mode = Mode::Insert,
            Char('a') => {
                self.buf_do(|b| b.right());
                self.mode = Mode::Insert;
            }
            Char('A') => {
                self.buf_do(|b| b.end());
                self.mode = Mode::Insert;
            }
            Char('I') => {
                self.buf_do(|b| b.first_non_blank());
                self.mode = Mode::Insert;
            }
            Char('o') => {
                self.buf_do(|b| b.open_below());
                self.mode = Mode::Insert;
            }
            Char('O') => {
                self.buf_do(|b| b.open_above());
                self.mode = Mode::Insert;
            }
            Char('d') => self.vim_pending = Some('d'),
            Char('y') => self.vim_pending = Some('y'),
            Char('g') => self.vim_pending = Some('g'),
            Char(':') => {
                self.focus = Focus::Palette;
                self.palette_query.clear();
                self.palette_sel = 0;
            }
            Char('/') => {
                self.focus = Focus::Find;
                self.find_query.clear();
            }
            _ => {}
        }
    }

    fn editor_key(&mut self, key: KeyEvent) {
        // Vim Normal mode: motions/commands only.
        if self.settings.vim_mode && self.mode == Mode::Normal {
            self.normal_key(key);
            return;
        }
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
            if self.settings.vim_mode {
                self.mode = Mode::Normal;
            } else if self.show_tree {
                self.focus = Focus::Tree;
            }
            return;
        }
        if key.code == KeyCode::F(12) {
            self.trigger_definition();
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
        let cmds: [(&str, &str, Action); 24] = [
            ("Save File", "save", Action::Save),
            ("New File", "new file create", Action::NewFile),
            ("Close Buffer", "close", Action::Close),
            ("Toggle File Tree", "tree files", Action::ToggleTree),
            ("Go to Definition", "go to definition lsp symbol", Action::GotoDef),
            ("Hover (type info)", "hover type info lsp", Action::Hover),
            ("Editor: Toggle Vim Mode", "editor toggle vim mode modal motions", Action::ToggleVim),
            ("Preferences: Settings", "settings preferences config", Action::Settings),
            ("Rusty: Update (reinstall latest)", "update upgrade", Action::Update),
            ("Rusty: Open Log", "open log file errors debug", Action::OpenLog),
            ("Git: Initialize Repository", "git initialize repository init create", Action::GitInit),
            ("Git: Add Remote…", "git add remote origin url", Action::AddRemotePrompt),
            ("Git: Stage Current File", "git stage current file add", Action::StageFile),
            ("Git: Stage All Changes", "git stage all changes add bulk", Action::StageAll),
            ("Git: Discard Changes (current file)", "git discard revert changes current file", Action::Discard),
            ("Git: Commit…", "git commit", Action::CommitPrompt),
            ("Git: Push", "git push upload origin", Action::Push),
            ("Git: Pull", "git pull update origin", Action::Pull),
            ("Git: Fetch", "git fetch origin", Action::Fetch),
            ("Git: Switch / Create Branch…", "git switch checkout create branch", Action::SwitchBranch),
            ("Git: Rename Branch…", "git rename branch name", Action::RenameBranchPrompt),
            ("Git: Delete Branch…", "git delete branch remove", Action::DeleteBranchPrompt),
            ("Git: Publish Branch (push -u)", "git publish push upstream branch", Action::PublishBranch),
            ("Quit", "quit exit", Action::Quit),
        ];
        let q = self.palette_query.trim();
        let mut scored: Vec<(i32, String, Action)> = Vec::new();
        for (label, key, action) in cmds {
            if let Some(s) = fuzzy(q, key) {
                scored.push((s, format!("> {label}"), action));
            }
        }

        // Dynamic commands: go-to-line, extensions, and AI.
        let mut extra: Vec<(String, String, Action)> = vec![(
            "Go to Line…".into(),
            "go to line number jump".into(),
            Action::GotoLinePrompt,
        )];
        for (i, e) in self.extensions.iter().enumerate() {
            extra.push((format!("Ext: {}", e.name), format!("ext extension {}", e.name), Action::RunExtension(i)));
        }
        if self.settings.ai_enabled {
            extra.push(("AI: Ask…".into(), "ai ask question local model".into(), Action::AiAskPrompt));
            extra.push(("AI: Explain Selection".into(), "ai explain selection code".into(), Action::AiExplain));
            extra.push(("AI: Set Model…".into(), "ai set model".into(), Action::AiSetModelPrompt));
            extra.push(("AI: Set Endpoint…".into(), "ai set endpoint url provider".into(), Action::AiSetEndpointPrompt));
            extra.push(("AI: Set API Token (BYOT)…".into(), "ai set api token key byot cloud".into(), Action::AiSetTokenPrompt));
            extra.push(("AI: Toggle Panel".into(), "ai panel toggle show".into(), Action::AiToggle));
        } else {
            extra.push(("AI: Enable Local AI (opt-in)".into(), "ai enable local turn on".into(), Action::AiEnable));
        }
        for (label, key, action) in extra {
            if let Some(s) = fuzzy(q, &key) {
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
            Action::Discard => {
                if let Some(p) = self.buffers.get(self.active).and_then(|b| b.path.clone()) {
                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    self.prompt = Some(Prompt {
                        label: format!("Discard all changes in '{name}'?"),
                        input: String::new(),
                        kind: PromptKind::Discard(p),
                    });
                    self.focus = Focus::Prompt;
                } else {
                    self.status = "No file to discard".to_string();
                }
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
            Action::GotoDef => self.trigger_definition(),
            Action::Hover => self.trigger_hover(),
            Action::GotoLinePrompt => {
                self.prompt = Some(Prompt {
                    label: "Go to line:".to_string(),
                    input: String::new(),
                    kind: PromptKind::GotoLine,
                });
                self.focus = Focus::Prompt;
            }
            Action::RunExtension(i) => {
                if let Some(e) = self.extensions.get(i).cloned() {
                    if !e.wasm.is_empty() {
                        self.run_wasm_extension(&e);
                    } else if e.terminal {
                        if !self.term_open || self.term.is_none() {
                            self.toggle_shell();
                        }
                        self.term_open = true;
                        self.focus = Focus::Shell;
                        if let Some(t) = &mut self.term {
                            t.send_str(&format!("{}\n", e.command));
                        }
                    } else {
                        self.run_hidden(&format!("Ext: {}", e.name), &e.command);
                    }
                }
            }
            Action::AiEnable => {
                self.settings.ai_enabled = true;
                self.settings.save();
                self.status = "AI enabled — checking connection…".to_string();
                // Probe the endpoint in the background.
                let (tx, rx) = channel();
                let endpoint = self.settings.ai_endpoint.clone();
                let key = self.settings.ai_api_key.clone();
                std::thread::spawn(move || {
                    let ok = crate::ai::available(&endpoint, &key);
                    let msg = if ok {
                        format!("AI connected: {endpoint}")
                    } else {
                        format!("AI enabled, but {endpoint} is unreachable (start a server or set endpoint/token)")
                    };
                    let _ = tx.send((ok, msg));
                });
                self.jobs.push(rx);
            }
            Action::AiToggle => self.ai_open = !self.ai_open,
            Action::AiAskPrompt => {
                self.prompt = Some(Prompt {
                    label: "Ask the local model:".to_string(),
                    input: String::new(),
                    kind: PromptKind::AiAsk,
                });
                self.focus = Focus::Prompt;
            }
            Action::AiSetModelPrompt => {
                self.prompt = Some(Prompt {
                    label: format!("Model name (current: {}):", self.settings.ai_model),
                    input: String::new(),
                    kind: PromptKind::AiModel,
                });
                self.focus = Focus::Prompt;
            }
            Action::AiSetEndpointPrompt => {
                self.prompt = Some(Prompt {
                    label: format!("API endpoint (current: {}):", self.settings.ai_endpoint),
                    input: String::new(),
                    kind: PromptKind::AiEndpoint,
                });
                self.focus = Focus::Prompt;
            }
            Action::AiSetTokenPrompt => {
                self.prompt = Some(Prompt {
                    label: "API token (stored in config; leave blank to clear):".to_string(),
                    input: String::new(),
                    kind: PromptKind::AiToken,
                });
                self.focus = Focus::Prompt;
            }
            Action::AiExplain => {
                let sel = self.buffers.get(self.active).and_then(|b| b.selected_text());
                let code = sel.or_else(|| self.buffers.get(self.active).map(|b| b.text())).unwrap_or_default();
                self.ai_ask(&format!("Explain this code concisely:\n\n{code}"));
            }
            Action::Settings => {
                self.focus = Focus::Settings;
                self.settings_sel = 0;
            }
            // These run hidden in the background; results appear in the status bar.
            Action::Push => self.run_hidden("Pushing to origin", "git push -u origin HEAD"),
            Action::Pull => self.run_hidden("Pulling", "git pull"),
            Action::Fetch => self.run_hidden("Fetching", "git fetch --all"),
            Action::PublishBranch => self.run_hidden("Publishing branch", "git push -u origin HEAD"),
            Action::SwitchBranch => {
                self.branch_list = git::branches(&self.tree.root);
                self.branch_query.clear();
                self.branch_sel = 0;
                self.focus = Focus::Branches;
            }
            Action::RenameBranchPrompt => {
                self.prompt = Some(Prompt {
                    label: "Rename current branch to:".to_string(),
                    input: String::new(),
                    kind: PromptKind::RenameBranch,
                });
                self.focus = Focus::Prompt;
            }
            Action::DeleteBranchPrompt => {
                self.prompt = Some(Prompt {
                    label: "Delete branch named:".to_string(),
                    input: String::new(),
                    kind: PromptKind::DeleteBranch,
                });
                self.focus = Focus::Prompt;
            }
            Action::Update => {
                self.run_hidden("Updating Rusty", &format!("cargo install --git {REPO_URL} --force"))
            }
            Action::ToggleVim => {
                self.settings.vim_mode = !self.settings.vim_mode;
                self.settings.save();
                self.mode = if self.settings.vim_mode { Mode::Normal } else { Mode::Insert };
                self.status = format!("Vim mode {}", if self.settings.vim_mode { "on" } else { "off" });
            }
            Action::OpenLog => match crate::logging::path() {
                Some(p) if p.exists() => self.open_path(&p),
                Some(p) => self.status = format!("No log yet at {}", p.display()),
                None => self.status = "No log path available".to_string(),
            },
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
