//! The draw loop and its widgets: hosts the tabs, renders panes, and routes input to the engine.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use portable_pty::CommandBuilder;
use ratatui::Frame;
use ratatui::crossterm::event::{
    self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::layout::{Constraint, Flex, Layout, Margin, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use tui_term::widget::PseudoTerminal;

use laura::protocol::PTY_PANE;
use laura::{Panel, Tab, bracketed_paste, wrap_line};
use serde_json::json;

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

/// What live typing captures: a per-line comment, the review body, or a tab rename. Enter branches per variant.
enum Draft {
    Comment(String),
    Review(String),
    Rename(String),
}

impl Draft {
    fn body_mut(&mut self) -> &mut String {
        match self {
            Draft::Comment(s) | Draft::Review(s) | Draft::Rename(s) => s,
        }
    }

    fn body(&self) -> &str {
        match self {
            Draft::Comment(s) | Draft::Review(s) | Draft::Rename(s) => s,
        }
    }

    /// Comment/Review are tied to the focused panel; Rename is not.
    fn is_panel(&self) -> bool {
        matches!(self, Draft::Comment(_) | Draft::Review(_))
    }

    /// `\`+Enter inserts a newline; Rename stays single-line.
    fn multiline(&self) -> bool {
        self.is_panel()
    }
}

pub fn run(terminal: &mut ratatui::DefaultTerminal, program: Vec<String>) -> Result<()> {
    let area = terminal.size()?;

    let mut tabs = vec![Tab::spawn(build_cmd(&program), area.height, area.width)?];
    let mut active = 0usize;
    // `^p` opens the panes popup; `Some(buf)` holds the pane id being typed (multi-digit for #10+).
    let mut panes: Option<String> = None;
    // `^t` toggles tab-nav in the footer (←/→ browse, n new); no popup.
    let mut tab_nav = false;
    // `F12` locks input: every key goes to the shell, Laura intercepts nothing but F12.
    let mut locked = false;
    // `^q` arms a quit confirm; the next key must be `y`.
    let mut confirm_quit = false;
    // Draft captures typing for a comment or the review body.
    let mut draft: Option<Draft> = None;
    let mut help = false;
    // An in-progress left-drag selection; local because a drag can't span a tab switch.
    let mut selection: Option<Selection> = None;

    loop {
        // Content rect sits below the 1-line tab bar; sockets deliver while unfocused, so drain every tab.
        let content = content_rect(terminal.get_frame().area());
        for tab in tabs.iter_mut() {
            tab.drain(content);
        }
        // A just-drained `open` requests focus; keep focus valid if a pane vanished.
        let a = &mut tabs[active];
        if let Some(id) = a.pending_focus.take() {
            a.focus = id;
        }
        if a.focus != PTY_PANE && !a.panels.contains_key(&a.focus) {
            a.focus = PTY_PANE;
        }
        // A panel draft is void once no panel is focused; a Rename draft survives (focus is the shell).
        if a.focus == PTY_PANE && draft.as_ref().is_some_and(Draft::is_panel) {
            draft = None;
        }

        // One rect map per frame — render, resize, wheel and popups all read it.
        let rect_map = laura::rects(&tabs[active].layout, content);
        let pty_inner = rect_map
            .get(&PTY_PANE)
            .copied()
            .unwrap_or(content)
            .inner(Margin::new(1, 1)); // shell sits inside its border box
        tabs[active].resize_to(pty_inner.height, pty_inner.width);

        let tab_labels: Vec<String> = tabs
            .iter()
            .enumerate()
            .map(|(i, t)| match &t.name {
                Some(n) => format!(" {}:{n} ", i + 1),
                None => format!(" {} ", i + 1),
            })
            .collect();
        let completed = terminal.draw(|f| {
            let rows = Layout::vertical([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(f.area());
            f.render_widget(tab_bar(&tab_labels, active, rows[0].width), rows[0]);
            let tab = &tabs[active];
            let map = laura::rects(&tab.layout, rows[1]);
            for (id, rect) in &map {
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
                    render_panel(f, *rect, panel, focused);
                }
            }
            let focus_hint;
            let hint = if locked {
                "  🔒 locked — every key goes to the shell · F12 unlock"
            } else if confirm_quit {
                "  quit? press y to confirm · any other key cancels"
            } else if let Some(d) = &draft {
                focus_hint = match d {
                    Draft::Comment(_) => {
                        let cursor = tab.focused_panel().map_or(0, |p| p.cursor);
                        format!(
                            "  comment L{} · \\+Enter newline · Enter add · Esc cancel",
                            cursor + 1
                        )
                    }
                    Draft::Review(_) => {
                        "  overall · \\+Enter newline · Enter submit · Esc cancel".into()
                    }
                    Draft::Rename(_) => "  rename tab · Enter save · Esc cancel".into(),
                };
                focus_hint.as_str()
            } else if panes.is_some() {
                "  type a pane id · Enter pick · Esc dismiss"
            } else if tab_nav {
                "  ←/→ tabs · n new tab · x close tab · r rename tab · Esc dismiss"
            } else if tab.focus != PTY_PANE {
                if tab.agent {
                    "  ↑/↓ move · c comment · S submit · Esc leave focus"
                } else {
                    "  ↑/↓ move · Esc leave focus · review: run `laura ready`"
                }
            } else {
                "  ^p panes · ^t tabs · ^h help · ^q quit"
            };
            f.render_widget(Paragraph::new(hint).dim(), rows[2]);
            // A live draft grows a bordered input box over the bottom of the shell pane — never over
            // a panel, so the file under review stays visible.
            if let Some(d) = &draft
                && let Some(pty_rect) = map.get(&PTY_PANE)
            {
                let (title, body) = match d {
                    Draft::Comment(s) => ("comment", s.as_str()),
                    Draft::Review(s) => ("overall", s.as_str()),
                    Draft::Rename(s) => ("rename tab", s.as_str()),
                };
                render_draft_box(f, *pty_rect, title, body);
            }
            if let Some(buf) = &panes {
                render_panes(f, tab, buf);
            }
            if help {
                render_help(f);
            }
            // Reverse-video the drag selection over whatever was just drawn (PTY or panel, one path).
            if let Some(sel) = &selection
                && let Some(rect) = map.get(&sel.pane)
            {
                let inner = rect.inner(Margin::new(1, 1));
                mouse::highlight(f.buffer_mut(), inner, sel.anchor, sel.head);
            }
        })?;
        // The post-draw buffer swap resets `current_buffer`, so copy-on-release must read the frame
        // that just showed the selection — snapshot it here while a drag is live, not the empty next one.
        let drag_frame = selection.is_some().then(|| completed.buffer.clone());

        // A tab closes when its shell exits; when the last one goes, quit.
        if let Some(dead) = tabs.iter().position(|t| t.pty.has_exited()) {
            tabs.remove(dead);
            if tabs.is_empty() {
                break;
            }
            active = active.min(tabs.len() - 1);
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
                    if key.code == KeyCode::F(12) {
                        locked = !locked; // the one key Laura keeps even while locked
                    } else if locked {
                        // Everything to the shell; only F12 (handled above) is intercepted.
                        if let Some(bytes) = key_to_bytes(key.code, key.modifiers) {
                            tabs[active].pty.to_live();
                            tabs[active].pty.write(&bytes);
                        }
                    } else if confirm_quit {
                        if key.code == KeyCode::Char('y') {
                            break;
                        }
                        confirm_quit = false; // any other key cancels
                    } else if help {
                        help = false;
                    } else if ctrl && key.code == KeyCode::Char('h') {
                        help = true;
                    } else if draft.is_some() {
                        match key.code {
                            KeyCode::Char(c) => draft.as_mut().unwrap().body_mut().push(c),
                            KeyCode::Backspace => {
                                draft.as_mut().unwrap().body_mut().pop();
                            }
                            // `\`+Enter inserts a newline (Shift+Enter isn't portable); plain Enter submits.
                            KeyCode::Enter
                                if draft.as_ref().unwrap().multiline()
                                    && draft.as_ref().unwrap().body().ends_with('\\') =>
                            {
                                let b = draft.as_mut().unwrap().body_mut();
                                b.pop();
                                b.push('\n');
                            }
                            KeyCode::Enter => match draft.take().unwrap() {
                                Draft::Comment(text) => {
                                    if let Some(p) = tabs[active].focused_panel_mut() {
                                        p.add_comment(text);
                                    }
                                }
                                Draft::Review(body) => {
                                    // Assemble + clear comments borrowing the panel, then inject (disjoint from `pty`).
                                    let logged = tabs[active]
                                        .focused_panel()
                                        .map(|p| (p.path.clone(), p.comments.len()));
                                    let payload = tabs[active].focused_panel_mut().map(|p| {
                                        let bytes =
                                            bracketed_paste(&p.assemble_review(&body), true);
                                        p.comments.clear();
                                        bytes
                                    });
                                    if let Some(payload) = payload {
                                        tabs[active].pty.write(&payload);
                                    }
                                    if let Some((path, comments)) = logged {
                                        tabs[active].log_event(json!({
                                            "type": "review",
                                            "path": path,
                                            "comments": comments,
                                            "body": body,
                                        }));
                                    }
                                    tabs[active].focus = PTY_PANE;
                                }
                                Draft::Rename(text) => {
                                    let text = text.trim();
                                    tabs[active].name =
                                        (!text.is_empty()).then(|| text.to_string());
                                }
                            },
                            KeyCode::Esc => draft = None,
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
                                let ids = tabs[active].layout.order();
                                if !pane_id_ambiguous(&ids, &buf) {
                                    if let Some(id) = pane_id_exact(&ids, &buf) {
                                        tabs[active].focus = id;
                                    }
                                    panes = None;
                                }
                            }
                            KeyCode::Backspace => {
                                panes.as_mut().expect("popup open").pop();
                            }
                            KeyCode::Enter => {
                                let buf = panes.take().expect("popup open");
                                let ids = tabs[active].layout.order();
                                if let Some(id) = pane_id_exact(&ids, &buf) {
                                    tabs[active].focus = id;
                                }
                            }
                            _ => panes = None, // Esc or anything else dismisses
                        }
                    } else if tab_nav {
                        match key.code {
                            KeyCode::Right => active = (active + 1) % tabs.len(),
                            KeyCode::Left => active = (active + tabs.len() - 1) % tabs.len(),
                            KeyCode::Char('n') => {
                                tabs.push(Tab::spawn(default_shell(), area.height, area.width)?);
                                active = tabs.len() - 1;
                                tab_nav = false;
                            }
                            KeyCode::Char('x') => {
                                tabs.remove(active); // drop kills its shell
                                if tabs.is_empty() {
                                    break;
                                }
                                active = active.min(tabs.len() - 1);
                                tab_nav = false;
                            }
                            KeyCode::Char('r') => {
                                let cur = tabs[active].name.clone().unwrap_or_default();
                                draft = Some(Draft::Rename(cur));
                                tab_nav = false;
                            }
                            _ => tab_nav = false, // Esc or anything else dismisses
                        }
                    } else if ctrl && key.code == KeyCode::Char('p') {
                        panes = Some(String::new());
                    } else if ctrl && key.code == KeyCode::Char('t') {
                        tab_nav = true;
                    } else if ctrl && key.code == KeyCode::Char('q') {
                        confirm_quit = true;
                    } else if tabs[active].focus != PTY_PANE {
                        // A panel is focused: arrows move its cursor, c/S draft, Esc leaves.
                        match key.code {
                            KeyCode::Up => {
                                if let Some(p) = tabs[active].focused_panel_mut() {
                                    p.move_cursor(-1)
                                }
                            }
                            KeyCode::Down => {
                                if let Some(p) = tabs[active].focused_panel_mut() {
                                    p.move_cursor(1)
                                }
                            }
                            KeyCode::Left => {
                                if let Some(p) = tabs[active].focused_panel_mut() {
                                    p.scroll_h(-1)
                                }
                            }
                            KeyCode::Right => {
                                if let Some(p) = tabs[active].focused_panel_mut() {
                                    p.scroll_h(1)
                                }
                            }
                            KeyCode::Char('c') if tabs[active].agent => {
                                draft = Some(Draft::Comment(String::new()))
                            }
                            KeyCode::Char('S')
                                if tabs[active].agent
                                    && tabs[active]
                                        .focused_panel()
                                        .is_some_and(|p| !p.comments.is_empty()) =>
                            {
                                draft = Some(Draft::Review(String::new()))
                            }
                            KeyCode::Esc => tabs[active].focus = PTY_PANE,
                            _ => {}
                        }
                    } else if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) {
                        // On the alt screen the child owns its history — forward the key so it scrolls
                        // itself; on the main screen scroll Laura's own scrollback.
                        if tabs[active].pty.on_alt_screen() {
                            if let Some(bytes) = key_to_bytes(key.code, key.modifiers) {
                                tabs[active].pty.write(&bytes);
                            }
                        } else if key.code == KeyCode::PageUp {
                            tabs[active].pty.scroll(pty_inner.height as isize);
                        } else {
                            tabs[active].pty.scroll(-(pty_inner.height as isize));
                        }
                    } else if let Some(bytes) = key_to_bytes(key.code, key.modifiers) {
                        tabs[active].pty.to_live(); // typing snaps to the live prompt
                        tabs[active].pty.write(&bytes);
                    }
                }
                // The resize is applied by `resize_to` at the top of the loop from the content-inner
                // rect; resizing the PTY to the raw terminal size here would clip the bottom rows.
                Event::Resize(..) => {}
                // A pasted block arrives as one unit: into a draft body, or to the shell wrapped in
                // bracketed paste (no trailing CR) so a REPL doesn't submit per newline.
                Event::Paste(s) => {
                    if let Some(d) = draft.as_mut() {
                        // Rename is single-line: an interior newline can't land in the tab name.
                        if d.multiline() {
                            d.body_mut().push_str(&s);
                        } else {
                            d.body_mut().push_str(&s.replace('\n', " "));
                        }
                    } else if locked
                        || (tabs[active].focus == PTY_PANE
                            && panes.is_none()
                            && !tab_nav
                            && !help
                            && !confirm_quit)
                    {
                        tabs[active].pty.to_live();
                        tabs[active].pty.write(&bracketed_paste(&s, false));
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

                    if let Some(mode) = (over == Some(PTY_PANE))
                        .then(|| tabs[active].pty.mouse_capture())
                        .flatten()
                    {
                        // Forward to the child; no `to_live` — forwarding mustn't disturb the view.
                        if let Some(bytes) = mouse::sgr_mouse_bytes(&m, pty_inner, mode) {
                            tabs[active].pty.write(&bytes);
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
                                    Some(PTY_PANE) | None => tabs[active].pty.scroll(-step),
                                    Some(id) => {
                                        if let Some(p) = tabs[active].panels.get_mut(&id) {
                                            p.move_cursor(step);
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
                        let text = match tabs[active].panels.get(&sel.pane) {
                            Some(panel) => {
                                let layout = panel.layout(inner.width as usize);
                                let off = panel.scroll_offset(&layout, inner.height as usize);
                                let line_at: Vec<usize> =
                                    layout.rows.iter().map(|r| r.line).collect();
                                let gutter = layout.gutter_width as u16 + 1;
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

/// The one-line tab bar: numbered (and optionally named) tabs, windowed to `width`, active reversed,
/// with `‹`/`›` markers when clipped either side.
fn tab_bar(labels: &[String], active: usize, width: u16) -> Line<'static> {
    let (lo, hi) = tab_window(labels, active, width as usize);
    let mut spans = vec![];
    if lo > 0 {
        spans.push(Span::raw("‹"));
    }
    for (i, label) in labels.iter().enumerate().take(hi).skip(lo) {
        if i == active {
            spans.push(Span::styled(label.clone(), Style::default().reversed()));
        } else {
            spans.push(Span::raw(label.clone()));
        }
    }
    if hi < labels.len() {
        spans.push(Span::raw("›"));
    }
    Line::from(spans)
}

/// The `[lo, hi)` slice of tab labels to show: always includes `active`, greedily fills `width`,
/// reserving a column for each `‹`/`›` marker when the ends are clipped.
fn tab_window(labels: &[String], active: usize, width: usize) -> (usize, usize) {
    let w = |i: usize| labels[i].chars().count();
    let total: usize = (0..labels.len()).map(w).sum();
    if total <= width {
        return (0, labels.len());
    }
    let (mut lo, mut hi, mut used) = (active, active + 1, w(active));
    loop {
        let reserve = usize::from(lo > 0) + usize::from(hi < labels.len());
        let budget = width.saturating_sub(reserve);
        if hi < labels.len() && used + w(hi) <= budget {
            used += w(hi);
            hi += 1;
        } else if lo > 0 && used + w(lo - 1) <= budget {
            lo -= 1;
            used += w(lo);
        } else {
            break;
        }
    }
    (lo, hi)
}

/// A bordered draft input box pinned to the bottom of `area` (the shell pane). Grows 2..=6 rows,
/// then scrolls to keep the tail visible; wraps like a panel.
fn render_draft_box(f: &mut Frame, area: Rect, title: &str, body: &str) {
    let inner_w = area.width.saturating_sub(2).max(1) as usize;
    let mut rows: Vec<String> = body
        .split('\n')
        .flat_map(|l| wrap_line(l, inner_w))
        .collect();
    if rows.is_empty() {
        rows.push(String::new());
    }
    let view = rows.len().clamp(2, 6);
    let h = view as u16 + 2; // + border
    let box_area = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(h),
        width: area.width,
        height: h.min(area.height),
    };
    let scroll = rows.len().saturating_sub(view) as u16;
    let text: Vec<Line> = rows.into_iter().map(Line::from).collect();
    f.render_widget(Clear, box_area);
    f.render_widget(
        Paragraph::new(text)
            .block(Block::bordered().title(format!(" {title} ")))
            .scroll((scroll, 0)),
        box_area,
    );
}

/// The frame minus the tab bar (top) and hint line (bottom).
fn content_rect(area: Rect) -> Rect {
    Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(area)[1]
}

/// The pane whose rect contains `(col, row)`, if any.
fn pane_at_point(map: &HashMap<laura::PaneId, Rect>, col: u16, row: u16) -> Option<laura::PaneId> {
    map.iter()
        .find(|(_, r)| col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height)
        .map(|(id, _)| *id)
}

/// A pane's border block: focused keeps the default color (plus a `▸ ` title mark on panels), unfocused dims to gray.
fn pane_block(title: Option<String>, focused: bool) -> Block<'static> {
    let mut block = Block::bordered();
    if let Some(t) = title {
        block = block.title(if focused { format!("▸ {t}") } else { t });
    }
    if !focused {
        block = block.border_style(Style::default().fg(Color::Rgb(90, 90, 90)));
    }
    block
}

/// Draw one file panel: pre-wrapped rows, gutter, cursor highlight (when focused), and an overflow scrollbar.
fn render_panel(f: &mut Frame, area: Rect, panel: &Panel, focused: bool) {
    // `Panel::layout` pre-wraps into 1:1 rows; we only style them here.
    let inner_w = area.width.saturating_sub(2) as usize; // minus borders
    let layout = panel.layout(inner_w);
    let gw = layout.gutter_width;
    let rows: Vec<Line> = layout
        .rows
        .iter()
        .map(|r| {
            let gutter = match r.gutter {
                Some(n) => format!("{n:>gw$} "),
                None => format!("{:>gw$} ", ""),
            };
            if r.comment {
                let mut spans = vec![Span::raw(gutter)];
                spans.extend(r.spans.iter().cloned());
                Line::from(spans).dim()
            } else {
                let mut spans = vec![Span::raw(gutter).dim()];
                spans.extend(r.spans.iter().cloned());
                let line = Line::from(spans);
                if focused && r.line == panel.cursor {
                    line.reversed()
                } else {
                    line
                }
            }
        })
        .collect();
    let title = if panel.comments.is_empty() {
        panel.path.clone()
    } else {
        format!("{}  [review: {}]", panel.path, panel.comments.len())
    };
    // Scroll off the cursor line's *last* wrapped row, so its continuations stay on-screen instead of clipped.
    let view = area.height.saturating_sub(2) as usize;
    let total_rows = rows.len();
    let offset = panel.scroll_offset(&layout, view).min(u16::MAX as usize) as u16;
    f.render_widget(
        Paragraph::new(rows)
            .block(pane_block(Some(title), focused))
            .scroll((offset, 0)),
        area,
    );
    render_scrollbar(f, area, total_rows, view, offset as usize);
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

/// The tab's initial command: `program` (from `-- <cmd>`) if given, else the default shell.
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
                .map(|p| base_name(&p.path))
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
    lines.push(key(typed, "Enter pick · Esc dismiss".into()));

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

/// A path's file name (last `/`- or `\`-separated segment).
fn base_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

/// Draw the global help popup: a centered, bordered list of key bindings, mirroring the contextual hints.
fn render_help(f: &mut Frame) {
    let key = |k: &'static str, desc: &'static str| {
        Line::from(vec![
            Span::raw("  "),
            k.bold().cyan(),
            Span::raw("  "),
            desc.dim(),
        ])
    };
    let group = |g: &'static str| Line::from(g.bold());
    let lines = vec![
        group("Global"),
        key("^p", "panes popup"),
        key("^t", "tab nav (footer)"),
        key("^h", "this help"),
        key("F12", "lock all input to the shell"),
        key("drag", "select within pane (copies on release)"),
        key("^q", "quit (then y to confirm)"),
        Line::raw(""),
        group("Panes (^p …)"),
        key("id", "type a pane id, Enter to focus"),
        key("Esc", "dismiss"),
        Line::raw(""),
        group("Tabs (^t …)"),
        key("←/→", "browse tabs"),
        key("n", "new tab"),
        key("x", "close tab"),
        key("r", "rename tab"),
        key("Esc", "dismiss"),
        Line::raw(""),
        group("Panel focus"),
        key("↑/↓", "move cursor"),
        key("c", "comment on line (needs `laura ready`)"),
        key("S", "submit review (needs `laura ready`)"),
        key("Esc", "leave focus"),
        Line::raw(""),
        group("Draft"),
        key("\\+Enter", "newline (comment/review)"),
        key("Enter", "confirm / submit"),
        key("Esc", "cancel"),
    ];
    let width = 44u16;
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

#[cfg(test)]
mod tests {
    use super::{pane_id_ambiguous, pane_id_exact, tab_window};

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

    // Windowing is bin-internal (no CLI/socket surface), so it's checked here per CLAUDE.md's exception.
    #[test]
    fn window_shows_all_when_it_fits() {
        let labels = vec![" 1 ".to_string(), " 2 ".to_string(), " 3 ".to_string()];
        assert_eq!(tab_window(&labels, 0, 80), (0, 3));
    }

    #[test]
    fn window_always_includes_active_and_fits_width() {
        // 10 tabs of 3 cols each = 30; a 12-wide bar can't show them all.
        let labels: Vec<String> = (1..=10).map(|i| format!(" {i} ")).collect();
        let (lo, hi) = tab_window(&labels, 9, 12);
        assert!(lo <= 9 && 9 < hi, "active tab is inside the window");
        // Reserve one col for the left `‹` marker; the rest holds visible labels.
        let shown: usize = labels[lo..hi].iter().map(|s| s.chars().count()).sum();
        assert!(
            shown <= 12 - usize::from(lo > 0),
            "fits the width minus markers"
        );
        assert!(
            hi == labels.len(),
            "the last (active) tab reaches the right edge"
        );
    }
}
