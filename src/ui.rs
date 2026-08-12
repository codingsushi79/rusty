//! Rendering with ratatui.

use std::collections::HashMap;

use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::app::{App, Focus, Mode};

const BG: Color = Color::Rgb(30, 30, 30);
const SIDEBAR: Color = Color::Rgb(37, 37, 38);
const TAB_INACTIVE: Color = Color::Rgb(45, 45, 45);
const ACCENT: Color = Color::Rgb(0, 122, 204);
const TEXT: Color = Color::Rgb(212, 212, 212);
const WHITE: Color = Color::Rgb(255, 255, 255);
const MUTED: Color = Color::Rgb(133, 133, 133);
const LINE_NR: Color = Color::Rgb(110, 118, 129);
const SELECT: Color = Color::Rgb(38, 79, 120);
const CURRENT_BG: Color = Color::Rgb(40, 40, 42);
const BORDER: Color = Color::Rgb(60, 60, 62);
const GIT_ADD: Color = Color::Rgb(46, 160, 67);
const GIT_MOD: Color = Color::Rgb(0, 122, 204);
const GIT_DEL: Color = Color::Rgb(241, 76, 76);
// Tree decoration colors (VS Code-ish).
const DEC_MOD: Color = Color::Rgb(0xe2, 0xc0, 0x8d);
const DEC_ADD: Color = Color::Rgb(0x73, 0xc9, 0x91);
const DEC_DEL: Color = Color::Rgb(0xc7, 0x4e, 0x39);

fn deco_color(ch: char) -> Color {
    match ch {
        'M' => DEC_MOD,
        'U' | 'A' => DEC_ADD,
        _ => DEC_DEL, // D, C
    }
}
const ERR: Color = Color::Rgb(241, 76, 76);
const WARN: Color = Color::Rgb(204, 167, 0);

/// Map the active file's diagnostics to line → min-severity.
fn diag_map(app: &App) -> HashMap<usize, u8> {
    let mut m = HashMap::new();
    if let Some(uri) = app.buf().and_then(|b| b.path.as_ref()).map(|p| format!("file://{}", p.display())) {
        if let Some(ds) = app.diagnostics.get(&uri) {
            for d in ds {
                m.entry(d.line).and_modify(|s: &mut u8| *s = (*s).min(d.severity)).or_insert(d.severity);
            }
        }
    }
    m
}

pub fn render(f: &mut Frame, app: &mut App) {
    let size = f.area();
    f.render_widget(Block::default().style(Style::default().bg(BG)), size);

    let root = Layout::vertical([Constraint::Min(1), Constraint::Length(1), Constraint::Length(1)]).split(size);
    let (content, status, hints) = (root[0], root[1], root[2]);

    let editor_area = if app.show_tree {
        let cols = Layout::horizontal([Constraint::Length(32), Constraint::Min(10)]).split(content);
        render_tree(f, app, cols[0]);
        cols[1]
    } else {
        app.layout.tree = None;
        content
    };

    if app.term_open {
        let h = (editor_area.height / 3).clamp(6, 16);
        let split = Layout::vertical([Constraint::Min(3), Constraint::Length(h)]).split(editor_area);
        render_editor(f, app, split[0]);
        render_terminal(f, app, split[1]);
    } else if app.ai_open {
        let h = (editor_area.height / 2).clamp(6, 18);
        let split = Layout::vertical([Constraint::Min(3), Constraint::Length(h)]).split(editor_area);
        render_editor(f, app, split[0]);
        render_ai(f, app, split[1]);
    } else {
        render_editor(f, app, editor_area);
    }
    render_status(f, app, status);
    render_hints(f, app, hints);

    if app.focus == Focus::Palette {
        render_palette(f, app, size);
    } else if app.focus == Focus::Search {
        render_search(f, app, size);
    } else if app.focus == Focus::Settings {
        render_settings(f, app, size);
    } else if app.focus == Focus::Branches {
        render_branches(f, app, size);
    }
    if app.completion.is_some() && app.focus == Focus::Editor {
        render_completion(f, app);
    }
}

fn render_branches(f: &mut Frame, app: &App, size: Rect) {
    let w = (size.width / 2).clamp(40, 70);
    let h = (size.height / 2).clamp(8, 20);
    let area = centered(w, h, size);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(" Switch Branch  (Enter: checkout · type a new name + Enter to create · Esc) ")
        .style(Style::default().bg(SIDEBAR));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).split(inner);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("switch to: ", Style::default().fg(ACCENT)),
            Span::styled(format!("{}_", app.branch_query), Style::default().fg(WHITE)),
        ])),
        rows[0],
    );
    f.render_widget(Block::default().style(Style::default().bg(BORDER)), rows[1]);

    let filtered = app.branches_filtered();
    let current = app.branch_list.first().cloned().unwrap_or_default();
    let items: Vec<ListItem> = filtered
        .iter()
        .map(|b| {
            let marker = if *b == current { "● " } else { "  " };
            let color = if *b == current { GIT_ADD } else { TEXT };
            ListItem::new(Line::from(Span::styled(format!("{marker}{b}"), Style::default().fg(color))))
        })
        .collect();
    let mut st = ListState::default();
    if !items.is_empty() {
        st.select(Some(app.branch_sel.min(items.len() - 1)));
    }
    let list = List::new(items).highlight_style(Style::default().bg(SELECT).fg(WHITE));
    f.render_stateful_widget(list, rows[2], &mut st);
}

fn render_settings(f: &mut Frame, app: &App, size: Rect) {
    let w = (size.width * 2 / 3).clamp(52, 92);
    let h = 12;
    let area = centered(w, h, size);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(" Settings   (↑↓ select · ◂ ▸ or Enter to change · Esc: save & close) ")
        .style(Style::default().bg(SIDEBAR));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let on = |b: bool| if b { "On".to_string() } else { "Off".to_string() };
    let rows: [(&str, String, bool); 8] = [
        ("Tab Size", app.settings.tab_size.to_string(), false),
        ("Line Numbers", on(app.settings.line_numbers), false),
        ("Syntax Theme", app.hl.theme_name().to_string(), false),
        ("Vim Mode", on(app.settings.vim_mode), false),
        ("Local AI (opt-in)", on(app.settings.ai_enabled), false),
        ("AI Endpoint", app.settings.ai_endpoint.clone(), true),
        ("AI Model", app.settings.ai_model.clone(), true),
        ("AI API Token", if app.settings.ai_api_key.is_empty() { "not set".into() } else { "set ✓".into() }, true),
    ];
    let items: Vec<ListItem> = rows
        .iter()
        .map(|(label, value, edit)| {
            // Text fields are edited with Enter; the rest cycle with ◂ ▸.
            let value_span = if *edit {
                Span::styled(format!("{value}  ⏎"), Style::default().fg(WHITE).add_modifier(Modifier::BOLD))
            } else {
                Span::styled(format!("◂ {value} ▸"), Style::default().fg(WHITE).add_modifier(Modifier::BOLD))
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {label:<18}"), Style::default().fg(TEXT)),
                value_span,
            ]))
        })
        .collect();
    let mut st = ListState::default();
    st.select(Some(app.settings_sel.min(items.len().saturating_sub(1))));
    let list = List::new(items).highlight_style(Style::default().bg(SELECT));
    f.render_stateful_widget(list, inner, &mut st);
}

fn render_completion(f: &mut Frame, app: &App) {
    let Some(c) = &app.completion else { return };
    let Some(buf) = app.buf() else { return };
    let t = app.layout.text;
    if t.width == 0 {
        return;
    }
    // Anchor just below the cursor.
    let cx = t.x + app.layout.gutter + buf.col.saturating_sub(buf.left) as u16;
    let cy = t.y + buf.row.saturating_sub(buf.top) as u16;

    let rows = (c.items.len().min(8)) as u16;
    let w = c.items.iter().map(|(l, _)| l.chars().count()).max().unwrap_or(10).clamp(12, 48) as u16 + 2;
    let x = cx.min(t.x + t.width.saturating_sub(w));
    let y = if cy + 1 + rows <= t.y + t.height { cy + 1 } else { cy.saturating_sub(rows) };
    let area = Rect { x, y, width: w, height: rows };

    f.render_widget(Clear, area);
    f.render_widget(Block::default().style(Style::default().bg(Color::Rgb(45, 45, 48))), area);
    let items: Vec<ListItem> = c
        .items
        .iter()
        .map(|(label, _)| ListItem::new(Line::from(Span::styled(format!(" {label}"), Style::default().fg(TEXT)))))
        .collect();
    let mut st = ListState::default();
    st.select(Some(c.sel.min(items.len().saturating_sub(1))));
    let list = List::new(items).highlight_style(Style::default().bg(ACCENT).fg(WHITE));
    f.render_stateful_widget(list, area, &mut st);
}

fn render_tree(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Tree;
    let outer = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(SIDEBAR));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(inner);
    let name = app.tree.name().to_uppercase();
    f.render_widget(
        Paragraph::new(Span::styled(
            format!(" {name}"),
            Style::default().fg(if focused { WHITE } else { MUTED }).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(SIDEBAR)),
        rows[0],
    );

    let expanded = &app.tree.expanded;
    let git_map = &app.git_status_map;
    let items: Vec<ListItem> = app
        .tree
        .visible()
        .iter()
        .map(|e| {
            let indent = "  ".repeat(e.depth);
            let icon = if e.is_dir {
                if expanded.contains(&e.path) { "▾ " } else { "▸ " }
            } else {
                "  "
            };
            let deco = if e.is_dir { None } else { git_map.get(&e.path).copied() };
            let name_color = match deco {
                Some(ch) => deco_color(ch),
                None if e.is_dir => TEXT,
                None => MUTED,
            };
            let mut spans = vec![Span::styled(format!("{indent}{icon}{}", e.name), Style::default().fg(name_color))];
            if let Some(ch) = deco {
                spans.push(Span::styled(
                    format!("  {ch}"),
                    Style::default().fg(name_color).add_modifier(Modifier::BOLD),
                ));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items).highlight_style(
        Style::default().bg(if focused { SELECT } else { TAB_INACTIVE }).fg(WHITE),
    );
    app.tree_state.select(Some(app.tree.selected));
    f.render_stateful_widget(list, rows[1], &mut app.tree_state);
    app.layout.tree = Some(rows[1]);
}

fn render_editor(f: &mut Frame, app: &mut App, area: Rect) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);
    app.layout.tabs = rows[0];
    render_tabs(f, app, rows[0]);
    let text_area = rows[1];
    app.layout.text = text_area;

    if app.buf().is_none() {
        render_placeholder(f, text_area);
        return;
    }

    let show_nums = app.settings.line_numbers;
    let total = app.buf().unwrap().lines.len();
    let digits = total.to_string().len().max(3);
    let gutter_w = if show_nums { (digits + 4) as u16 } else { 2 }; // (mark + num) or just mark
    let text_w = text_area.width.saturating_sub(gutter_w) as usize;
    let height = text_area.height as usize;
    app.editor_height = height;
    app.editor_width = text_w;
    app.layout.gutter = gutter_w;
    app.prepare_active(height, text_w);

    let diags = diag_map(app);
    let buf = app.buf().unwrap();
    let selection = buf.selection();
    let mut lines: Vec<Line> = Vec::new();
    let end = (buf.top + height).min(buf.lines.len());
    for r in buf.top..end {
        let is_cur = r == buf.row;
        let gbg = if is_cur { CURRENT_BG } else { BG };

        let (mchar, mcolor) = match buf.marks.get(r).copied().flatten() {
            Some(crate::buffer::GitMark::Added) => ("▎", GIT_ADD),
            Some(crate::buffer::GitMark::Modified) => ("▎", GIT_MOD),
            Some(crate::buffer::GitMark::Deleted) => ("▁", GIT_DEL),
            None => (" ", gbg),
        };
        let mut spans = vec![Span::styled(format!("{mchar} "), Style::default().fg(mcolor).bg(gbg))];
        if show_nums {
            let num = format!("{:>w$}  ", r + 1, w = digits);
            let num_fg = match diags.get(&r).copied() {
                Some(1) => ERR,
                Some(2) => WARN,
                _ if is_cur => WHITE,
                _ => LINE_NR,
            };
            spans.push(Span::styled(num, Style::default().fg(num_fg).bg(gbg)));
        }

        let line_len = buf.lines[r].chars().count();
        let sel_cols = selection.and_then(|((sr, sc), (er, ec))| {
            if r < sr || r > er {
                None
            } else {
                let s = if r == sr { sc } else { 0 };
                let e = if r == er { ec } else { line_len };
                Some((s, e))
            }
        });

        if let Some(hl) = buf.hl.get(r) {
            push_visible(&mut spans, hl, buf.left, text_w, is_cur, sel_cols);
        }
        lines.push(Line::from(spans));
    }

    f.render_widget(Paragraph::new(lines).style(Style::default().bg(BG)), text_area);

    if app.focus == Focus::Editor {
        let cx = text_area.x + gutter_w + (buf.col.saturating_sub(buf.left)) as u16;
        let cy = text_area.y + (buf.row.saturating_sub(buf.top)) as u16;
        f.set_cursor_position(Position::new(cx, cy));
    }
}

fn render_placeholder(f: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled("Rusty", Style::default().fg(WHITE).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(Span::styled("No file open", Style::default().fg(MUTED))),
        Line::from(""),
        Line::from(Span::styled("^P  Open a file", Style::default().fg(ACCENT))),
        Line::from(Span::styled("^N  New file", Style::default().fg(ACCENT))),
        Line::from(Span::styled("^B  Toggle file tree", Style::default().fg(ACCENT))),
    ];
    let inner = centered(40, lines.len() as u16, area);
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
}

/// Push the visible slice of a highlighted line, applying selection/cursor bg.
fn push_visible(
    spans: &mut Vec<Span>,
    hl: &[(Style, String)],
    left: usize,
    width: usize,
    is_cur: bool,
    sel: Option<(usize, usize)>,
) {
    let mut pos = 0usize;
    let mut taken = 0usize;
    let mut acc = String::new();
    let mut acc_style: Option<Style> = None;

    for (style, txt) in hl {
        for ch in txt.chars() {
            if taken >= width {
                break;
            }
            if pos >= left {
                let selected = sel.map_or(false, |(s, e)| pos >= s && pos < e);
                let mut st = *style;
                if selected {
                    st = st.bg(SELECT);
                } else if is_cur {
                    st = st.bg(CURRENT_BG);
                }
                if acc_style != Some(st) {
                    if let Some(prev) = acc_style {
                        spans.push(Span::styled(std::mem::take(&mut acc), prev));
                    }
                    acc_style = Some(st);
                }
                acc.push(ch);
                taken += 1;
            }
            pos += 1;
        }
        if taken >= width {
            break;
        }
    }
    if let Some(st) = acc_style {
        spans.push(Span::styled(acc, st));
    }
}

fn render_tabs(f: &mut Frame, app: &mut App, area: Rect) {
    f.render_widget(Block::default().style(Style::default().bg(SIDEBAR)), area);
    app.layout.tab_spans.clear();
    if app.buffers.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(" No open files", Style::default().fg(MUTED))),
            area,
        );
        return;
    }
    let mut spans = Vec::new();
    let mut x = area.x;
    for (i, b) in app.buffers.iter().enumerate() {
        let active = i == app.active;
        let label = format!(" {}{} ", if b.modified { "● " } else { "" }, b.name());
        let w = label.chars().count() as u16;
        app.layout.tab_spans.push((x, x + w, i));
        x += w + 1; // label + one raw space separator
        let style = if active {
            Style::default().bg(BG).fg(WHITE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(TAB_INACTIVE).fg(MUTED)
        };
        spans.push(Span::styled(label, style));
        spans.push(Span::raw(" "));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_status(f: &mut Frame, app: &App, area: Rect) {
    f.render_widget(Block::default().style(Style::default().bg(ACCENT)), area);
    let branch = app.branch.clone().unwrap_or_else(|| "no repo".to_string());
    let (mut errs, mut warns) = (0usize, 0usize);
    if let Some(uri) = app.buf().and_then(|b| b.path.as_ref()).map(|p| format!("file://{}", p.display())) {
        if let Some(ds) = app.diagnostics.get(&uri) {
            for d in ds {
                match d.severity {
                    1 => errs += 1,
                    2 => warns += 1,
                    _ => {}
                }
            }
        }
    }
    let diag = if errs + warns > 0 { format!("✖ {errs}  ⚠ {warns}   ") } else { String::new() };
    let vim = if app.settings.vim_mode {
        match app.mode {
            Mode::Normal => " NORMAL ",
            Mode::Insert => " INSERT ",
        }
    } else {
        ""
    };
    let (left, right) = match app.buf() {
        Some(buf) => (
            format!("{vim} ⎇ {}   {}{}", branch, buf.name(), if buf.modified { " ●" } else { "" }),
            format!("{}Ln {}, Col {}   {}   UTF-8 ", diag, buf.row + 1, buf.col + 1, buf.language()),
        ),
        None => (format!("{vim} ⎇ {}   —", branch), String::new()),
    };
    let s = Style::default().fg(WHITE).bg(ACCENT);
    f.render_widget(Paragraph::new(left).style(s).alignment(Alignment::Left), area);
    f.render_widget(Paragraph::new(right).style(s).alignment(Alignment::Right), area);
}

fn render_hints(f: &mut Frame, app: &App, area: Rect) {
    f.render_widget(Block::default().style(Style::default().bg(BG)), area);
    let line = match app.focus {
        Focus::Tree => Line::from(Span::styled(
            "  ↑↓/jk move   Enter open/expand   n new file   d delete   Esc editor",
            Style::default().fg(MUTED),
        )),
        Focus::Shell => Line::from(Span::styled(
            "  interactive shell — keys go to the terminal · ^J to return to the editor",
            Style::default().fg(MUTED),
        )),
        Focus::Find => Line::from(vec![
            Span::styled(" Find: ", Style::default().fg(ACCENT)),
            Span::styled(format!("{}_", app.find_query), Style::default().fg(WHITE)),
            Span::styled("   (Enter: next · Esc: close)", Style::default().fg(MUTED)),
        ]),
        Focus::Prompt => {
            let p = app.prompt.as_ref();
            let label = p.map(|p| p.label.clone()).unwrap_or_default();
            let input = p.map(|p| p.input.clone()).unwrap_or_default();
            Line::from(vec![
                Span::styled(format!(" {label} "), Style::default().fg(ACCENT)),
                Span::styled(format!("{input}_"), Style::default().fg(WHITE)),
                Span::styled("   (Enter: confirm · Esc: cancel)", Style::default().fg(MUTED)),
            ])
        }
        _ => {
            // A diagnostic on the cursor line takes over the hint bar.
            let cur_diag = app.buf().and_then(|b| {
                let uri = b.path.as_ref().map(|p| format!("file://{}", p.display()))?;
                let ds = app.diagnostics.get(&uri)?;
                ds.iter().find(|d| d.line == b.row).map(|d| (d.severity, d.message.clone()))
            });
            if let Some((sev, msg)) = cur_diag {
                let (tag, color) = if sev == 1 { ("error", ERR) } else { ("warn ", WARN) };
                Line::from(vec![
                    Span::styled(format!(" {tag}: "), Style::default().fg(color).add_modifier(Modifier::BOLD)),
                    Span::styled(msg, Style::default().fg(TEXT)),
                ])
            } else {
                let keys = "  ^S Save  ^P Palette  ^Space Complete  ^F Find  ^G Search  ^J Term  ^B Files  ^Q Quit";
                Line::from(vec![
                    Span::styled(keys, Style::default().fg(MUTED)),
                    Span::styled(format!("    {}", app.status), Style::default().fg(TEXT)),
                ])
            }
        }
    };
    f.render_widget(Paragraph::new(line), area);
}

fn render_palette(f: &mut Frame, app: &App, size: Rect) {
    let w = (size.width * 3 / 5).clamp(40, 90);
    let h = (size.height * 3 / 5).clamp(8, 22);
    let area = centered(w, h, size);
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(" Command Palette ")
        .style(Style::default().bg(SIDEBAR));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).split(inner);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(ACCENT)),
            Span::styled(format!("{}_", app.palette_query), Style::default().fg(WHITE)),
        ])),
        rows[0],
    );
    f.render_widget(Block::default().style(Style::default().bg(BORDER)), rows[1]);

    let items: Vec<ListItem> = app
        .palette_items()
        .into_iter()
        .map(|(label, _)| ListItem::new(Line::from(Span::styled(format!("  {label}"), Style::default().fg(TEXT)))))
        .collect();
    let mut st = ListState::default();
    if !items.is_empty() {
        st.select(Some(app.palette_sel.min(items.len() - 1)));
    }
    let list = List::new(items).highlight_style(Style::default().bg(SELECT).fg(WHITE));
    f.render_stateful_widget(list, rows[2], &mut st);
}

fn render_ai(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(BORDER))
        .title(Span::styled(
            format!(" LOCAL AI — {} ", app.settings.ai_model),
            Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(Color::Rgb(24, 24, 26)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        Paragraph::new(app.ai_output.clone())
            .style(Style::default().fg(TEXT))
            .wrap(ratatui::widgets::Wrap { trim: false }),
        inner,
    );
}

fn render_terminal(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Shell;
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(BORDER))
        .title(Span::styled(
            " TERMINAL ",
            Style::default().fg(if focused { WHITE } else { MUTED }).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(Color::Rgb(24, 24, 26)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(term) = &mut app.term else { return };
    term.resize(inner.height, inner.width);
    let screen = term.screen();
    let (rows, cols) = screen.size();

    let mut lines: Vec<Line> = Vec::with_capacity(rows as usize);
    for r in 0..rows {
        let mut spans: Vec<Span> = Vec::new();
        let mut run = String::new();
        let mut run_style: Option<Style> = None;
        for c in 0..cols {
            let (ch, style) = match screen.cell(r, c) {
                Some(cell) => {
                    let s = cell.contents();
                    (if s.is_empty() { " ".to_string() } else { s }, cell_style(cell))
                }
                None => (" ".to_string(), Style::default()),
            };
            if run_style != Some(style) {
                if let Some(st) = run_style {
                    spans.push(Span::styled(std::mem::take(&mut run), st));
                }
                run_style = Some(style);
            }
            run.push_str(&ch);
        }
        if let Some(st) = run_style {
            spans.push(Span::styled(run, st));
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines), inner);

    if focused && !screen.hide_cursor() {
        let (cr, cc) = screen.cursor_position();
        f.set_cursor_position(Position::new(inner.x + cc, inner.y + cr));
    }
}

fn vt_color(c: vt100::Color) -> Option<Color> {
    match c {
        vt100::Color::Default => None,
        vt100::Color::Idx(i) => Some(Color::Indexed(i)),
        vt100::Color::Rgb(r, g, b) => Some(Color::Rgb(r, g, b)),
    }
}

fn cell_style(cell: &vt100::Cell) -> Style {
    let mut s = Style::default();
    if let Some(fg) = vt_color(cell.fgcolor()) {
        s = s.fg(fg);
    }
    if let Some(bg) = vt_color(cell.bgcolor()) {
        s = s.bg(bg);
    }
    if cell.bold() {
        s = s.add_modifier(Modifier::BOLD);
    }
    if cell.italic() {
        s = s.add_modifier(Modifier::ITALIC);
    }
    if cell.underline() {
        s = s.add_modifier(Modifier::UNDERLINED);
    }
    if cell.inverse() {
        s = s.add_modifier(Modifier::REVERSED);
    }
    s
}

fn render_search(f: &mut Frame, app: &App, size: Rect) {
    let w = (size.width * 3 / 4).clamp(50, 110);
    let h = (size.height * 3 / 4).clamp(10, 28);
    let area = centered(w, h, size);
    f.render_widget(Clear, area);

    let title = format!(
        " Search & Replace — {} results   (Tab: switch · Ctrl+R: replace all · Esc: close) ",
        app.search_results.len()
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(title)
        .style(Style::default().bg(SIDEBAR));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .split(inner);

    let field = |label: &str, value: &str, active: bool| {
        let lc = if active { WHITE } else { MUTED };
        Line::from(vec![
            Span::styled(format!("{label} "), Style::default().fg(if active { ACCENT } else { MUTED })),
            Span::styled(
                format!("{value}{}", if active { "_" } else { "" }),
                Style::default().fg(lc),
            ),
        ])
    };
    f.render_widget(Paragraph::new(field("search: ", &app.search_query, !app.search_in_replace)), rows[0]);
    f.render_widget(Paragraph::new(field("replace:", &app.search_replace, app.search_in_replace)), rows[1]);
    f.render_widget(Block::default().style(Style::default().bg(BORDER)), rows[2]);

    let root = &app.tree.root;
    let items: Vec<ListItem> = app
        .search_results
        .iter()
        .map(|(path, line, text)| {
            let rel = path.strip_prefix(root).unwrap_or(path);
            ListItem::new(Line::from(vec![
                Span::styled(format!("{}:{}", rel.display(), line + 1), Style::default().fg(ACCENT)),
                Span::styled("  ", Style::default()),
                Span::styled(text.clone(), Style::default().fg(TEXT)),
            ]))
        })
        .collect();
    let mut st = ListState::default();
    if !items.is_empty() {
        st.select(Some(app.search_sel.min(items.len() - 1)));
    }
    let list = List::new(items).highlight_style(Style::default().bg(SELECT).fg(WHITE));
    f.render_stateful_widget(list, rows[3], &mut st);
}

fn centered(w: u16, h: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 3;
    Rect { x, y, width: w.min(area.width), height: h.min(area.height) }
}
