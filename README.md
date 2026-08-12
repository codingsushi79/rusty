![Contributors](https://img.shields.io/github/contributors/codingsushi79/rusty?color=blue)
![GitHub commit activity](https://img.shields.io/github/commit-activity/m/codingsushi79/rusty?color=blue)
![GitHub last commit](https://img.shields.io/github/last-commit/codingsushi79/rusty?color=blue)

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

**Nix** (flake, Linux & macOS · x86-64 and ARM):

```bash
nix run github:codingsushi79/rusty            # run without installing
nix profile install github:codingsushi79/rusty
```

Or run from source: `cargo run --release -- .`

Rusty is cross-platform — macOS, Linux, Windows, and \*BSD — since it builds on
`crossterm`/`ratatui`; `cargo install` works on all of them.

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
  **file-tree decorations** (`M`/`A`/`U`/`D` badges with colored names),
  **branch** switch / create / rename / delete / publish, plus **init repo**,
  **add remote**, **stage file / stage all**, **discard changes**, **commit**,
  and **push / pull / fetch** from the palette. Network commands run **in the
  background** (no pane) using your existing git credentials, and report the
  result in the status bar.
- **Extensions** — drop a small TOML manifest in `~/.config/rusty/extensions/`
  or `<project>/.rusty/extensions/` to add an `Ext: <name>` palette command that
  runs a tool with `RUSTY_FILE` / `RUSTY_LINE` / `RUSTY_COL` / `RUSTY_ROOT` in
  the environment (in the background or the terminal pane).
- **Local AI (opt-in)** — off by default, no network unless you enable it.
  **Ask** / **Explain Selection** against a **local** model (Ollama / llama.cpp)
  *or* any OpenAI-compatible cloud provider by **bringing your own token**
  (`AI: Set API Token`). Replies **stream** token-by-token into a bottom panel.
- **Vim mode (opt-in)** — modal editing with Normal/Insert modes and motions:
  `h j k l`, `w b`, `0 $ ^`, `gg G`, `x`, `dd`, `D`, `yy`, `p`, `u`,
  `i a A I o O`, and `:` / `/` to jump to the palette / find. Toggle in Settings
  (`Ctrl+,`) or `Editor: Toggle Vim Mode`; the mode shows in the status bar.
- **Go to Line** (`Go to Line…` in the palette).
- **Settings** (`Ctrl+,`) — tab size, line numbers, syntax theme; saved to
  `~/.config/rusty/config.toml` (or the platform equivalent).
- **Self-update** — `Rusty: Update` in the palette reinstalls the latest build.
- **Mouse** — click to place the cursor, open files, and switch tabs; drag to
  select; wheel to scroll.
- **Status bar** — git branch, file, diagnostics, Ln/Col, language; **hint bar**
  that auto-fades after a few seconds.
- **Live file tree** — files added/removed outside the editor appear automatically.
- **Logging** — errors and panics are written to a log file; open it with
  `Rusty: Open Log` in the palette.

## Language servers

LSP auto-starts if the server binary is on your `PATH`:

| Language | Server | Install |
| --- | --- | --- |
| Rust | `rust-analyzer` | `rustup component add rust-analyzer` |
| Python | `pylsp` | `pipx install python-lsp-server` |
| TS/JS | `typescript-language-server` | `npm i -g typescript-language-server typescript` |
| Go | `gopls` | `go install golang.org/x/tools/gopls@latest` |

## Extensions

Create `~/.config/rusty/extensions/<name>.toml` (or `<project>/.rusty/extensions/<name>.toml`):

```toml
name = "Format (rustfmt)"
command = "rustfmt \"$RUSTY_FILE\""
terminal = false   # true = run in the terminal pane instead of the background
```

It shows up as `Ext: Format (rustfmt)` in the command palette.

### WASM plugins

An extension can instead point at a sandboxed WebAssembly module:

```toml
name = "My Plugin"
wasm = "plugin.wasm"   # path relative to the manifest
```

The plugin exports `run()` and may import host functions (module `rusty`) —
`status`, `log`, `insert`, `line`, `col`, `file_len`, `file_read` — to read the
current file and change the editor. Effects are applied after `run()` returns,
so plugins never touch editor state directly (run via `wasmi`, no JIT).

## AI

Off by default. Enable via `AI: Enable Local AI` in the palette (or Settings).
Works with a local server or any OpenAI-compatible API:

- **Local:** run `ollama serve`; default endpoint `http://localhost:11434/v1`.
- **Cloud (BYOT):** `AI: Set Endpoint…` (e.g. `https://api.openai.com/v1`) and
  `AI: Set API Token…`, then `AI: Set Model…`.

## Roadmap

| Feature | Notes |
| --- | --- |
| **Vim counts & registers** | `3j`, named registers, more operators |
| **Richer WASM host API** | text ranges, selections, callbacks |
| **Debugger (DAP)** | breakpoints, stepping |

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
  ai.rs          opt-in AI client (OpenAI-compatible; local or BYOT)
  extensions.rs  external-tool extensions (TOML manifests)
  wasmext.rs     sandboxed WASM plugin host (wasmi)
  logging.rs     file logging (errors/panics)
```

## License

[MIT](./LICENSE-MIT) OR [Apache-2.0](./LICENSE-Apache-2.0).
