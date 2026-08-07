//! Syntax highlighting via `syntect`, producing ratatui-styled spans per line.

use ratatui::style::{Color, Style};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

pub struct Highlighter {
    syntaxes: SyntaxSet,
    themes: ThemeSet,
    theme: String,
}

/// A highlighted line: runs of (style, text).
pub type HlLine = Vec<(Style, String)>;

impl Highlighter {
    pub fn new() -> Self {
        let themes = ThemeSet::load_defaults();
        let theme = if themes.themes.contains_key("base16-ocean.dark") {
            "base16-ocean.dark".to_string()
        } else {
            themes.themes.keys().next().cloned().unwrap_or_default()
        };
        Self { syntaxes: SyntaxSet::load_defaults_newlines(), themes, theme }
    }

    /// Available theme names, sorted.
    pub fn theme_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.themes.themes.keys().cloned().collect();
        v.sort();
        v
    }

    pub fn theme_name(&self) -> &str {
        &self.theme
    }

    /// Switch theme; returns false if the name is unknown.
    pub fn set_theme(&mut self, name: &str) -> bool {
        if self.themes.themes.contains_key(name) {
            self.theme = name.to_string();
            true
        } else {
            false
        }
    }

    /// Highlight an entire file into per-line styled runs.
    pub fn highlight(&self, text: &str, extension: &str) -> Vec<HlLine> {
        let syntax = self
            .syntaxes
            .find_syntax_by_extension(extension)
            .unwrap_or_else(|| self.syntaxes.find_syntax_plain_text());
        let theme = &self.themes.themes[&self.theme];
        let mut h = HighlightLines::new(syntax, theme);

        let mut out = Vec::new();
        for line in LinesWithEndings::from(text) {
            let ranges = h.highlight_line(line, &self.syntaxes).unwrap_or_default();
            let mut spans: HlLine = Vec::new();
            for (style, piece) in ranges {
                let piece = piece.trim_end_matches('\n').trim_end_matches('\r');
                if piece.is_empty() {
                    continue;
                }
                let c = style.foreground;
                spans.push((Style::default().fg(Color::Rgb(c.r, c.g, c.b)), piece.to_string()));
            }
            out.push(spans);
        }
        if out.is_empty() {
            out.push(Vec::new());
        }
        out
    }
}
