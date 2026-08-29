//! The draw loop and its widgets: hosts the tabs, renders panes, and routes input to the engine.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use portable_pty::CommandBuilder;
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};
use ratatui::layout::{Constraint, Flex, Layout, Margin, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Tabs,
};
use tui_term::widget::PseudoTerminal;

use laura::protocol::PTY_PANE;
use laura::{Panel, Tab, bracketed_paste};

use crate::keys::key_to_bytes;

/// What `panel_focus` typing captures: a per-line comment or the review body. Only Enter branches (add comment vs. inject the review).
enum Draft {
    Comment(String),
    Review(String),
}

impl Draft {
    fn body_mut(&mut self) -> &mut String {
        match self {
            Draft::Comment(s) | Draft::Review(s) => s,
        }
    }
}

pub fn run(terminal: &mut ratatui::DefaultTerminal, program: Vec<String>) -> Result<()> {
    let area = terminal.size()?;

    let mut tabs = vec![Tab::spawn(build_cmd(&program), area.height, area.width)?];
    let mut active = 0usize;
    // `^p` opens the panes popup (a digit focuses a pane).
    let mut panes = false;
    // `^t` toggles tab-nav in the footer (←/→ browse, n new); no popup.
    let mut tab_nav = false;
    // `F12` locks input: every key goes to the shell, Laura intercepts nothing but F12.
    let mut locked = false;
    // `^q` arms a quit confirm; the next key must be `y`.
    let mut confirm_quit = false;
    // Draft captures typing for a comment or the review body.
    let mut draft: Option<Draft> = None;
    let mut help = false;

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
        if a.focus == PTY_PANE {
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

        let titles: Vec<String> = (1..=tabs.len()).map(|i| format!(" {i} ")).collect();
        terminal.draw(|f| {
            let rows = Layout::vertical([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(f.area());
            f.render_widget(Tabs::new(titles).select(active), rows[0]);
            let tab = &tabs[active];
            let map = laura::rects(&tab.layout, rows[1]);
            for (id, rect) in &map {
                let focused = tab.focus == *id;
                if *id == PTY_PANE {
                    let inner = rect.inner(Margin::new(1, 1));
                    f.render_widget(pane_block(None, focused), *rect);
                    tab.pty
                        .with_screen(|s| f.render_widget(PseudoTerminal::new(s), inner));
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
                    Draft::Comment(text) => {
                        let cursor = tab.focused_panel().map_or(0, |p| p.cursor);
                        format!("  comment L{}: {text}", cursor + 1)
                    }
                    Draft::Review(body) => format!("  overall (Enter submits): {body}"),
                };
                focus_hint.as_str()
            } else if panes {
                "  Esc dismiss"
            } else if tab_nav {
                "  ←/→ tabs · n new tab · x close tab · Esc dismiss"
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
            if panes {
                render_panes(f, tab);
            }
            if help {
                render_help(f);
            }
        })?;

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
            match event::read()? {
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
                            KeyCode::Enter => match draft.take().unwrap() {
                                Draft::Comment(text) => {
                                    if let Some(p) = tabs[active].focused_panel_mut() {
                                        p.add_comment(text);
                                    }
                                }
                                Draft::Review(body) => {
                                    // Assemble + clear comments borrowing the panel, then inject (disjoint from `pty`).
                                    let payload = tabs[active].focused_panel_mut().map(|p| {
                                        let bytes = bracketed_paste(&p.assemble_review(&body));
                                        p.comments.clear();
                                        bytes
                                    });
                                    if let Some(payload) = payload {
                                        tabs[active].pty.write(&payload);
                                    }
                                    tabs[active].focus = PTY_PANE;
                                }
                            },
                            KeyCode::Esc => draft = None,
                            _ => {}
                        }
                    } else if panes {
                        match key.code {
                            KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                                let label = c as usize - '0' as usize;
                                if let Some(id) = laura::pane_at(&tabs[active].layout, label) {
                                    tabs[active].focus = id;
                                }
                                panes = false;
                            }
                            _ => panes = false, // Esc or anything else dismisses
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
                            _ => tab_nav = false, // Esc or anything else dismisses
                        }
                    } else if ctrl && key.code == KeyCode::Char('p') {
                        panes = true;
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
                    } else if key.code == KeyCode::PageUp {
                        tabs[active].pty.scroll(pty_inner.height as isize);
                    } else if key.code == KeyCode::PageDown {
                        tabs[active].pty.scroll(-(pty_inner.height as isize));
                    } else if let Some(bytes) = key_to_bytes(key.code, key.modifiers) {
                        tabs[active].pty.to_live(); // typing snaps to the live prompt
                        tabs[active].pty.write(&bytes);
                    }
                }
                // The resize is applied by `resize_to` at the top of the loop from the content-inner
                // rect; resizing the PTY to the raw terminal size here would clip the bottom rows.
                Event::Resize(..) => {}
                // Wheel routes by pointer: over a panel moves its cursor; over the PTY scrolls history.
                Event::Mouse(m) => {
                    let step = match m.kind {
                        MouseEventKind::ScrollUp => -3,
                        MouseEventKind::ScrollDown => 3,
                        _ => 0,
                    };
                    if step != 0 {
                        match pane_at_point(&rect_map, m.column, m.row) {
                            Some(PTY_PANE) | None => tabs[active].pty.scroll(-step as isize),
                            Some(id) => {
                                if let Some(p) = tabs[active].panels.get_mut(&id) {
                                    p.move_cursor(step as isize);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
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
    let cursor_end = layout
        .starts
        .get(panel.cursor + 1)
        .map(|n| n - 1)
        .unwrap_or(total_rows.saturating_sub(1));
    let offset = cursor_end
        .saturating_sub(view.saturating_sub(1))
        .min(u16::MAX as usize) as u16;
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
    let mut sb = ScrollbarState::new(total)
        .viewport_content_length(view)
        .position(pos);
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

/// Draw the `^p` panes popup: panes numbered positionally `1..N` with their stable ids; a digit focuses one.
fn render_panes(f: &mut Frame, tab: &Tab) {
    let key = |k: String, desc: String| {
        Line::from(vec![
            Span::raw("  "),
            k.bold().cyan(),
            Span::raw("  "),
            desc.dim(),
        ])
    };
    let mut lines = vec![];
    for (i, id) in tab.layout.order().into_iter().enumerate() {
        let name = if id == PTY_PANE {
            "shell".to_string()
        } else {
            tab.panels
                .get(&id)
                .map(|p| base_name(&p.path))
                .unwrap_or_else(|| "?".into())
        };
        lines.push(key(format!("{}", i + 1), format!("{name}  #{id}")));
    }
    lines.push(Line::raw(""));
    lines.push(key("digit".into(), "focus that pane".into()));
    lines.push(key("Esc".into(), "dismiss".into()));

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
        key("⇧drag", "select text, then ^c to copy"),
        key("^q", "quit (then y to confirm)"),
        Line::raw(""),
        group("Panes (^p …)"),
        key("digit", "focus that pane"),
        key("Esc", "dismiss"),
        Line::raw(""),
        group("Tabs (^t …)"),
        key("←/→", "browse tabs"),
        key("n", "new tab"),
        key("x", "close tab"),
        key("Esc", "dismiss"),
        Line::raw(""),
        group("Panel focus"),
        key("↑/↓", "move cursor"),
        key("c", "comment on line (needs `laura ready`)"),
        key("S", "submit review (needs `laura ready`)"),
        key("Esc", "leave focus"),
        Line::raw(""),
        group("Draft"),
        key("Enter", "confirm"),
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
