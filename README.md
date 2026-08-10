![GitHub top language](https://img.shields.io/github/languages/top/codingsushi79/rusty)
![GitHub commit activity](https://img.shields.io/github/commit-activity/m/codingsushi79/rusty)
![GitHub last commit](https://img.shields.io/github/last-commit/codingsushi79/rusty)

# Rusty

A fast, friendly **terminal** code editor written in Rust. Run `rusty <dir>` in
any terminal and get a full-screen TUI — file tree, tabs, syntax highlighting,
line numbers, a fuzzy command palette, and find — that's modeless and
nano-easy (the shortcuts are always shown at the bottom).

Built on [`ratatui`](https://ratatui.rs) + `crossterm`, with `syntect`
highlighting. Single ~2 MB binary, no runtime dependencies.

## Install (one line)

With a Rust toolchain (`rustup`), this builds and installs the `rusty` command
to `~/.cargo/bin` on macOS, Linux, and Windows:

```bash
cargo install --git https://github.com/codingsushi79/rusty
```

Update later with the same command plus `--force`, or run **`Rusty: Update`**
from the command palette (`Ctrl+P`).

No Rust yet? The bootstrap scripts install it for you:

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/codingsushi79/rusty/master/scripts/install.sh | sh
# Windows (PowerShell)
irm https://raw.githubusercontent.com/codingsushi79/rusty/master/scripts/install.ps1 | iex
```

Or run from source: `cargo run --release -- .`

## Usage

```bash
rusty            # open the current directory
rusty ./my-proj  # open a folder
```

| Key | Action |
| --- | --- |
| `Ctrl+P` | command palette — fuzzy-open files & run commands |
| `Ctrl+G` | search across files (Tab → replace field, `Ctrl+R` = replace all) |
| `Ctrl+J` | toggle the interactive terminal pane (and return from it) |
| `Ctrl+F` | find in file (Enter = next, Esc = close) |
| `Ctrl+Space` | LSP completion (↑/↓ select, Enter/Tab accept, Esc dismiss) |
| `F12` | go to definition · `Ctrl+K` hover type info |
| `Ctrl+N` | new file — pick a spot in the tree, type `name.ext` |
| `Ctrl+,` | settings screen |
| `Ctrl+S` | save |
| mouse | click to place cursor / open files / switch tabs · drag to select · wheel to scroll |
| `Ctrl+Z` / `Ctrl+Y` | undo / redo |
| `Ctrl+C` / `Ctrl+X` / `Ctrl+V` | copy / cut / paste (system clipboard) |
| `Shift`+arrows | select text |
| `Ctrl+B` | focus the file tree (again to hide it) |
| `Ctrl+W` | close the current buffer |
| `Ctrl+Q` | quit |
| arrows / PgUp / PgDn / Home / End | move around |
| in the tree | `↑/↓` or `j/k` move, `Enter`/`→` open or expand, `←` collapse, `n` new file here, `d` delete (with confirm), `Esc` back to editor |

## What works today

- **File tree** — collapsible directories, keyboard navigation, **create files**
  in place (`Ctrl+N` / `n`, type `name.ext`), and **delete** files/folders
  (`d`, with a confirm).
- **Editing** — insert/delete, newline, tab, cursor movement, page scrolling,
  horizontal scroll, **undo/redo**, **selection** (Shift+arrows), and
  **copy/cut/paste** via the system clipboard.
- **Line numbers** with the current line highlighted.
- **Syntax highlighting** (truecolor) via `syntect`.
- **Tabs / multiple buffers**, dirty (`●`) indicators; an empty-state welcome
  screen when nothing is open (closing the last tab doesn't force an untitled).
- **Command palette** (`Ctrl+P`) — fuzzy over commands + every file.
- **Find in file** (`Ctrl+F`) and **workspace search & replace** (`Ctrl+G`) —
  live results across files; `Ctrl+R` replaces all occurrences on disk.
- **Integrated terminal** (`Ctrl+J`) — a **fully interactive PTY** running your
  `$SHELL` in a bottom pane, with colors and cursor addressing, so `vim`, `top`,
  `less`, etc. work. While focused, keys go to the shell; `Ctrl+J` returns to
  the editor.
- **LSP** — diagnostics (in the gutter, status counts, and the hint bar),
  completion (`Ctrl+Space`), **go-to-definition** (`F12`), and **hover** type
  info (`Ctrl+K`) — also in the palette. Auto-starts a server for the file's
  language if one is on your `PATH`.
- **Git** — a **diff gutter** (added / modified / removed markers vs HEAD),
  **file-tree decorations** (`M`/`A`/`U`/`D` badges with colored names), plus
  **init repo**, **add remote**, **stage file / stage all**, **discard changes**,
  **commit**, and **push / pull / fetch** from the palette. Network commands run
  **in the background** (no pane) using your existing git credentials, and report
  the result in the status bar.
- **Settings** (`Ctrl+,`) — tab size, line numbers, syntax theme; saved to
  `~/.config/rusty/config.toml` (or the platform equivalent).
- **Self-update** — `Rusty: Update` in the palette reinstalls the latest build.
- **Mouse** — click to place the cursor, open files, and switch tabs; drag to
  select; wheel to scroll.
- **Status bar** — git branch, file, diagnostics, Ln/Col, language; **hint bar**.

## Language servers

LSP auto-starts if the server binary is on your `PATH`:

| Language | Server | Install |
| --- | --- | --- |
| Rust | `rust-analyzer` | `rustup component add rust-analyzer` |
| Python | `pylsp` | `pipx install python-lsp-server` |
| TS/JS | `typescript-language-server` | `npm i -g typescript-language-server typescript` |
| Go | `gopls` | `go install golang.org/x/tools/gopls@latest` |

## Roadmap

| Feature | Notes |
| --- | --- |
| **LSP go-to-definition / hover** | jump to symbols, show types on demand |
| **Extensions & local AI** | scripting hooks; opt-in local model, no telemetry |
| **Config & themes** | `~/.config/rusty/config.toml`, selectable themes |

## Architecture

```
src/
  main.rs        terminal setup, event loop, teardown
  app.rs         state, input handling, focus, fuzzy palette, find
  ui.rs          ratatui rendering (tree, editor, tabs, status, palette)
  buffer.rs      editable text buffer (cursor, scroll, highlight cache)
  highlight.rs   syntect → ratatui styled spans
  tree.rs        workspace file tree
  config.rs      settings (tab size, line numbers, theme) load/save
  git.rs         branch, HEAD blob (diff gutter), init, remote, stage, commit
  lsp.rs         JSON-RPC language-server client (diagnostics, completion)
  term.rs        interactive PTY terminal (vt100 emulator + key encoding)
```

## License

[MIT](./LICENSE-MIT) OR [Apache-2.0](./LICENSE-Apache-2.0).
