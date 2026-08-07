//! A fully interactive terminal: the user's `$SHELL` in a PTY, with output run
//! through a `vt100` emulator (colors, cursor addressing, alt-screen), so
//! programs like `vim`, `top`, and `less` work. Keystrokes are encoded to the
//! usual terminal byte sequences and written to the PTY.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};

pub struct Terminal {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    _child: Box<dyn portable_pty::Child + Send + Sync>,
    parser: vt100::Parser,
    rx: Receiver<Vec<u8>>,
    rows: u16,
    cols: u16,
}

impl Terminal {
    pub fn spawn(cwd: PathBuf, rows: u16, cols: u16) -> anyhow::Result<Self> {
        let rows = rows.max(1);
        let cols = cols.max(1);
        let pty = native_pty_system();
        let pair = pty.openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let mut cmd = CommandBuilder::new(shell);
        cmd.env("TERM", "xterm-256color");
        cmd.cwd(cwd);
        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        Ok(Self {
            writer,
            master: pair.master,
            _child: child,
            parser: vt100::Parser::new(rows, cols, 0),
            rx,
            rows,
            cols,
        })
    }

    /// Drain PTY output into the emulator. Returns true if anything arrived.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        for chunk in self.rx.try_iter() {
            self.parser.process(&chunk);
            changed = true;
        }
        changed
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        if rows == self.rows && cols == self.cols {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        self.parser.set_size(rows, cols);
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    fn write(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    /// Encode a key event to terminal bytes and send it to the shell.
    pub fn key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let bytes: Vec<u8> = match key.code {
            KeyCode::Char(c) => {
                if ctrl {
                    // Control byte: Ctrl-A..Z -> 1..26, plus a few specials.
                    let b = (c.to_ascii_lowercase() as u8).wrapping_sub(b'a').wrapping_add(1);
                    match c {
                        ' ' => vec![0],
                        'a'..='z' | 'A'..='Z' => vec![b],
                        '[' => vec![0x1b],
                        '\\' => vec![0x1c],
                        ']' => vec![0x1d],
                        _ => c.to_string().into_bytes(),
                    }
                } else {
                    c.to_string().into_bytes()
                }
            }
            KeyCode::Enter => vec![b'\r'],
            KeyCode::Backspace => vec![0x7f],
            KeyCode::Tab => vec![b'\t'],
            KeyCode::BackTab => b"\x1b[Z".to_vec(),
            KeyCode::Esc => vec![0x1b],
            KeyCode::Left => b"\x1b[D".to_vec(),
            KeyCode::Right => b"\x1b[C".to_vec(),
            KeyCode::Up => b"\x1b[A".to_vec(),
            KeyCode::Down => b"\x1b[B".to_vec(),
            KeyCode::Home => b"\x1b[H".to_vec(),
            KeyCode::End => b"\x1b[F".to_vec(),
            KeyCode::PageUp => b"\x1b[5~".to_vec(),
            KeyCode::PageDown => b"\x1b[6~".to_vec(),
            KeyCode::Delete => b"\x1b[3~".to_vec(),
            KeyCode::Insert => b"\x1b[2~".to_vec(),
            KeyCode::F(n) => match n {
                1 => b"\x1bOP".to_vec(),
                2 => b"\x1bOQ".to_vec(),
                3 => b"\x1bOR".to_vec(),
                4 => b"\x1bOS".to_vec(),
                _ => format!("\x1b[{}~", 10 + n).into_bytes(),
            },
            _ => return,
        };
        self.write(&bytes);
    }
}
