//! Integrated shell pane — a command runner.
//!
//! Each submitted line runs with `$SHELL -c` in the pane's working directory;
//! combined stdout/stderr streams back through a channel the app drains each
//! tick. `cd` persists between commands. (A full interactive PTY is a separate,
//! larger effort; this stays reliable and clean.)

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};

pub struct Shell {
    pub cwd: PathBuf,
    tx: Sender<String>,
}

impl Shell {
    pub fn new(cwd: PathBuf) -> (Self, Receiver<String>) {
        let (tx, rx) = channel();
        let cwd = cwd.canonicalize().unwrap_or(cwd);
        let _ = tx.send(format!("rusty shell — {}\n", pretty(&cwd)));
        (Self { cwd, tx }, rx)
    }

    pub fn prompt(&self) -> String {
        format!("{} $ ", pretty(&self.cwd))
    }

    pub fn run(&mut self, line: &str) {
        let line = line.trim_end().to_string();
        let _ = self.tx.send(format!("{}{}\n", self.prompt(), line));
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return;
        }
        if trimmed == "cd" || trimmed.starts_with("cd ") {
            self.cd(trimmed["cd".len()..].trim());
            return;
        }
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let cwd = self.cwd.clone();
        let tx = self.tx.clone();
        let script = format!("{line} 2>&1");
        std::thread::spawn(move || {
            match Command::new(shell)
                .arg("-c")
                .arg(&script)
                .current_dir(&cwd)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(mut child) => {
                    if let Some(mut out) = child.stdout.take() {
                        let mut buf = [0u8; 4096];
                        loop {
                            match out.read(&mut buf) {
                                Ok(0) | Err(_) => break,
                                Ok(n) => {
                                    if tx.send(sanitize(&buf[..n])).is_err() {
                                        return;
                                    }
                                }
                            }
                        }
                    }
                    let _ = child.wait();
                }
                Err(e) => {
                    let _ = tx.send(format!("rusty: {e}\n"));
                }
            }
        });
    }

    fn cd(&mut self, target: &str) {
        let dest = if target.is_empty() || target == "~" {
            dirs::home_dir().unwrap_or_else(|| self.cwd.clone())
        } else if let Some(rest) = target.strip_prefix("~/") {
            dirs::home_dir().map(|h| h.join(rest)).unwrap_or_else(|| self.cwd.clone())
        } else {
            let p = Path::new(target);
            if p.is_absolute() { p.to_path_buf() } else { self.cwd.join(p) }
        };
        match std::fs::canonicalize(&dest) {
            Ok(c) if c.is_dir() => self.cwd = c,
            _ => {
                let _ = self.tx.send(format!("cd: no such directory: {target}\n"));
            }
        }
    }
}

fn pretty(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rest) = path.strip_prefix(&home) {
            return if rest.as_os_str().is_empty() {
                "~".to_string()
            } else {
                format!("~/{}", rest.display())
            };
        }
    }
    path.display().to_string()
}

/// Strip ANSI escapes, expand tabs, drop lone CRs — clean monospace text.
fn sanitize(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(s.len());
    let mut col = 0usize;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.next() {
                Some('[') => {
                    while let Some(&n) = chars.peek() {
                        chars.next();
                        if ('\x40'..='\x7e').contains(&n) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(&n) = chars.peek() {
                        chars.next();
                        if n == '\x07' || n == '\x1b' {
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\r' => {}
            '\x08' => {
                out.pop();
                col = col.saturating_sub(1);
            }
            '\n' => {
                out.push('\n');
                col = 0;
            }
            '\t' => {
                let n = 8 - (col % 8);
                (0..n).for_each(|_| out.push(' '));
                col += n;
            }
            c if !c.is_control() => {
                out.push(c);
                col += 1;
            }
            _ => {}
        }
    }
    out
}
