//! The draw loop and its widgets: hosts the workspace, renders panes, and routes input to the engine.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Result;
use portable_pty::CommandBuilder;
use ratatui::backend::Backend;
use ratatui::crossterm::event::{
    self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::layout::{Constraint, Flex, Layout, Margin, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::{Frame, Terminal};
use tui_term::widget::PseudoTerminal;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use laura::editor::Nvim;
use laura::protocol::PTY_PANE;
use laura::tab::{SHELL_EXITED, SUBMIT_FAILED};
use laura::view::{self, Hint};
use laura::{PaneId, Panel, PtyTab, Tab, UserAction};

use crate::keys::key_to_bytes;
use crate::mouse::{self, Selection};

/// Windows delivers a paste as a burst of key events (no `Event::Paste`). Given the first key of a
/// suspected burst, drain everything already queued and fold it into one paste. A real paste is text
/// only, so interleaved Release/Focus/Resize records are skipped; a single character means it was just
/// this key plus its Release echo, not a paste, so the original key flows through unchanged.
#[cfg(windows)]
fn coalesce_paste_burst(first: ratatui::crossterm::event::KeyEvent) -> Result<Event> {
    fn push(s: &mut String, code: KeyCode) {
        match code {
            KeyCode::Char(c) => s.push(c),
            KeyCode::Enter => s.push('\n'),
            _ => {}
        }
    }
    let mut s = String::new();
    push(&mut s, first.code);
    while event::poll(Duration::ZERO)? {
        if let Event::Key(k) = event::read()?
            && k.kind == KeyEventKind::Press
        {
            push(&mut s, k.code);
        }
    }
    Ok(if s.chars().count() >= 2 {
        Event::Paste(s)
    } else {
        Event::Key(first)
    })
}

/// A draft's text plus a cursor. `cur` is a byte offset, always on a char boundary.
#[derive(Default)]
struct Editor {
    text: String,
    cur: usize,
}

impl Editor {
    /// Seeded text with the cursor at the end, ready to append.
    fn new(text: String) -> Self {
        Editor {
            cur: text.len(),
            text,
        }
    }

    fn insert(&mut self, s: &str) {
        self.text.insert_str(self.cur, s);
        self.cur += s.len();
    }

    fn left(&mut self) {
        if let Some(c) = self.text[..self.cur].chars().next_back() {
            self.cur -= c.len_utf8();
        }
    }

    fn right(&mut self) {
        if let Some(c) = self.text[self.cur..].chars().next() {
            self.cur += c.len_utf8();
        }
    }

    fn backspace(&mut self) {
        let end = self.cur;
        self.left();
        self.text.replace_range(self.cur..end, "");
    }

    fn delete(&mut self) {
        if let Some(c) = self.text[self.cur..].chars().next() {
            self.text
                .replace_range(self.cur..self.cur + c.len_utf8(), "");
        }
    }

    fn home(&mut self) {
        self.cur = self.text[..self.cur].rfind('\n').map_or(0, |i| i + 1);
    }

    fn end(&mut self) {
        self.cur += self.text[self.cur..]
            .find('\n')
            .unwrap_or(self.text.len() - self.cur);
    }

    /// `\` right before the cursor becomes a newline; false (untouched) otherwise.
    fn continue_line(&mut self) -> bool {
        if !self.text[..self.cur].ends_with('\\') {
            return false;
        }
        self.text.replace_range(self.cur - 1..self.cur, "\n");
        true
    }

    /// The cursor's (row, col) in cells, wrapped to `w`. `col == w` only at the end of a full line.
    fn cursor_rc(&self, w: usize) -> (usize, usize) {
        let before = &self.text[..self.cur];
        let (prev, line) = before.rsplit_once('\n').unwrap_or(("", before));
        let prior: usize = if before.contains('\n') {
            prev.split('\n').map(|l| wrap_rows(l, w).len()).sum()
        } else {
            0
        };
        let rows = wrap_rows(line, w);
        let (r, c) = (prior + rows.len() - 1, rows.last().map_or(0, |s| s.width()));
        match self.text[self.cur..].chars().next() {
            Some(ch) if ch != '\n' && c > 0 && c + ch.width().unwrap_or(0) > w => (r + 1, 0),
            _ => (r, c),
        }
    }
}

/// `line` greedily filled to `w` cells per row, at least one row.
// ponytail: char-wrap (splits words) so the cursor maps exactly; word-wrap with an offset map if it reads badly
fn wrap_rows(line: &str, w: usize) -> Vec<String> {
    let (mut rows, mut cur, mut cw) = (Vec::new(), String::new(), 0);
    for ch in line.chars() {
        let chw = ch.width().unwrap_or(0);
        if cw > 0 && cw + chw > w {
            rows.push(std::mem::take(&mut cur));
            cw = 0;
        }
        cur.push(ch);
        cw += chw;
    }
    rows.push(cur);
    rows
}

/// What live typing captures: a per-line comment or the review body. Enter branches per variant.
enum Draft {
    /// `line` is the cursor's source line when `c` was pressed: the comment lands there, not wherever the cursor drifted.
    /// `edit` means `c` seeded your own comment, so an empty Enter deletes it.
    Comment {
        pane: PaneId,
        line: u32,
        edit: bool,
        text: Editor,
    },
    Review(PaneId, Editor),
}

impl Draft {
    fn editor_mut(&mut self) -> &mut Editor {
        match self {
            Draft::Comment { text: e, .. } | Draft::Review(_, e) => e,
        }
    }

    fn editor(&self) -> &Editor {
        match self {
            Draft::Comment { text: e, .. } | Draft::Review(_, e) => e,
        }
    }
}

/// The theme wizard's answer: `laura` or `neovim`; missing means ask (only when editor panes are on).
fn theme_path() -> PathBuf {
    laura::journal::data_dir().join("editor-theme")
}

/// Start a workspace running `program` (else the shell), with editor and browser panes resolved
/// and the saved theme applied. The TUI and `laura --web` (`web`) share it.
pub fn spawn_tab(program: &[String], rows: u16, cols: u16, web: bool) -> Result<Tab> {
    let browser = laura::browser::resolve();
    let mut tab = if web {
        Tab::spawn_web(build_cmd(program), rows, cols, browser)?
    } else {
        // The TUI keeps the browser, so `open <url>` says it needs `--web`.
        let mut tab = Tab::spawn(build_cmd(program), rows, cols)?;
        tab.browser_path = browser;
        tab
    };
    // Editor panes are resolved once: a terminal's PATH doesn't change under a running Laura.
    tab.editor = laura::editor::resolve();
    tab.editor_theme = std::fs::read_to_string(theme_path()).is_ok_and(|s| s.trim() == "laura");
    Ok(tab)
}

pub fn run(terminal: &mut ratatui::DefaultTerminal, program: Vec<String>) -> Result<()> {
    let area = terminal.size()?;
    let mut tab = spawn_tab(&program, area.height, area.width, false)?;
    let theme_path = theme_path();
    let mut wizard = tab.editor.is_ok() && !theme_path.exists();
    // `^p` opens the panes popup; `Some(buf)` holds the pane id being typed (multi-digit for #10+).
    let mut panes: Option<String> = None;
    // `F12` locks input: every keystroke goes to the shell, Laura intercepts nothing but F12.
    let mut locked = false;
    // `^q` arms a quit confirm; the next key must be `y`.
    let mut confirm_quit = false;
    // Draft captures typing for a comment or the review body.
    let mut draft: Option<Draft> = None;
    let mut help = false;
    // An in-progress left-drag selection.
    let mut selection: Option<Selection> = None;
    // A transient bottom-right toast (message + expiry); armed once if `git` is missing. The user
    // turned editor panes on, so a missing `nvim` is said at startup, not left to a refused `--edit`.
    let mut toast: Option<(String, Instant)> = tab
        .editor
        .as_ref()
        .err()
        .filter(|e| *e == laura::editor::NO_NVIM)
        .map(|e| (format!("⚠ {e}"), Instant::now() + Duration::from_secs(10)));
    let mut toast_git_shown = false;

    loop {
        // Content rect sits above the 1-line hint line.
        let content = content_rect(terminal)?;
        // A comment/review-body draft holds requests so the pane under it can't move; the theme
        // wizard holds them too, so an `open --edit` launches with the answer.
        tab.hold = wizard || draft.is_some();
        tab.drain(content);
        // git-presence is a machine-global fact: arm the "install git" toast once,
        // the first time any diff attempt (open or reload) found no `git` binary.
        if !toast_git_shown && laura::gitdiff::git_missing() {
            toast_git_shown = true;
            toast = Some((
                view::NO_GIT.into(),
                Instant::now() + Duration::from_secs(10),
            ));
        }
        if toast.as_ref().is_some_and(|(_, e)| Instant::now() >= *e) {
            toast = None;
        }
        // One rect map per frame — render, resize, wheel and popups all read it.
        let rect_map = tab.tick(content);
        let pty_inner = rect_map
            .get(&PTY_PANE)
            .copied()
            .unwrap_or(content)
            .inner(Margin::new(1, 1));

        let completed = terminal.draw(|f| {
            let rows =
                Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(f.area());
            for (id, rect) in &rect_map {
                let focused = tab.focus == *id;
                if *id == PTY_PANE {
                    let inner = rect.inner(Margin::new(1, 1));
                    f.render_widget(pane_block(None, focused), *rect);
                    tab.pty
                        .with_screen(|s| f.render_widget(PseudoTerminal::new(s), inner));
                    // The alt screen keeps no scrollback — the child owns its history, so no scrollbar.
                    if !tab.pty.on_alt_screen() {
                        // Offset counts up from live (0); the scrollbar counts down from the top.
                        let max = tab.pty.scrollback_max();
                        let view = inner.height as usize;
                        render_scrollbar(
                            f,
                            *rect,
                            max + view,
                            view,
                            max - tab.pty.scrollback_offset(),
                        );
                    }
                } else if let Some(panel) = tab.panels.get(id) {
                    let editor = tab.editors.get(id);
                    if let Some(e) = editor.filter(|e| e.editor_view) {
                        let block =
                            pane_block(Some(view::pane_title(*id, panel, Some(e))), focused);
                        f.render_widget(block, *rect);
                        e.pty.with_screen(|s| {
                            f.render_widget(PseudoTerminal::new(s), rect.inner(Margin::new(1, 1)))
                        });
                    } else {
                        render_panel(f, *rect, *id, panel, focused, editor);
                    }
                }
            }
            let hint = if locked {
                Hint::Locked
            } else if wizard {
                Hint::Wizard
            } else if confirm_quit {
                Hint::Quit
            } else if let Some(d) = &draft {
                match d {
                    Draft::Comment { line, edit, .. } => Hint::Comment {
                        at: format!("L{line}"),
                        edit: *edit,
                    },
                    Draft::Review(..) => Hint::Review,
                }
            } else if panes.is_some() {
                Hint::Panes
            } else {
                Hint::Pane
            };
            f.render_widget(
                Paragraph::new(view::hint_text(&view::hint(&tab, hint))).dim(),
                rows[1],
            );
            // A live draft grows a bordered input box over the bottom of the shell pane — never over
            // a panel, so the file under review stays visible.
            if let Some(d) = &draft
                && let Some(pty_rect) = rect_map.get(&PTY_PANE)
            {
                let title = match d {
                    Draft::Comment { .. } => view::DRAFT_COMMENT,
                    Draft::Review(..) => view::DRAFT_REVIEW,
                };
                render_draft_box(f, *pty_rect, title, d.editor(), !help);
            }
            if let Some(buf) = &panes {
                render_panes(f, &tab, buf);
            }
            if help {
                render_help(f);
            }
            if wizard {
                render_wizard(f, &theme_path);
            }
            // A live toast (e.g. a failed submit) wins the bottom-right slot; then the exited shell's
            // notice, while Neovim holds Laura open; then a focused frozen pane's notice, which
            // returns once the toast expires, until submit / ^r clears it.
            if let Some((msg, _)) = &toast {
                render_toast(f, msg);
            } else if tab.pty.has_exited() {
                render_toast(f, SHELL_EXITED);
            } else if let Some(p) = tab.focused_panel().filter(|p| p.source_changed) {
                render_toast(
                    f,
                    &view::frozen_notice(view::base_name(&p.path), f.area().width),
                );
            }
            // Reverse-video the drag selection over whatever was just drawn (PTY or panel, one path).
            if let Some(sel) = &selection
                && let Some(rect) = rect_map.get(&sel.pane)
            {
                let inner = rect.inner(Margin::new(1, 1));
                mouse::highlight(f.buffer_mut(), inner, sel.anchor, sel.head);
            }
        })?;
        // The post-draw buffer swap resets `current_buffer`, so copy-on-release must read the frame
        // that just showed the selection — snapshot it here while a drag is live, not the empty next one.
        let drag_frame = selection.is_some().then(|| completed.buffer.clone());

        // The shell exiting quits Laura, once no Neovim has unsaved edits.
        if tab.shell_done() {
            break;
        }

        // ~60fps poll so PTY output and panel reloads redraw without a keypress; ratatui diffs, so idle redraws emit ~nothing.
        if event::poll(Duration::from_millis(16))? {
            #[cfg_attr(not(windows), allow(unused_mut))] // only rebound on Windows
            let mut ev = event::read()?;
            // Windows has no Event::Paste — crossterm reads console records, not the VT byte stream —
            // so a paste lands as a rapid burst of Char/Enter key events. If more input is already
            // queued the instant we read the first key, it's a paste, not a keystroke: drain the burst
            // into one synthetic Event::Paste so the arm below handles it as a unit.
            #[cfg(windows)]
            if let Event::Key(k) = &ev
                && k.kind == KeyEventKind::Press
                && matches!(k.code, KeyCode::Char(_) | KeyCode::Enter)
                && event::poll(Duration::ZERO)?
            {
                ev = coalesce_paste_burst(*k)?;
            }
            match ev {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                    if wizard {
                        // The startup wizard takes every key, F12 included, until answered or put off.
                        let answer = match key.code {
                            KeyCode::Char('y') => Some(true),
                            KeyCode::Char('n') => Some(false),
                            _ => None,
                        };
                        wizard = answer.is_none() && key.code != KeyCode::Esc;
                        if let Some(yes) = answer {
                            // A failed write only means the wizard asks again next launch.
                            let _ = std::fs::create_dir_all(laura::journal::data_dir()).and_then(
                                |()| {
                                    std::fs::write(
                                        &theme_path,
                                        if yes { "laura" } else { "neovim" },
                                    )
                                },
                            );
                            toast = Some((
                                "✅ Experimental · Editor panes setup complete".into(),
                                Instant::now() + Duration::from_secs(10),
                            ));
                            tab.editor_theme = yes;
                        }
                    } else if key.code == KeyCode::F(12) {
                        locked = !locked; // the one key Laura keeps even while locked
                    } else if locked {
                        // Everything to the focused shell or editor pane (else the shell); only F12
                        // (handled above) is intercepted.
                        if let Some(bytes) = key_to_bytes(key.code, key.modifiers) {
                            let _ = tab.act(UserAction::Input {
                                pane: key_target(&tab),
                                bytes,
                            });
                        }
                    } else if confirm_quit {
                        if key.code == KeyCode::Char('y') {
                            break;
                        }
                        confirm_quit = false; // any other keystroke cancels
                    } else if help {
                        help = false;
                    } else if ctrl && key.code == KeyCode::Char('h') {
                        help = true;
                    } else if draft.is_some() {
                        let ed = draft.as_mut().expect("draft checked above").editor_mut();
                        match key.code {
                            KeyCode::Char(c) => ed.insert(c.encode_utf8(&mut [0; 4])),
                            KeyCode::Backspace => ed.backspace(),
                            KeyCode::Delete => ed.delete(),
                            KeyCode::Left => ed.left(),
                            KeyCode::Right => ed.right(),
                            KeyCode::Home => ed.home(),
                            KeyCode::End => ed.end(),
                            // `\`+Enter inserts a newline (Shift+Enter isn't portable); plain Enter submits.
                            KeyCode::Enter
                                if draft.as_mut().unwrap().editor_mut().continue_line() => {}
                            KeyCode::Enter => match draft.take().unwrap() {
                                Draft::Comment {
                                    pane,
                                    line,
                                    text: ed,
                                    ..
                                } => {
                                    let res = tab.act(UserAction::Comment {
                                        pane,
                                        line,
                                        text: ed.text,
                                    });
                                    if let Err(t) = res {
                                        toast = Some((t, Instant::now() + Duration::from_secs(5)));
                                    }
                                }
                                Draft::Review(pane, Editor { text: body, .. }) => {
                                    let res = tab.act(UserAction::Submit {
                                        pane,
                                        body: body.clone(),
                                    });
                                    if let Err(t) = res {
                                        toast = Some((t, Instant::now() + Duration::from_secs(5)));
                                        draft = Some(Draft::Review(pane, Editor::new(body)));
                                    } else {
                                        // A retry that lands must not leave "Enter to retry" up.
                                        if toast.as_ref().is_some_and(|(m, _)| m == SUBMIT_FAILED) {
                                            toast = None;
                                        }
                                    }
                                }
                            },
                            KeyCode::Esc => {
                                draft = None;
                                // Nothing left to retry, so "Enter to retry" would lie.
                                if toast.as_ref().is_some_and(|(m, _)| m == SUBMIT_FAILED) {
                                    toast = None;
                                }
                            }
                            _ => {}
                        }
                    } else if panes.is_some() {
                        // Typed digits are the pane id (shell is #0), matching `laura close`/`focus`.
                        // Commit as soon as no larger id could still be typed, else wait for `Enter`.
                        match key.code {
                            KeyCode::Char(c) if c.is_ascii_digit() => {
                                let buf = panes.as_mut().expect("popup open");
                                buf.push(c);
                                let buf = buf.clone();
                                let ids = tab.layout.order();
                                if !pane_id_ambiguous(&ids, &buf) {
                                    if let Some(id) = pane_id_exact(&ids, &buf) {
                                        let _ = tab.act(UserAction::Focus { pane: id });
                                    }
                                    panes = None;
                                }
                            }
                            KeyCode::Backspace => {
                                panes.as_mut().expect("popup open").pop();
                            }
                            KeyCode::Enter => {
                                let buf = panes.take().expect("popup open");
                                let ids = tab.layout.order();
                                if let Some(id) = pane_id_exact(&ids, &buf) {
                                    let _ = tab.act(UserAction::Focus { pane: id });
                                }
                            }
                            _ => panes = None, // Esc or anything else dismisses
                        }
                    } else if ctrl && key.code == KeyCode::Char('p') {
                        panes = Some(String::new());
                    } else if ctrl && key.code == KeyCode::Char('q') {
                        match tab.act(UserAction::Quit) {
                            Err(message) => {
                                toast = Some((message, Instant::now() + Duration::from_secs(5)))
                            }
                            Ok(_) => confirm_quit = true,
                        }
                    } else if ctrl
                        && matches!(key.code, KeyCode::Char('l') | KeyCode::Char('L'))
                        && tab.editors.contains_key(&tab.focus)
                    {
                        let _ = tab.act(UserAction::ToggleEditorView { pane: tab.focus });
                    } else if tab.focus != PTY_PANE && focused_pty(&tab).is_some() {
                        // An editor pane takes every other keystroke, Esc and PageUp/PageDown included.
                        if let Some(bytes) = key_to_bytes(key.code, key.modifiers) {
                            let _ = tab.act(UserAction::Input {
                                pane: tab.focus,
                                bytes,
                            });
                        }
                    } else if tab.focus != PTY_PANE {
                        // A panel is focused: arrows move its cursor, c/S draft, Esc leaves.
                        let pane = tab.focus;
                        let res = match key.code {
                            KeyCode::Up => tab.act(UserAction::MoveCursor { pane, delta: -1 }),
                            KeyCode::Down => tab.act(UserAction::MoveCursor { pane, delta: 1 }),
                            KeyCode::Left => tab.act(UserAction::ScrollH { pane, delta: -1 }),
                            KeyCode::Right => tab.act(UserAction::ScrollH { pane, delta: 1 }),
                            KeyCode::Char('d') => tab.act(UserAction::ToggleDiff { pane }),
                            KeyCode::Char('p') => tab.act(UserAction::TogglePause { pane }),
                            KeyCode::Char('c') => {
                                let line = tab.focused_panel().map_or(1, Panel::cursor_line);
                                tab.act(UserAction::BeginComment { pane }).map(|seed| {
                                    let seed = seed.unwrap_or_default();
                                    draft = Some(Draft::Comment {
                                        pane,
                                        line,
                                        edit: !seed.is_empty(),
                                        text: Editor::new(seed),
                                    });
                                    None
                                })
                            }
                            KeyCode::Char('r') if ctrl => tab.act(UserAction::Refresh { pane }),
                            KeyCode::Char('r') => {
                                tab.act(UserAction::ToggleThread { pane, line: None })
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') => {
                                tab.act(UserAction::JumpThread {
                                    pane,
                                    next: key.code == KeyCode::Char('n'),
                                })
                            }
                            KeyCode::Char('S') => tab
                                .act(UserAction::BeginReview { pane })
                                .inspect(|_| draft = Some(Draft::Review(pane, Editor::default()))),
                            KeyCode::Char('x') => tab.act(UserAction::Close { pane }),
                            KeyCode::Char('h') => tab.act(UserAction::ClearHighlight { pane }),
                            KeyCode::Esc => tab.act(UserAction::Focus { pane: PTY_PANE }),
                            _ => Ok(None),
                        };
                        if let Err(t) = res {
                            toast = Some((t, Instant::now() + Duration::from_secs(5)));
                        }
                    } else if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown)
                        && !tab.pty.on_alt_screen()
                    {
                        // The main screen scrolls Laura's own scrollback; on the alt screen the
                        // child owns its history, so the key goes to it below.
                        if key.code == KeyCode::PageUp {
                            tab.pty.scroll(pty_inner.height as isize);
                        } else {
                            tab.pty.scroll(-(pty_inner.height as isize));
                        }
                    } else if let Some(bytes) = key_to_bytes(key.code, key.modifiers) {
                        let _ = tab.act(UserAction::Input {
                            pane: PTY_PANE,
                            bytes,
                        });
                    }
                }
                // `tab.tick` applies the resize at the top of the loop from the content-inner
                // rect; resizing the PTY to the raw terminal size here would clip the bottom rows.
                Event::Resize(..) => {}
                // A pasted block arrives as one unit: into a draft body, or to the shell wrapped in
                // bracketed paste (no trailing CR) so a REPL doesn't submit per newline.
                Event::Paste(s) => {
                    if let Some(d) = draft.as_mut() {
                        d.editor_mut().insert(&s);
                    } else if locked
                        || (focused_pty(&tab).is_some()
                            && panes.is_none()
                            && !help
                            && !confirm_quit
                            && !wizard)
                    {
                        let _ = tab.act(UserAction::Paste {
                            pane: key_target(&tab),
                            text: s,
                        });
                    }
                    // Popups / focused panel without a draft / help / confirm — paste is meaningless, drop it.
                }
                // Mouse: a child in SGR mouse mode gets real events; else the wheel scrolls
                // history/cursor and a plain left drag selects within the pane. Shift+drag is the
                // terminal's own reserved gesture (native, whole-window selection).
                Event::Mouse(m) => {
                    let over = pane_at_point(&rect_map, m.column, m.row);
                    let left = matches!(
                        m.kind,
                        MouseEventKind::Down(MouseButton::Left)
                            | MouseEventKind::Drag(MouseButton::Left)
                            | MouseEventKind::Up(MouseButton::Left)
                    );

                    let capture = over.and_then(|id| {
                        let pty = tab.pane_pty(id)?;
                        Some((id, pty, pty.mouse_capture()?))
                    });
                    if let Some((id, pty, mode)) = capture {
                        // Forward to the child; no `to_live` — forwarding mustn't disturb the view.
                        let inner = rect_map
                            .get(&id)
                            .map_or(pty_inner, |r| r.inner(Margin::new(1, 1)));
                        if let Some(bytes) = mouse::sgr_mouse_bytes(&m, inner, mode) {
                            let _ = pty.write(&bytes);
                        }
                    } else {
                        match m.kind {
                            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                                let step: isize = if matches!(m.kind, MouseEventKind::ScrollUp) {
                                    -3
                                } else {
                                    3
                                };
                                match over {
                                    Some(PTY_PANE) | None => tab.pty.scroll(-step),
                                    // Neovim without mouse mode drops the wheel: the panel under it is hidden.
                                    Some(id) => {
                                        if tab.pane_pty(id).is_none()
                                            && let Some(p) = tab.panels.get_mut(&id)
                                        {
                                            p.scroll_view(step);
                                        }
                                    }
                                }
                            }
                            _ if left => {
                                mouse::update_selection(&mut selection, &m, over, &rect_map)
                            }
                            _ => {}
                        }
                    }

                    // Finish a selection on release: copy its glyphs (OSC 52 to our own stdout), then clear.
                    if matches!(m.kind, MouseEventKind::Up(MouseButton::Left))
                        && let Some(sel) = selection.take()
                        && sel.anchor != sel.head
                        && let Some(buf) = &drag_frame
                        && let Some(rect) = rect_map.get(&sel.pane)
                    {
                        let inner = rect.inner(Margin::new(1, 1));
                        // Panels draw a gutter + wrap the source, so copy source-aware; the PTY owns
                        // its own glyphs (no gutter, child wraps) → scrape as-is.
                        let panel = tab
                            .panels
                            .get(&sel.pane)
                            .filter(|_| tab.pane_pty(sel.pane).is_none());
                        let text = match panel {
                            Some(panel) => {
                                let layout = panel.layout(inner.width as usize);
                                let off = panel.scroll_offset(&layout, inner.height as usize);
                                let line_at: Vec<usize> =
                                    layout.rows.iter().map(|r| r.line).collect();
                                let gutter = layout.gutter_width as u16 + 5;
                                mouse::extract_panel(
                                    buf, inner, gutter, off, &line_at, sel.anchor, sel.head,
                                )
                            }
                            None => mouse::extract(buf, inner, sel.anchor, sel.head),
                        };
                        if !text.is_empty() {
                            use std::io::Write;
                            let mut out = std::io::stdout();
                            let _ = out.write_all(&mouse::osc52(&text));
                            let _ = out.flush();
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// A bordered draft input box pinned to the bottom of `area` (the shell pane). Grows 2..=6 rows,
/// then scrolls to keep the cursor visible. `cursor` places the terminal cursor at the edit point.
fn render_draft_box(f: &mut Frame, area: Rect, title: &str, ed: &Editor, cursor: bool) {
    let inner_w = area.width.saturating_sub(2).max(1) as usize;
    let mut rows: Vec<String> = ed
        .text
        .split('\n')
        .flat_map(|l| wrap_rows(l, inner_w))
        .collect();
    let (mut r, mut c) = ed.cursor_rc(inner_w);
    // Past a full row the cursor wraps onto a fresh one.
    if c == inner_w {
        rows.insert(r + 1, String::new());
        (r, c) = (r + 1, 0);
    }
    let view = rows.len().clamp(2, 6);
    let h = (view as u16 + 2).min(area.height); // + border
    let box_area = Rect {
        x: area.x,
        y: area.y + area.height - h,
        width: area.width,
        height: h,
    };
    let inner_h = h.saturating_sub(2).max(1) as usize;
    let scroll = r.saturating_sub(inner_h - 1);
    let text: Vec<Line> = rows.into_iter().map(Line::from).collect();
    f.render_widget(Clear, box_area);
    f.render_widget(
        Paragraph::new(text)
            .block(Block::bordered().title(format!(" {title} ")))
            .scroll((scroll as u16, 0)),
        box_area,
    );
    if cursor && h > 2 && area.width > 2 {
        let x = box_area.x + 1 + c as u16;
        let y = box_area.y + 1 + (r - scroll) as u16;
        f.set_cursor_position((x, y));
    }
}

/// The frame minus the hint line (bottom). Autoresizes first: `get_frame()` keeps the old size until
/// `draw` syncs it, and a stale rect panics tui-term on a shrink.
fn content_rect<B: Backend>(terminal: &mut Terminal<B>) -> Result<Rect, B::Error> {
    terminal.autoresize()?;
    Ok(
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)])
            .split(terminal.get_frame().area())[0],
    )
}

/// The pane whose rect contains `(col, row)`, if any.
fn pane_at_point(map: &HashMap<laura::PaneId, Rect>, col: u16, row: u16) -> Option<laura::PaneId> {
    map.iter()
        .find(|(_, r)| col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height)
        .map(|(id, _)| *id)
}

/// A pane's border block: focused keeps the default color, unfocused dims to gray.
fn pane_block(title: Option<Line<'static>>, focused: bool) -> Block<'static> {
    let mut block = Block::bordered();
    if let Some(t) = title {
        block = block.title(t);
    }
    if !focused {
        block = block.border_style(Style::default().fg(Color::Rgb(90, 90, 90)));
    }
    block
}

/// Draw one file pane: its styled rows (see `view::panel_lines`) and an overflow scrollbar.
/// `editor`: the editor pane's Neovim when this is its file view.
fn render_panel(
    f: &mut Frame,
    area: Rect,
    id: PaneId,
    panel: &Panel,
    focused: bool,
    editor: Option<&Nvim>,
) {
    let inner_w = area.width.saturating_sub(2) as usize; // minus borders
    let layout = panel.layout(inner_w);
    let rows = view::panel_lines(panel, &layout, focused, inner_w, false);
    // The offset is remembered across frames (scrolls only at the edges); see `scroll_offset`.
    let height = area.height.saturating_sub(2) as usize;
    let total_rows = rows.len();
    let offset = panel.scroll_offset(&layout, height).min(u16::MAX as usize) as u16;
    let mut block = pane_block(Some(view::pane_title(id, panel, editor)), focused);
    if panel.source_changed {
        // Trailing space: ⚠ renders width-2 under VS16, so pad or the corner clips.
        block = block.title_bottom(Line::from(" ⚠ ").right_aligned().fg(view::WARN));
    }
    f.render_widget(Paragraph::new(rows).block(block).scroll((offset, 0)), area);
    render_scrollbar(f, area, total_rows, height, offset as usize);
}

/// Right-edge vertical scrollbar for a pane; drawn only when content overflows the viewport. Shared by panels and the PTY.
fn render_scrollbar(f: &mut Frame, area: Rect, total: usize, view: usize, pos: usize) {
    if total <= view {
        return;
    }
    // ratatui sizes the thumb over `content_length - 1 + view`; `steps + 1` maps pos=0 flush top,
    // pos=steps flush bottom. But with the PTY's 10k-line scrollback the thumb collapses to a single
    // cell parked in the corner — invisible against the border. Cap the reported range at 3× the
    // viewport so the thumb stays ~1/4 of the track, scaling position into it so the ends still map flush.
    let steps = total - view; // real scroll range: pos ∈ 0..=steps
    let (content, position) = if steps > 3 * view {
        (3 * view, pos * 3 * view / steps)
    } else {
        (steps, pos)
    };
    let mut sb = ScrollbarState::new(content + 1)
        .viewport_content_length(view)
        .position(position);
    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓")),
        area.inner(Margin {
            horizontal: 0,
            vertical: 1,
        }),
        &mut sb,
    );
}

/// The shell's initial command: `program` (from `-- <cmd>`) if given, else the default shell.
fn build_cmd(program: &[String]) -> CommandBuilder {
    match program.split_first() {
        Some((prog, args)) => {
            let mut cmd = CommandBuilder::new(prog);
            cmd.args(args);
            with_cwd(cmd)
        }
        None => default_shell(),
    }
}

/// The user's default shell — `$SHELL` on unix, `%COMSPEC%` on Windows.
fn default_shell() -> CommandBuilder {
    let shell = if cfg!(windows) {
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
    };
    with_cwd(CommandBuilder::new(shell))
}

/// Point `cmd` at the current working directory (best-effort).
fn with_cwd(mut cmd: CommandBuilder) -> CommandBuilder {
    if let Ok(cwd) = std::env::current_dir() {
        cmd.cwd(cwd);
    }
    cmd
}

/// The focused terminal pane (shell or editor pane), which takes keys and paste.
fn focused_pty(tab: &Tab) -> Option<&PtyTab> {
    tab.pane_pty(tab.focus)
}

/// The pane whose terminal takes typed keys and paste: the focused one, else the shell.
fn key_target(tab: &Tab) -> PaneId {
    if focused_pty(tab).is_some() {
        tab.focus
    } else {
        PTY_PANE
    }
}

/// True if some live id extends `buf` (a longer id is still reachable, so don't commit yet).
fn pane_id_ambiguous(ids: &[laura::PaneId], buf: &str) -> bool {
    ids.iter().any(|id| {
        let s = id.to_string();
        s.len() > buf.len() && s.starts_with(buf)
    })
}

/// The live pane whose id string exactly equals `buf`, if any.
fn pane_id_exact(ids: &[laura::PaneId], buf: &str) -> Option<laura::PaneId> {
    ids.iter().copied().find(|id| id.to_string() == buf)
}

/// Draw the `^p` panes popup: each pane keyed by its stable id (shell is `#0`); typing that id
/// focuses it. `buf` is the id being typed — its matching row is highlighted.
fn render_panes(f: &mut Frame, tab: &Tab, buf: &str) {
    let key = |k: String, desc: String| {
        Line::from(vec![
            Span::raw("  "),
            k.bold().cyan(),
            Span::raw("  "),
            desc.dim(),
        ])
    };
    let mut lines = vec![];
    for id in tab.layout.order() {
        let name = if id == PTY_PANE {
            "shell".to_string()
        } else {
            tab.panels
                .get(&id)
                .map(|p| view::base_name(&p.path).to_string())
                .unwrap_or_else(|| "?".into())
        };
        // Dim rows the current input can't reach; leave the shortlist bright.
        let matches = buf.is_empty() || id.to_string().starts_with(buf);
        let row = key(format!("{id}"), name);
        lines.push(if matches { row } else { row.dim() });
    }
    lines.push(Line::raw(""));
    let typed = if buf.is_empty() {
        "type a pane id".to_string()
    } else {
        format!("typed: {buf}")
    };
    lines.push(key(typed, "Enter focus · Esc close".into()));

    let width = 40u16;
    let height = lines.len() as u16 + 2; // + border
    let [area] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(f.area());
    let [area] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" Panes ")),
        area,
    );
}

/// Draw a one-line notice in the bottom-right corner (nord yellow), just above the hint line.
fn render_toast(f: &mut Frame, msg: &str) {
    let area = f.area();
    let w = (msg.chars().count() as u16 + 2).min(area.width);
    let rect = Rect {
        x: area.width.saturating_sub(w + 1),
        y: area.height.saturating_sub(2),
        width: w,
        height: 1,
    };
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(format!(" {msg} ")).style(Style::default().fg(view::WARN)),
        rect,
    );
}

/// Draw the help popup in a centered, bordered box.
fn render_help(f: &mut Frame) {
    let lines = view::help_lines();
    let width = lines.iter().map(Line::width).max().unwrap_or(0) as u16 + 4; // + border, margin
    let height = lines.len() as u16 + 2; // + border
    let [area] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(f.area());
    let [area] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" Help ")),
        area,
    );
}

/// The startup wizard: should editor panes use Laura's theme? Centered like `render_help`; `answer`
/// is the file the answer is kept in, named so the user can delete it to be asked again.
fn render_wizard(f: &mut Frame, answer: &Path) {
    let lines = vec![
        Line::raw("Laura can open your files in editor panes via Neovim."),
        Line::raw("Would you like to adopt Laura's theme while running"),
        Line::raw("Neovim inside Laura?"),
        Line::raw(""),
        Line::raw("The theme applies only inside Laura; your Neovim config"),
        Line::raw("stays as it is."),
        Line::raw(""),
        Line::raw("If you change your mind, delete").dim(),
        Line::raw(answer.display().to_string()).dim(),
        Line::raw("and restart Laura to bring up this wizard again.").dim(),
        Line::raw(""),
        Line::from(vec![
            " y".bold().cyan(),
            " yes, use Laura's theme · ".into(),
            "n".bold().cyan(),
            " no, keep mine · ".into(),
            "Esc".bold().cyan(),
            " not now".into(),
        ]),
    ];
    let width = lines.iter().map(Line::width).max().unwrap_or(0) as u16 + 4; // + border, margin
    let height = lines.len() as u16 + 2; // + border
    let [area] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(f.area());
    let [area] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" Experimental · Editor panes ")),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::{Editor, content_rect, pane_id_ambiguous, pane_id_exact, render_draft_box};
    use ratatui::{Terminal, backend::TestBackend, layout::Rect};

    // The draft editor only sees keystrokes from the render loop (no CLI/socket surface), so it's checked here.
    #[test]
    fn editor_inserts_and_deletes_at_the_cursor() {
        // Mid-insert.
        let mut e = Editor::new("helo".into());
        e.left();
        e.insert("l");
        assert_eq!((e.text.as_str(), e.cur), ("hello", 4));
        // Backspace and Delete around the cursor.
        e.backspace();
        e.delete();
        assert_eq!((e.text.as_str(), e.cur), ("hel", 3));
        // Home/End stay on the cursor's line.
        let mut e = Editor::new("ab\ncd".into());
        e.home();
        assert_eq!(e.cur, 3);
        e.left();
        e.home();
        assert_eq!(e.cur, 0);
        e.end();
        assert_eq!(e.cur, 2);
        // `\`+Enter mid-text splits the line at the cursor.
        let mut e = Editor::new("ab\\cd".into());
        e.home();
        (0..3).for_each(|_| e.right());
        assert!(e.continue_line());
        assert_eq!((e.text.as_str(), e.cur), ("ab\ncd", 3));
        assert!(!e.continue_line());
        // cursor_rc at a wrap boundary and after a newline.
        assert_eq!(Editor::new("abcd".into()).cursor_rc(4), (0, 4));
        assert_eq!(Editor::new("abcde".into()).cursor_rc(4), (1, 1));
        assert_eq!(Editor::new("abcde\n".into()).cursor_rc(4), (2, 0));
        // Mid-line wrap boundary.
        let mut e = Editor::new("abcdefgh".into());
        (0..4).for_each(|_| e.left());
        assert_eq!(e.cursor_rc(4), (1, 0));
        // Wide glyphs.
        assert_eq!(Editor::new("日本a".into()).cursor_rc(20), (0, 5));
        // A multi-byte char is one step.
        let mut e = Editor::new("é".into());
        e.left();
        assert_eq!(e.cur, 0);
        e.right();
        e.backspace();
        assert_eq!((e.text.as_str(), e.cur), ("", 0));
    }

    // The content rect feeds the render loop (no CLI/socket surface), so the shrink is checked here.
    #[test]
    fn content_rect_follows_a_shrink() {
        let mut t = Terminal::new(TestBackend::new(80, 24)).expect("test backend can't fail");
        t.draw(|_| {}).expect("test backend can't fail");
        t.backend_mut().resize(60, 15);
        assert_eq!(
            content_rect(&mut t).expect("test backend can't fail"),
            Rect::new(0, 0, 60, 14)
        );
    }

    fn draw_draft(ed: &Editor, w: u16, h: u16) -> (Vec<String>, (u16, u16)) {
        let mut t = Terminal::new(TestBackend::new(w, h)).expect("test backend can't fail");
        t.draw(|f| render_draft_box(f, Rect::new(0, 0, w, h), "t", ed, true))
            .expect("test backend can't fail");
        let buf = t.backend().buffer();
        let rows = (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect())
            .collect();
        let pos = t.get_cursor_position().expect("test backend can't fail");
        (rows, (pos.x, pos.y))
    }

    // The draft box is drawn by the render loop (no CLI/socket surface), so its cursor is checked here.
    #[test]
    fn draft_box_keeps_the_cursor_on_its_row() {
        // Mid-line wrap boundary: no blank row.
        let mut e = Editor::new("abcdefgh".into());
        (0..4).for_each(|_| e.left());
        let (rows, pos) = draw_draft(&e, 6, 10);
        let body: Vec<&str> = rows
            .iter()
            .filter_map(|r| r.strip_prefix('│')?.strip_suffix('│'))
            .collect();
        assert_eq!(body, ["abcd", "efgh"], "{rows:#?}");
        assert_eq!(pos, (1, 8));
        // Short pane: cursor on the `7` row, not the border.
        let (rows, pos) = draw_draft(&Editor::new("1\n2\n3\n4\n5\n6\n7".into()), 10, 5);
        assert_eq!(pos.1, 3, "{rows:#?}");
        assert!(rows[3].contains('7'), "{rows:#?}");
        // `日本a` is 5 cells wide.
        let (_, pos) = draw_draft(&Editor::new("日本a".into()), 20, 5);
        assert_eq!(pos.0, 6);
    }

    // The pane-id popup entry lives in the TUI event loop (no CLI/socket surface), so its
    // disambiguation is checked here per CLAUDE.md's exception. This is the `1` vs `10` case.
    #[test]
    fn pane_id_entry_resolves_1_versus_10() {
        let ids = [0u64, 1, 2, 10];
        // "1" is a live id but could still grow into "10" -> wait, don't commit.
        assert!(pane_id_ambiguous(&ids, "1"));
        assert_eq!(pane_id_exact(&ids, "1"), Some(1));
        // "10" is terminal -> commit to #10.
        assert!(!pane_id_ambiguous(&ids, "10"));
        assert_eq!(pane_id_exact(&ids, "10"), Some(10));
        // "2" and "0" have no longer sibling -> instant commit (the common <10 case).
        assert!(!pane_id_ambiguous(&ids, "2"));
        assert!(!pane_id_ambiguous(&ids, "0"));
        assert_eq!(pane_id_exact(&ids, "0"), Some(0));
        // A typed id that doesn't exist commits to nothing.
        assert!(!pane_id_ambiguous(&ids, "7"));
        assert_eq!(pane_id_exact(&ids, "7"), None);
    }
}
