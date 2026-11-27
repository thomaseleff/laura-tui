//! The draw loop and its widgets: hosts the workspace, renders panes, and routes input to the engine.

use std::collections::HashMap;
use std::ops::ControlFlow;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use portable_pty::CommandBuilder;
use ratatui::backend::Backend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::layout::{Constraint, Flex, Layout, Margin, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
};
use ratatui::{Frame, Terminal};
use tui_term::widget::PseudoTerminal;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use laura::editor::Nvim;
use laura::layout::workspace_fits;
use laura::protocol::PTY_PANE;
use laura::tab::{Draft, Mode, Overlay, SHELL_EXITED};
use laura::view::{self, Hint};
use laura::{PaneId, Panel, PtyTab, Tab, UserAction};

use crate::keys::key_to_bytes;
use crate::mouse::{self, Pointer, Selection};

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

/// The theme wizard's answer, `laura` or `neovim`; `None`: ask (only when editor panes are on).
fn editor_theme() -> Option<String> {
    if let Some(v) = laura::settings::get("editor_theme") {
        return v.as_str().map(String::from);
    }
    // An earlier Laura kept the answer in `~/.laura/editor-theme`. A failed move keeps the file,
    // so Laura tries again at the next start.
    let old = laura::platform::data_dir().join("editor-theme");
    let s = std::fs::read_to_string(&old).ok()?.trim().to_string();
    if laura::settings::set("editor_theme", Some(s.clone().into())).is_ok() {
        let _ = std::fs::remove_file(&old);
    }
    Some(s)
}

/// Start a workspace running `program` (else the shell), with editor and browser panes resolved
/// and the saved theme applied. The TUI and the web app (`web`) share it.
pub fn spawn_tab(program: &[String], rows: u16, cols: u16, web: bool) -> Result<Tab> {
    let browser = laura::browser::resolve();
    let mut tab = if web {
        Tab::spawn_web(build_cmd(program), rows, cols, browser)?
    } else {
        // The TUI keeps the browser, so `open <url>` says it needs the web app.
        let mut tab = Tab::spawn(build_cmd(program), rows, cols)?;
        tab.browser_path = browser;
        tab
    };
    // Editor panes are resolved once: a terminal's PATH doesn't change under a running Laura.
    tab.editor = laura::editor::resolve();
    let theme = editor_theme();
    tab.editor_theme = theme.as_deref() == Some("laura");
    // The web app has no wizard: it starts with Neovim's own theme until the TUI asks.
    if !web && tab.editor.is_ok() && theme.is_none() {
        tab.ask_theme();
    }
    Ok(tab)
}

/// A bottom-right toast: its text, its expiry, and the error it shows.
type Toast = (String, Instant, Option<laura::tab::Error>);

/// The toast shows a failed submit, which a retry or `Esc` takes down.
fn submit_failed(toast: &Option<Toast>) -> bool {
    matches!(toast, Some((_, _, Some(laura::tab::Error::SubmitFailed))))
}

/// What the loop drew this frame, which the input handlers read.
struct Shown {
    /// The workspace above the hint line.
    content: Rect,
    rects: HashMap<PaneId, Rect>,
    /// The shell's rect inside its border.
    pty_inner: Rect,
    /// The frame that showed the drag selection, while a drag is live.
    drag_frame: Option<Buffer>,
}

/// The TUI's loop state: the workspace, what the TUI draws of its modes, the pointer and the toast.
/// The modes live in `Tab` (`Tab::modes`).
struct Tui {
    tab: Tab,
    /// The pane id typed in the panes popup (multi-digit for #10+).
    typed: String,
    /// The draft's text (`Tab::draft`).
    text: Editor,
    pointer: Pointer,
    /// A transient bottom-right toast; armed by a queued notice, and once if `git` is missing.
    toast: Option<Toast>,
    git_shown: bool,
    /// The count of the latest say the toast showed (`Tab::latest_say`).
    said: u64,
}

pub fn run(terminal: &mut ratatui::DefaultTerminal, program: Vec<String>) -> Result<()> {
    let area = terminal.size()?;
    let mut tui = Tui::new(spawn_tab(&program, area.height, area.width, false)?);
    loop {
        // Content rect sits above the 1-line hint line.
        let content = content_rect(terminal)?;
        let fits = workspace_fits(terminal.get_frame().area());
        let rects = tui.before_draw(content);
        let pty_inner = rects
            .get(&PTY_PANE)
            .copied()
            .unwrap_or(content)
            .inner(Margin::new(1, 1));
        let completed = terminal.draw(|f| tui.draw(f, fits, &rects))?;
        // The post-draw buffer swap resets `current_buffer`, so copy-on-release must read the frame
        // that just showed the selection — snapshot it here while a drag is live, not the empty next one.
        let drag_frame = tui
            .pointer
            .selection
            .is_some()
            .then(|| completed.buffer.clone());

        // The shell exiting quits Laura, once no Neovim has unsaved edits.
        if tui.tab.shell_done() {
            break;
        }
        // ~60fps poll so PTY output and panel reloads redraw without a keypress; ratatui diffs, so idle redraws emit ~nothing.
        if !event::poll(Duration::from_millis(16))? {
            continue;
        }
        let shown = Shown {
            content,
            rects,
            pty_inner,
            drag_frame,
        };
        if tui.on_event(read_event()?, fits, &shown).is_break() {
            break;
        }
    }
    Ok(())
}

/// The next event. Windows has no Event::Paste — crossterm reads console records, not the VT byte
/// stream — so a paste lands as a rapid burst of Char/Enter key events. If more input is already
/// queued the instant we read the first key, it's a paste, not a keystroke: drain the burst into one
/// synthetic Event::Paste so `on_paste` handles it as a unit.
fn read_event() -> Result<Event> {
    #[cfg_attr(not(windows), allow(unused_mut))] // only rebound on Windows
    let mut ev = event::read()?;
    #[cfg(windows)]
    if let Event::Key(k) = &ev
        && k.kind == KeyEventKind::Press
        && matches!(k.code, KeyCode::Char(_) | KeyCode::Enter)
        && event::poll(Duration::ZERO)?
    {
        ev = coalesce_paste_burst(*k)?;
    }
    Ok(ev)
}

impl Tui {
    fn new(tab: Tab) -> Tui {
        // The user turned editor panes on, so a missing `nvim` is said at startup, not left to a
        // refused `--edit`.
        let toast = tab
            .editor
            .as_ref()
            .err()
            .filter(|e| **e == laura::editor::Error::NoNvim)
            .map(|e| {
                let until = Instant::now() + Duration::from_secs(10);
                (format!("⚠ {e}"), until, None)
            });
        Tui {
            tab,
            typed: String::new(),
            text: Editor::default(),
            pointer: Pointer::default(),
            toast,
            git_shown: false,
            said: 0,
        }
    }

    /// Show `text` in the toast for `secs` seconds.
    fn notice(&mut self, text: String, secs: u64) {
        self.toast = Some((text, Instant::now() + Duration::from_secs(secs), None));
    }

    /// Per-frame upkeep before drawing `content`: the agent's requests, the notices, the toast's
    /// expiry. Returns each pane's rect: render, resize, wheel and popups all read it.
    fn before_draw(&mut self, content: Rect) -> HashMap<PaneId, Rect> {
        // A draft holds requests so the pane under it can't move; the theme wizard holds them too,
        // so an `open --edit` launches with the answer (`Tab::held`).
        self.tab.drain(content);
        // The web app shows each notice; the TUI shows the newest in its toast.
        let newest = self.tab.notices.drain(..).next_back();
        if let Some((_, text)) = newest {
            // The theme's answer ends the wizard, so its notice stays long enough to read.
            let secs = if text == laura::tab::THEME_SET { 10 } else { 5 };
            self.notice(text, secs);
        }
        // The TUI has no chat: each new say shows in the toast, over this frame's other notice.
        let say = self.tab.latest_say().map(|(n, body)| (n, body.to_string()));
        if let Some((n, body)) = say
            && n != self.said
        {
            self.said = n;
            self.notice(body, 10);
        }
        // git-presence is a machine-global fact: arm the "install git" toast once,
        // the first time any diff attempt (open or reload) found no `git` binary.
        if !self.git_shown && laura::gitdiff::git_missing() {
            self.git_shown = true;
            self.notice(view::NO_GIT.into(), 10);
        }
        if self
            .toast
            .as_ref()
            .is_some_and(|(_, e, _)| Instant::now() >= *e)
        {
            self.toast = None;
        }
        self.tab.tick(content)
    }

    /// Draw one frame: the panes, the hint line, the overlays, the toast and the drag selection.
    fn draw(&self, f: &mut Frame, fits: bool, rects: &HashMap<PaneId, Rect>) {
        if !fits {
            return draw_too_small(f);
        }
        let rows = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(f.area());
        let hint = view::mode_hint(&self.tab).unwrap_or(Hint::Pane);
        draw_panes(f, &self.tab, rects);
        f.render_widget(
            Paragraph::new(view::hint_text(&view::hint(&self.tab, hint))).dim(),
            rows[1],
        );
        self.draw_overlays(f, rects);
        self.draw_toast(f);
        // Reverse-video the drag selection over whatever was just drawn (PTY or panel, one path).
        if let Some(sel) = &self.pointer.selection
            && let Some(rect) = rects.get(&sel.pane)
        {
            let inner = rect.inner(Margin::new(1, 1));
            mouse::highlight(f.buffer_mut(), inner, sel.anchor, sel.head);
        }
    }

    /// The draft's box, and the overlay: the panes popup, the help or the wizard.
    fn draw_overlays(&self, f: &mut Frame, rects: &HashMap<PaneId, Rect>) {
        // A live draft grows a bordered input box over the bottom of the shell pane — never over
        // a panel, so the file under review stays visible.
        if let Some(d) = self.tab.draft()
            && let Some(pty_rect) = rects.get(&PTY_PANE)
        {
            let title = match d {
                Draft::Comment { .. } => view::DRAFT_COMMENT,
                Draft::Review { .. } => view::DRAFT_REVIEW,
            };
            let cursor = self.tab.mode() == Some(Mode::Draft);
            render_draft_box(f, *pty_rect, title, &self.text, cursor);
        }
        match self.tab.overlay() {
            Some(Overlay::Selector) => render_panes(f, &self.tab, &self.typed),
            Some(Overlay::Help) => render_help(f),
            Some(Overlay::Wizard) => render_wizard(f, &laura::settings::path()),
            Some(Overlay::Quit) | None => {}
        }
    }

    /// A live toast (e.g. a failed submit) wins the bottom-right slot; then the exited shell's
    /// notice, while Neovim holds Laura open; then a focused frozen pane's notice, which returns
    /// once the toast expires, until submit / ^r clears it.
    fn draw_toast(&self, f: &mut Frame) {
        if let Some((msg, _, _)) = &self.toast {
            render_toast(f, msg);
        } else if self.tab.pty.has_exited() {
            render_toast(f, SHELL_EXITED);
        } else if let Some(p) = self.tab.focused_panel().filter(|p| p.source_changed) {
            render_toast(
                f,
                &view::frozen_notice(view::base_name(&p.path), f.area().width),
            );
        }
    }

    /// One event. `Break`: the user quit.
    fn on_event(&mut self, ev: Event, fits: bool, shown: &Shown) -> ControlFlow<()> {
        match ev {
            // The user can't see what a keystroke or a click would do.
            _ if !fits && !matches!(ev, Event::Resize(..)) => {}
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                return self.on_key(key, shown.pty_inner);
            }
            // `tab.tick` applies the resize at the top of the loop from the content-inner
            // rect; resizing the PTY to the raw terminal size here would clip the bottom rows.
            Event::Resize(..) => {}
            Event::Paste(s) => self.on_paste(s),
            Event::Mouse(m) => {
                let (frame, ptr) = (shown.drag_frame.as_ref(), &mut self.pointer);
                let (content, map, inner) = (shown.content, &shown.rects, shown.pty_inner);
                if let Err(e) = on_mouse(&mut self.tab, m, content, map, inner, ptr, frame) {
                    self.notice(e.to_string(), 5);
                }
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }

    /// A keystroke: to the mode that takes it (`Tab::mode`), else to the focused pane. `Break`: the
    /// user quit.
    fn on_key(&mut self, key: KeyEvent, pty_inner: Rect) -> ControlFlow<()> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match self.tab.mode() {
            Some(Mode::Wizard) => self.wizard_key(key.code),
            // The one key Laura keeps even while locked.
            _ if key.code == KeyCode::F(12) => self.act(UserAction::Lock),
            Some(Mode::Help) => self.act(UserAction::Overlay { overlay: None }),
            Some(Mode::Selector) if ctrl && key.code == KeyCode::Char('h') => {
                self.act(UserAction::Overlay {
                    overlay: Some(Overlay::Help),
                })
            }
            Some(Mode::Selector) => self.selector_key(key.code),
            Some(Mode::Quit) => return self.quit_key(key.code),
            Some(Mode::Locked) => self.locked_key(key),
            _ if ctrl && key.code == KeyCode::Char('h') => self.act(UserAction::Overlay {
                overlay: Some(Overlay::Help),
            }),
            Some(Mode::Draft) => self.draft_key(key.code),
            None => self.pane_key(key, pty_inner),
        }
        ControlFlow::Continue(())
    }

    /// An action whose error, if any, needs no notice.
    fn act(&mut self, action: UserAction) {
        let _ = self.tab.act(action);
    }

    /// The startup wizard takes every key, F12 included, until answered or put off.
    fn wizard_key(&mut self, code: KeyCode) {
        let laura = match code {
            KeyCode::Char('y') => Some(true),
            KeyCode::Char('n') => Some(false),
            KeyCode::Esc => None,
            _ => return,
        };
        self.act(UserAction::Theme { laura });
    }

    /// Everything to the focused shell or editor pane (else the shell); only F12 is intercepted.
    fn locked_key(&mut self, key: KeyEvent) {
        if let Some(bytes) = key_to_bytes(key.code, key.modifiers) {
            let _ = self.tab.act(UserAction::Input {
                pane: key_target(&self.tab),
                bytes,
            });
        }
    }

    /// `y` quits; any other keystroke cancels.
    fn quit_key(&mut self, code: KeyCode) -> ControlFlow<()> {
        if code == KeyCode::Char('y') {
            return ControlFlow::Break(());
        }
        self.act(UserAction::Overlay { overlay: None });
        ControlFlow::Continue(())
    }

    /// The draft's text editing; Enter confirms, Esc cancels.
    fn draft_key(&mut self, code: KeyCode) {
        let ed = &mut self.text;
        match code {
            KeyCode::Char(c) => ed.insert(c.encode_utf8(&mut [0; 4])),
            KeyCode::Backspace => ed.backspace(),
            KeyCode::Delete => ed.delete(),
            KeyCode::Left => ed.left(),
            KeyCode::Right => ed.right(),
            KeyCode::Home => ed.home(),
            KeyCode::End => ed.end(),
            // `\`+Enter inserts a newline (Shift+Enter isn't portable); plain Enter submits.
            KeyCode::Enter if ed.continue_line() => {}
            KeyCode::Enter => self.confirm_draft(),
            KeyCode::Esc => self.cancel_draft(),
            _ => {}
        }
    }

    /// Enter: add the comment, or submit the inline review. Laura closes the draft; a failure keeps
    /// it, and its text, for a retry.
    fn confirm_draft(&mut self) {
        let text = self.text.text.clone();
        let res = match self.tab.draft() {
            Some(&Draft::Comment { pane, line, .. }) => {
                self.tab.act(UserAction::Comment { pane, line, text })
            }
            Some(&Draft::Review { pane }) => self.tab.act(UserAction::Submit { pane, body: text }),
            None => return,
        };
        match res {
            Err(t) => {
                let until = Instant::now() + Duration::from_secs(5);
                self.toast = Some((t.to_string(), until, Some(t)));
            }
            Ok(_) => {
                self.text = Editor::default();
                // A retry that lands must not leave "Enter to retry" up.
                if submit_failed(&self.toast) {
                    self.toast = None;
                }
            }
        }
    }

    fn cancel_draft(&mut self) {
        self.act(UserAction::CancelDraft);
        self.text = Editor::default();
        // Nothing left to retry, so "Enter to retry" would lie.
        if submit_failed(&self.toast) {
            self.toast = None;
        }
    }

    /// Typed digits are the pane id (shell is #0), matching `laura close`/`focus`. Commit as soon
    /// as no larger id could still be typed, else wait for `Enter`.
    fn selector_key(&mut self, code: KeyCode) {
        let ids = self.tab.layout.order();
        let done = match code {
            KeyCode::Char(c) if c.is_ascii_digit() => {
                self.typed.push(c);
                !pane_id_ambiguous(&ids, &self.typed)
            }
            KeyCode::Backspace => {
                self.typed.pop();
                return;
            }
            KeyCode::Enter => true,
            // Esc or anything else dismisses.
            _ => return self.act(UserAction::Overlay { overlay: None }),
        };
        if done {
            if let Some(id) = pane_id_exact(&ids, &self.typed) {
                self.act(UserAction::Focus { pane: id });
            }
            self.act(UserAction::Overlay { overlay: None });
        }
    }

    /// A keystroke that no mode takes: the workspace's keystrokes, then the focused pane's.
    fn pane_key(&mut self, key: KeyEvent, pty_inner: Rect) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let focus = self.tab.focus;
        if ctrl && key.code == KeyCode::Char('p') {
            self.typed.clear();
            self.act(UserAction::Overlay {
                overlay: Some(Overlay::Selector),
            });
        } else if ctrl && key.code == KeyCode::Char('q') {
            // `Ok` opens the quit question.
            if let Err(e) = self.tab.act(UserAction::Quit) {
                self.notice(e.to_string(), 5);
            }
        } else if ctrl
            && matches!(key.code, KeyCode::Char('l') | KeyCode::Char('L'))
            && self.tab.editors.contains_key(&focus)
        {
            let _ = self.tab.act(UserAction::ToggleEditorView { pane: focus });
        } else if focus != PTY_PANE && focused_pty(&self.tab).is_some() {
            // An editor pane takes every other keystroke, Esc and PageUp/PageDown included.
            if let Some(bytes) = key_to_bytes(key.code, key.modifiers) {
                let _ = self.tab.act(UserAction::Input { pane: focus, bytes });
            }
        } else if focus != PTY_PANE {
            self.panel_key(key);
        } else {
            self.shell_key(key, pty_inner);
        }
    }

    /// A file pane is focused: arrows move its cursor, c/S draft, Esc leaves.
    fn panel_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let pane = self.tab.focus;
        let tab = &mut self.tab;
        let res = match key.code {
            KeyCode::Up => tab.act(UserAction::MoveCursor { pane, delta: -1 }),
            KeyCode::Down => tab.act(UserAction::MoveCursor { pane, delta: 1 }),
            KeyCode::Left => tab.act(UserAction::ScrollH { pane, delta: -1 }),
            KeyCode::Right => tab.act(UserAction::ScrollH { pane, delta: 1 }),
            KeyCode::Char('d') => tab.act(UserAction::ToggleDiff { pane }),
            KeyCode::Char('p') => tab.act(UserAction::TogglePause { pane }),
            KeyCode::Char('c') => tab.act(UserAction::BeginComment { pane }).map(|seed| {
                self.text = Editor::new(seed.unwrap_or_default());
                None
            }),
            KeyCode::Char('r') if ctrl => tab.act(UserAction::Refresh { pane }),
            KeyCode::Char('r') => tab.act(UserAction::ToggleThread { pane, line: None }),
            KeyCode::Char('n') | KeyCode::Char('N') => tab.act(UserAction::JumpThread {
                pane,
                next: key.code == KeyCode::Char('n'),
            }),
            KeyCode::Char('S') => tab
                .act(UserAction::BeginReview { pane })
                .inspect(|_| self.text = Editor::default()),
            KeyCode::Char('x') => tab.act(UserAction::Close { pane }),
            KeyCode::Char('h') => tab.act(UserAction::ClearHighlight { pane }),
            KeyCode::Esc => tab.act(UserAction::Focus { pane: PTY_PANE }),
            _ => Ok(None),
        };
        if let Err(t) = res {
            self.notice(t.to_string(), 5);
        }
    }

    /// The shell is focused. The main screen scrolls Laura's own scrollback with PageUp/PageDown;
    /// on the alt screen the child owns its history, so the key goes to it.
    fn shell_key(&mut self, key: KeyEvent, pty_inner: Rect) {
        let page = pty_inner.height as isize;
        match key.code {
            KeyCode::PageUp if !self.tab.pty.on_alt_screen() => self.tab.pty.scroll(page),
            KeyCode::PageDown if !self.tab.pty.on_alt_screen() => self.tab.pty.scroll(-page),
            _ => {
                if let Some(bytes) = key_to_bytes(key.code, key.modifiers) {
                    let _ = self.tab.act(UserAction::Input {
                        pane: PTY_PANE,
                        bytes,
                    });
                }
            }
        }
    }

    /// A pasted block arrives as one unit: into a draft body, or to the shell wrapped in
    /// bracketed paste (no trailing CR) so a REPL doesn't submit per newline. An overlay, or a
    /// focused panel without a draft, drops it.
    fn on_paste(&mut self, s: String) {
        match self.tab.mode() {
            // A draft takes a paste under the help or the quit question; under the lock, the shell does.
            _ if self.tab.draft().is_some() && !self.tab.modes().contains(&Mode::Locked) => {
                return self.text.insert(&s);
            }
            Some(Mode::Locked) => {}
            None if focused_pty(&self.tab).is_some() => {}
            _ => return,
        }
        let pane = key_target(&self.tab);
        self.act(UserAction::Paste { pane, text: s });
    }
}

/// The terminal is smaller than the workspace's minimum: one line that says so, centered.
fn draw_too_small(f: &mut Frame) {
    // One more row than the characters need, for the words the wrap moves down.
    let (n, w) = (view::TOO_SMALL.width() as u16, f.area().width.max(1));
    let h = n.div_ceil(w) + u16::from(w < n);
    let [line] = Layout::vertical([Constraint::Length(h)])
        .flex(Flex::Center)
        .areas(f.area());
    f.render_widget(
        Paragraph::new(view::TOO_SMALL)
            .centered()
            .wrap(Wrap { trim: true }),
        line,
    );
}

/// Each pane at its rect: the shell's and each editor view's screen, else the file pane.
fn draw_panes(f: &mut Frame, tab: &Tab, rects: &HashMap<PaneId, Rect>) {
    for (id, rect) in rects {
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
                let block = pane_block(Some(view::pane_title(*id, panel, Some(e))), focused);
                f.render_widget(block, *rect);
                e.pty.with_screen(|s| {
                    f.render_widget(PseudoTerminal::new(s), rect.inner(Margin::new(1, 1)))
                });
            } else {
                render_panel(f, *rect, *id, panel, focused, editor);
            }
        }
    }
}

/// Mouse: a left drag on the border between two panes resizes them. Else a child in SGR mouse
/// mode gets real events, the wheel scrolls history/cursor, and a plain left drag selects within
/// the pane. Shift+drag is the terminal's own reserved gesture (native, whole-window selection).
/// `drag_frame`: the frame that showed the selection. `Err`: the notice for a resize.
fn on_mouse(
    tab: &mut Tab,
    m: MouseEvent,
    content: Rect,
    rect_map: &HashMap<PaneId, Rect>,
    pty_inner: Rect,
    pointer: &mut Pointer,
    drag_frame: Option<&Buffer>,
) -> Result<(), laura::tab::Error> {
    if let Some(resized) = on_split(tab, &m, content, pointer) {
        return resized;
    }
    let over = pane_at_point(rect_map, m.column, m.row);
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
            _ if left => mouse::update_selection(&mut pointer.selection, &m, over, rect_map),
            _ => {}
        }
    }

    // Finish a selection on release: copy its glyphs (OSC 52 to our own stdout), then clear.
    if matches!(m.kind, MouseEventKind::Up(MouseButton::Left))
        && let Some(sel) = pointer.selection.take()
        && sel.anchor != sel.head
        && let Some(buf) = drag_frame
        && let Some(rect) = rect_map.get(&sel.pane)
    {
        copy_selection(tab, &sel, buf, rect.inner(Margin::new(1, 1)));
    }
    Ok(())
}

/// A left press on the border between two panes starts a drag of their split, and a second press
/// on the same border within `DOUBLE_CLICK`, with no drag between, gives the size back to Laura. `None`: the event isn't
/// the split's.
fn on_split(
    tab: &mut Tab,
    m: &MouseEvent,
    content: Rect,
    pointer: &mut Pointer,
) -> Option<Result<(), laura::tab::Error>> {
    match m.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let handles = tab.handles(content);
            let h = mouse::handle_at(&handles, m.column, m.row)?;
            let (a, b) = (h.first[0], h.second[0]);
            if pointer.press_line((a, b), Instant::now()) {
                return Some(tab.act(UserAction::Unpin { a, b }).map(drop));
            }
            pointer.split = Some((a, b));
            Some(Ok(()))
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            let (a, b) = pointer.split?;
            let handles = tab.handles(content);
            // A pane closed during the drag: the line is gone, so the drag ends, as in the web app.
            let Some(h) = handles.iter().find(|h| (h.first[0], h.second[0]) == (a, b)) else {
                pointer.split = None;
                return Some(Ok(()));
            };
            let ratio = mouse::ratio_at(h, m.column, m.row);
            pointer.moved_line();
            Some(tab.act(UserAction::Resize { a, b, ratio }).map(drop))
        }
        MouseEventKind::Up(MouseButton::Left) => pointer.split.take().map(|_| Ok(())),
        _ => None,
    }
}

/// Copy the glyphs `sel` covers in `buf` to the clipboard. Panels draw a gutter + wrap the
/// source, so copy source-aware; the PTY owns its own glyphs (no gutter, child wraps) → scrape as-is.
fn copy_selection(tab: &Tab, sel: &Selection, buf: &Buffer, inner: Rect) {
    let panel = tab
        .panels
        .get(&sel.pane)
        .filter(|_| tab.pane_pty(sel.pane).is_none());
    let text = match panel {
        Some(panel) => {
            let layout = panel.layout(inner.width as usize);
            let off = panel.scroll_offset(&layout, inner.height as usize);
            let line_at: Vec<Option<usize>> = layout
                .rows
                .iter()
                .map(|r| (!r.comment).then_some(r.line))
                .collect();
            let gutter = layout.text_col() as u16;
            mouse::extract_panel(buf, inner, gutter, off, &line_at, sel.anchor, sel.head)
        }
        None => mouse::extract(buf, inner, sel.anchor, sel.head),
    };
    if !text.is_empty() {
        use std::io::Write;
        let mut out = std::io::stdout();
        let _ = out.write_all(&mouse::osc52(&text));
        let _ = out.flush();
        #[cfg(target_os = "macos")]
        mouse::pbcopy_in_order(text);
    }
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
            tab.shell_name()
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
/// is the settings file, named so the user can remove the answer from it to be asked again.
fn render_wizard(f: &mut Frame, answer: &Path) {
    let lines = vec![
        Line::raw("Laura can open your files in editor panes via Neovim."),
        Line::raw("Would you like to adopt Laura's theme while running"),
        Line::raw("Neovim inside Laura?"),
        Line::raw(""),
        Line::raw("The theme applies only inside Laura; your Neovim config"),
        Line::raw("stays as it is."),
        Line::raw(""),
        Line::raw("If you change your mind, remove editor_theme from").dim(),
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
