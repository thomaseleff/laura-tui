//! Editor panes (experimental): Neovim running in a pane over a file panel. The panel keeps the
//! file view and its threads; this module owns only the Neovim side.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{Sender, channel};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use portable_pty::CommandBuilder;
use serde_json::json;

use crate::panel::{Author, Panel};
use crate::protocol::PaneId;
use crate::pty::PtyTab;

/// Shows threads as diagnostics (`laura_threads(json)`, called over `--listen`) and keeps the
/// unsaved-edits marker.
const BOOTSTRAP: &str = include_str!("editor.lua");

/// Laura's theme, loaded after `BOOTSTRAP` when the user said `y` to the startup wizard. Runs
/// after `theme_lua()`'s color table.
const THEME: &str = include_str!("editor_theme.lua");

/// Vim highlight group → the TextMate scope whose Nord color it takes, so both views share one
/// palette (`assets/nord.tmTheme`). `source` resolves to the default foreground.
const GROUPS: &[(&str, &str)] = &[
    ("Normal", "source"),
    ("Comment", "comment"),
    ("String", "string"),
    ("Character", "constant.character"),
    ("Number", "constant.numeric"),
    ("Float", "constant.numeric"),
    // The theme has no bare `constant` rule.
    ("Constant", "constant.other"),
    ("Boolean", "constant.language"),
    ("Operator", "keyword.operator"),
    ("Keyword", "keyword"),
    ("Statement", "keyword"),
    ("PreProc", "keyword"),
    ("Conditional", "keyword.control"),
    ("Repeat", "keyword.control"),
    ("Exception", "keyword.control"),
    ("Include", "keyword.control"),
    ("StorageClass", "storage.modifier"),
    ("Structure", "storage.type"),
    ("Function", "entity.name.function"),
    ("Type", "entity.name.type"),
    // syntect scopes Rust's primitives (`str`, `bool`) as `storage.type`.
    ("rustType", "storage.type"),
    ("Tag", "entity.name.tag"),
    ("Identifier", "source"),
    ("Special", "source"),
    ("Delimiter", "source"),
    // The theme has no `support.macro` rule: `format!` stays the default foreground.
    ("Macro", "source"),
];

/// `THEME` behind a `C` table of each `GROUPS` color.
fn theme_lua() -> String {
    let colors: String = GROUPS
        .iter()
        .map(|(g, scope)| format!("  {g} = '{}',\n", crate::render::nord_hex(scope)))
        .collect();
    format!("local C = {{\n{colors}}}\n{THEME}")
}

/// Refusal when `LAURA_EXPERIMENTAL_EDITOR` isn't set.
pub const FLAG_OFF: &str =
    "editor panes are experimental: start Laura with LAURA_EXPERIMENTAL_EDITOR=1";

/// Refusal when the flag is set but `nvim` isn't on `PATH`.
pub const NO_NVIM: &str =
    "editor panes need Neovim: nvim isn't on PATH (just installed it? restart your terminal)";

/// Refusal for a highlight or diff on an editor pane: the editor view would show neither.
pub const SHOWS_NEITHER: &str =
    "editor panes show no highlight or diff — annotate lines with `laura comment` instead";

/// The startup gate: `nvim`'s path, or why editor panes are off. Resolved once by the TUI.
pub fn resolve() -> Result<PathBuf, String> {
    let on = std::env::var("LAURA_EXPERIMENTAL_EDITOR").is_ok_and(|v| !v.is_empty() && v != "0");
    if !on {
        return Err(FLAG_OFF.into());
    }
    let exe = if cfg!(windows) { "nvim.exe" } else { "nvim" };
    std::env::var_os("PATH")
        .and_then(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join(exe))
                .find(|f| f.is_file())
        })
        .ok_or_else(|| NO_NVIM.into())
}

/// One editor pane's Neovim. Named `Nvim`, not `Editor`: the TUI's draft editor has that name.
pub struct Nvim {
    pub pty: PtyTab,
    /// `true`: the pane shows Neovim; `false`: its file view (the panel), flipped by `Ctrl+L`.
    pub editor_view: bool,
    size: (u16, u16),
    /// The last payload handed to the pusher, so an unchanged panel isn't pushed again.
    last: String,
    tx: Sender<String>,
    /// Bootstrap files written at launch, removed on drop.
    files: Vec<PathBuf>,
    /// Exists while Neovim has unsaved edits; `editor.lua` keeps it in sync.
    dirty: PathBuf,
}

impl Nvim {
    /// Spawn `nvim <path>` in a `rows`x`cols` PTY, with `LAURA_TAB` so `:!laura …` addresses its
    /// tab, listening on a per-pane address for `push`. `theme` adds Laura's theme.
    pub fn spawn(
        nvim: &Path,
        path: &str,
        socket: &str,
        pane: PaneId,
        theme: bool,
        rows: u16,
        cols: u16,
    ) -> Result<Nvim> {
        let name = format!("{socket}-{pane}");
        let addr = if cfg!(windows) {
            format!(r"\\.\pipe\{name}")
        } else {
            std::env::temp_dir().join(&name).display().to_string()
        };
        // A file, not an inline `--cmd`: the tests' fake `nvim.cmd` cuts a multi-line argument short.
        let mut files = Vec::new();
        for (suffix, body) in [
            ("", Some(BOOTSTRAP.to_string())),
            ("-theme", theme.then(theme_lua)),
        ] {
            let Some(body) = body else { continue };
            let f = std::env::temp_dir().join(format!("{name}{suffix}.lua"));
            std::fs::write(&f, body).with_context(|| format!("couldn't write {}", f.display()))?;
            files.push(f);
        }
        let dirty = std::env::temp_dir().join(format!("{name}.dirty"));
        let mut cmd = CommandBuilder::new(nvim);
        cmd.env("LAURA_TAB", socket);
        cmd.env("LAURA_DIRTY", &dirty);
        cmd.args(["--listen", &addr]);
        for f in &files {
            cmd.args(["--cmd", &dofile(f)]);
        }
        cmd.arg(path);
        if let Ok(cwd) = std::env::current_dir() {
            cmd.cwd(cwd);
        }
        let pty = PtyTab::spawn(cmd, rows, cols)
            .inspect_err(|_| {
                for f in &files {
                    let _ = std::fs::remove_file(f);
                }
            })
            .with_context(|| format!("couldn't start {}", nvim.display()))?;
        Ok(Nvim {
            pty,
            editor_view: true,
            size: (rows, cols),
            last: String::new(),
            tx: pusher(nvim.to_path_buf(), addr),
            files,
            dirty,
        })
    }

    /// Whether Neovim has unsaved edits: a stat of its marker file, no round-trip.
    pub fn modified(&self) -> bool {
        self.dirty.exists()
    }

    /// Queue a push of `panel`'s threads, if they changed since the last push.
    pub fn push(&mut self, panel: &Panel) {
        let threads: Vec<_> = panel
            .threads
            .iter()
            .map(|t| {
                let notes: Vec<_> = t
                    .notes()
                    .map(|n| {
                        let author = match &n.author {
                            Author::User => "You",
                            Author::Agent(a) => a.as_str(),
                        };
                        json!({"author": author, "body": n.body})
                    })
                    .collect();
                let (a, b) = panel.thread_source(t);
                // The key is the thread's rendered row: stable, since threads don't move in the panel.
                json!({"key": t.line, "start": a + 1, "end": b + 1, "notes": notes})
            })
            .collect();
        let payload = json!({"path": panel.path, "threads": threads}).to_string();
        if payload != self.last {
            // A Vimscript single-quoted string: `'` doubles.
            let _ = self.tx.send(format!(
                "v:lua.laura_threads('{}')",
                payload.replace('\'', "''")
            ));
            self.last = payload;
        }
    }

    /// Resize only when the pane's inner rect changed, like `Tab::resize_to`.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        if (rows, cols) != self.size {
            self.size = (rows, cols);
            self.pty.resize(rows, cols);
        }
    }
}

impl Drop for Nvim {
    fn drop(&mut self) {
        for f in self.files.iter().chain([&self.dirty]) {
            let _ = std::fs::remove_file(f);
        }
    }
}

/// `--cmd` running a Lua file. `dofile` over `luafile`: a long-bracket string needs no escaping
/// for spaces, `%` or `#` in the path, where `:luafile` would expand them.
fn dofile(f: &Path) -> String {
    format!("lua dofile([==[{}]==])", f.display())
}

/// One pusher thread per editor pane: drains to the newest expression and runs it with
/// `--remote-expr`, retrying while Neovim's server starts. Ends when the `Nvim` drops.
/// ponytail: one `nvim` process per push, and Windows drops a push over ~32 KB (its command-line
/// cap); msgpack-RPC on the pipe if either bites.
fn pusher(nvim: PathBuf, addr: String) -> Sender<String> {
    let (tx, rx) = channel::<String>();
    std::thread::spawn(move || {
        while let Ok(mut expr) = rx.recv() {
            while let Ok(newer) = rx.try_recv() {
                expr = newer;
            }
            let t0 = Instant::now();
            while t0.elapsed() < Duration::from_secs(5) {
                let mut c = Command::new(&nvim);
                c.args(["--headless", "--server", &addr, "--remote-expr", &expr]);
                #[cfg(windows)]
                std::os::windows::process::CommandExt::creation_flags(&mut c, 0x0800_0000); // CREATE_NO_WINDOW
                if c.output().is_ok_and(|o| o.status.success()) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });
    tx
}
