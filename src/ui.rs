//! Rendering with ratatui.

use std::collections::HashMap;

use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::app::{App, Focus};

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

    if app.shell_open {
        let h = (editor_area.height / 3).clamp(6, 14);
        let split = Layout::vertical([Constraint::Min(3), Constraint::Length(h)]).split(editor_area);
        render_editor(f, app, split[0]);
        render_shell(f, app, split[1]);
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
    }
    if app.completion.is_some() && app.focus == Focus::Editor {
        render_completion(f, app);
    }
}

fn render_settings(f: &mut Frame, app: &App, size: Rect) {
    let w = (size.width * 3 / 5).clamp(48, 80);
    let h = 9;
    let area = centered(w, h, size);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(" Settings   (↑↓ select · ◂ ▸ change · Esc: save & close) ")
        .style(Style::default().bg(SIDEBAR));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = [
        ("Tab Size", app.settings.tab_size.to_string()),
        ("Line Numbers", if app.settings.line_numbers { "On".into() } else { "Off".into() }),
        ("Syntax Theme", app.hl.theme_name().to_string()),
    ];
    let items: Vec<ListItem> = rows
        .iter()
        .map(|(label, value)| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {label:<16}"), Style::default().fg(TEXT)),
                Span::styled(format!("◂ {value} ▸"), Style::default().fg(WHITE).add_modifier(Modifier::BOLD)),
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
            let style = if e.is_dir { Style::default().fg(TEXT) } else { Style::default().fg(MUTED) };
            ListItem::new(Line::from(Span::styled(format!("{indent}{icon}{}", e.name), style)))
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
    let (left, right) = match app.buf() {
        Some(buf) => (
            format!(" ⎇ {}   {}{}", branch, buf.name(), if buf.modified { " ●" } else { "" }),
            format!("{}Ln {}, Col {}   {}   UTF-8 ", diag, buf.row + 1, buf.col + 1, buf.language()),
        ),
        None => (format!(" ⎇ {}   —", branch), String::new()),
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
            "  Enter run   'clear' to clear   Esc editor   ^J hide",
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

fn render_shell(f: &mut Frame, app: &App, area: Rect) {
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

    let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);

    // Output: show the tail that fits.
    let h = rows[0].height as usize;
    let all: Vec<&str> = app.shell_output.lines().collect();
    let start = all.len().saturating_sub(h);
    let out: Vec<Line> = all[start..]
        .iter()
        .map(|l| Line::from(Span::styled((*l).to_string(), Style::default().fg(TEXT))))
        .collect();
    f.render_widget(Paragraph::new(out), rows[0]);

    // Input line.
    let prompt = app.shell.as_ref().map(|s| s.prompt()).unwrap_or_else(|| "$ ".to_string());
    let cursor = if focused { "_" } else { "" };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(prompt, Style::default().fg(GIT_ADD)),
            Span::styled(format!("{}{cursor}", app.shell_input), Style::default().fg(WHITE)),
        ])),
        rows[1],
    );
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
