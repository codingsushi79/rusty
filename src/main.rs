//! Rusty — a fast, friendly terminal code editor.
//!
//! Usage: `rusty [path]` — opens a folder (or the current directory).

mod app;
mod buffer;
mod config;
mod git;
mod highlight;
mod lsp;
mod term;
mod tree;
mod ui;

use std::io::stdout;
use std::path::PathBuf;

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind,
};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use app::App;

fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());
    let root = PathBuf::from(dir);
    if !root.is_dir() {
        eprintln!("rusty: {} is not a directory", root.display());
        std::process::exit(1);
    }
    let mut app = App::new(&root)?;

    // Restore the terminal even if we panic.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen, DisableMouseCapture);
        default_hook(info);
    }));

    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let result = run(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    result
}

fn run<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> anyhow::Result<()> {
    use std::time::Duration;
    terminal.draw(|f| ui::render(f, app))?;
    while !app.quit {
        let mut dirty = false;
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    app.on_key(key);
                    dirty = true;
                }
                Event::Mouse(m) => {
                    app.on_mouse(m);
                    dirty = true;
                }
                Event::Resize(_, _) => dirty = true,
                _ => {}
            }
        }
        app.lsp_sync();
        if app.lsp_poll() {
            dirty = true;
        }
        if app.term_poll() {
            dirty = true;
        }
        if app.tick() {
            dirty = true;
        }
        if dirty {
            terminal.draw(|f| ui::render(f, app))?;
        }
    }
    Ok(())
}
