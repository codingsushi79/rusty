//! WASM extension host — runs sandboxed `.wasm` plugins via `wasmi`.
//!
//! A plugin exports `run()` (and a `memory`), and may import these host
//! functions (module `"rusty"`) to interact with the editor:
//!
//! ```text
//! rusty.status(ptr, len)          set the status bar to a UTF-8 string
//! rusty.log(ptr, len)             append to the log file
//! rusty.insert(ptr, len)          insert text at the cursor
//! rusty.line() -> i32             1-indexed cursor line
//! rusty.col()  -> i32             1-indexed cursor column
//! rusty.file_len() -> i32         byte length of the current file
//! rusty.file_read(ptr, cap) -> i32  copy up to `cap` file bytes into wasm mem
//! ```
//!
//! Host functions collect their effects in [`Host`]; the app applies them after
//! `run()` returns, so plugins never touch editor state directly.

use std::path::Path;

use wasmi::{Caller, Engine, Extern, Linker, Memory, Module, Store};

#[derive(Default)]
pub struct Host {
    pub file_text: String,
    pub line: i32,
    pub col: i32,
    pub status: Option<String>,
    pub inserts: Vec<String>,
    pub logs: Vec<String>,
}

fn memory(caller: &mut Caller<'_, Host>) -> Option<Memory> {
    match caller.get_export("memory") {
        Some(Extern::Memory(m)) => Some(m),
        _ => None,
    }
}

fn read_string(caller: &mut Caller<'_, Host>, ptr: i32, len: i32) -> String {
    let Some(mem) = memory(caller) else { return String::new() };
    let data = mem.data(&*caller);
    let (start, end) = (ptr as usize, ptr as usize + len as usize);
    match data.get(start..end) {
        Some(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        None => String::new(),
    }
}

/// Load and run a WASM plugin against `file_text`/cursor, returning its effects.
pub fn run(wasm_path: &Path, file_text: String, line: i32, col: i32) -> anyhow::Result<Host> {
    let bytes = std::fs::read(wasm_path)?;
    let engine = Engine::default();
    let module = Module::new(&engine, &bytes[..])?;
    let host = Host { file_text, line, col, ..Default::default() };
    let mut store = Store::new(&engine, host);
    let mut linker: Linker<Host> = Linker::new(&engine);

    linker.func_wrap("rusty", "status", |mut caller: Caller<'_, Host>, ptr: i32, len: i32| {
        let s = read_string(&mut caller, ptr, len);
        caller.data_mut().status = Some(s);
    })?;
    linker.func_wrap("rusty", "log", |mut caller: Caller<'_, Host>, ptr: i32, len: i32| {
        let s = read_string(&mut caller, ptr, len);
        caller.data_mut().logs.push(s);
    })?;
    linker.func_wrap("rusty", "insert", |mut caller: Caller<'_, Host>, ptr: i32, len: i32| {
        let s = read_string(&mut caller, ptr, len);
        caller.data_mut().inserts.push(s);
    })?;
    linker.func_wrap("rusty", "line", |caller: Caller<'_, Host>| caller.data().line)?;
    linker.func_wrap("rusty", "col", |caller: Caller<'_, Host>| caller.data().col)?;
    linker.func_wrap("rusty", "file_len", |caller: Caller<'_, Host>| {
        caller.data().file_text.len() as i32
    })?;
    linker.func_wrap(
        "rusty",
        "file_read",
        |mut caller: Caller<'_, Host>, ptr: i32, cap: i32| -> i32 {
            let Some(mem) = memory(&mut caller) else { return 0 };
            let text = caller.data().file_text.clone();
            let n = (cap as usize).min(text.len());
            if mem.write(&mut caller, ptr as usize, &text.as_bytes()[..n]).is_err() {
                return 0;
            }
            n as i32
        },
    )?;

    let instance = linker.instantiate(&mut store, &module)?.start(&mut store)?;
    let run = instance.get_typed_func::<(), ()>(&store, "run")?;
    run.call(&mut store, ())?;
    Ok(store.into_data())
}
