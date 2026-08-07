//! A small Language Server Protocol client (JSON-RPC over stdio).
//!
//! One server per workspace. A reader thread parses `Content-Length`-framed
//! messages and forwards them over a channel; the app drains it each tick to
//! pick up diagnostics and completion responses.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};

use serde_json::{json, Value};

/// The language server + language id for a file extension, if we support it.
pub fn server_for(ext: &str) -> Option<(&'static str, &'static str)> {
    match ext {
        "rs" => Some(("rust-analyzer", "rust")),
        "py" => Some(("pylsp", "python")),
        "ts" | "tsx" | "js" | "jsx" => Some(("typescript-language-server", "typescript")),
        "go" => Some(("gopls", "go")),
        _ => None,
    }
}

pub struct Lsp {
    stdin: ChildStdin,
    next_id: i64,
    rx: Receiver<Value>,
    _child: Child,
}

impl Lsp {
    pub fn spawn(cmd: &str, root: &Path) -> Option<Self> {
        let mut child = Command::new(cmd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let stdin = child.stdin.take()?;
        let stdout = child.stdout.take()?;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            loop {
                let mut len = 0usize;
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    if line == "\r\n" || line == "\n" {
                        break;
                    }
                    if let Some(v) = line.strip_prefix("Content-Length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
                if len == 0 {
                    continue;
                }
                let mut buf = vec![0u8; len];
                if r.read_exact(&mut buf).is_err() {
                    return;
                }
                if let Ok(v) = serde_json::from_slice::<Value>(&buf) {
                    if tx.send(v).is_err() {
                        return;
                    }
                }
            }
        });

        let mut lsp = Self { stdin, next_id: 1, rx, _child: child };
        let root_uri = format!("file://{}", root.display());
        lsp.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": root_uri,
                "capabilities": {
                    "textDocument": {
                        "publishDiagnostics": {},
                        "completion": { "completionItem": { "snippetSupport": false } }
                    }
                }
            }),
        );
        lsp.notify("initialized", json!({}));
        Some(lsp)
    }

    fn send(&mut self, v: &Value) {
        let body = serde_json::to_vec(v).unwrap_or_default();
        let _ = write!(self.stdin, "Content-Length: {}\r\n\r\n", body.len());
        let _ = self.stdin.write_all(&body);
        let _ = self.stdin.flush();
    }

    fn request(&mut self, method: &str, params: Value) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        id
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    pub fn did_open(&mut self, uri: &str, language_id: &str, text: &str) {
        self.notify(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri, "languageId": language_id, "version": 1, "text": text}}),
        );
    }

    pub fn did_change(&mut self, uri: &str, version: i64, text: &str) {
        self.notify(
            "textDocument/didChange",
            json!({"textDocument": {"uri": uri, "version": version}, "contentChanges": [{"text": text}]}),
        );
    }

    /// Request completion; returns the request id to correlate the response.
    pub fn completion(&mut self, uri: &str, line: usize, character: usize) -> i64 {
        self.request(
            "textDocument/completion",
            json!({"textDocument": {"uri": uri}, "position": {"line": line, "character": character}}),
        )
    }

    pub fn definition(&mut self, uri: &str, line: usize, character: usize) -> i64 {
        self.request(
            "textDocument/definition",
            json!({"textDocument": {"uri": uri}, "position": {"line": line, "character": character}}),
        )
    }

    pub fn hover(&mut self, uri: &str, line: usize, character: usize) -> i64 {
        self.request(
            "textDocument/hover",
            json!({"textDocument": {"uri": uri}, "position": {"line": line, "character": character}}),
        )
    }

    /// Drain all messages received since the last call.
    pub fn drain(&self) -> Vec<Value> {
        self.rx.try_iter().collect()
    }
}
