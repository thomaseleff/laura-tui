//! `laura` — host the default shell/agent in a PTY, render it live, forward keystrokes, quit on Ctrl+Q or child exit.

use std::time::Duration;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use portable_pty::CommandBuilder;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Flex, Layout, Margin, Rect};
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Tabs,
};
use tui_term::widget::PseudoTerminal;

use laura::protocol::{self, Message};
use laura::{Tab, bracketed_paste};

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

/// Laura hosts your coding agent's shell in a PTY with a live side-panel for showing files and
/// receiving in-place review.
///
/// Agents: install the skill — `claude plugin marketplace add thomaseleff/laura-tui`.
#[derive(Parser)]
#[command(version, about, args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
    /// Program to host in the tab (after `--`); defaults to your shell.
    #[arg(last = true)]
    program: Vec<String>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Open a file in the panel beside the shell.
    Open {
        path: String,
        /// Don't move focus into the panel.
        #[arg(long)]
        no_focus: bool,
    },
    /// Close the tab's panel.
    Close,
    /// Mark this tab as hosting an agent (enables review submission).
    Ready,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Cmd::Open { path, no_focus }) => client_send(Message::Open {
            path,
            focus: !no_focus,
        }),
        Some(Cmd::Close) => client_send(Message::Close),
        Some(Cmd::Ready) => client_send(Message::Ready),
        None => {
            let mut terminal = ratatui::init();
            // Capture the wheel so panels scroll; the shell gets no mouse anyway (keys only).
            let _ = execute!(std::io::stdout(), EnableMouseCapture);
            let result = run(&mut terminal, cli.program);
            let _ = execute!(std::io::stdout(), DisableMouseCapture);
            ratatui::restore();
            result
        }
    }
}

/// Send one message to the current tab's socket ($LAURA_TAB). Never touches ratatui.
fn client_send(msg: Message) -> Result<()> {
    let Ok(tab) = std::env::var("LAURA_TAB") else {
        bail!("not inside a Laura tab (LAURA_TAB unset)");
    };
    protocol::send(&tab, &msg)
}

fn run(terminal: &mut ratatui::DefaultTerminal, program: Vec<String>) -> Result<()> {
    let area = terminal.size()?;

    let mut tabs = vec![Tab::spawn(build_cmd(&program), area.height, area.width)?];
    let mut active = 0usize;
    // tmux-style leader: `^t` arms it, the next key is a tab command. Only `^t` is intercepted.
    let mut prefix = false;
    // Panel mode: focus captures arrows/`c`, draft captures typing. Outside them every key reaches the PTY.
    let mut panel_focus = false;
    let mut draft: Option<Draft> = None;
    let mut help = false;

    loop {
        // Sockets deliver while unfocused, so drain every tab.
        for tab in tabs.iter_mut() {
            tab.drain();
        }
        // A just-drained `open` requests focus; a `close` (or none) clears it.
        let a = &mut tabs[active];
        if a.pending_focus && a.panel.is_some() {
            panel_focus = true;
            a.pending_focus = false;
        }
        if a.panel.is_none() {
            panel_focus = false;
            draft = None;
        }

        // Content rect sits below the 1-line tab bar; resize/draw only the active tab.
        let content = content_rect(terminal.get_frame().area());
        let active_tab = &mut tabs[active];
        let pty_rect = pty_layout(content, active_tab.panel.is_some()).inner(Margin::new(1, 1)); // shell sits inside its border box
        active_tab.resize_to(pty_rect.height, pty_rect.width);

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
            match &tab.panel {
                None => {
                    let inner = rows[1].inner(Margin::new(1, 1));
                    f.render_widget(Block::bordered(), rows[1]);
                    tab.pty
                        .with_screen(|s| f.render_widget(PseudoTerminal::new(s), inner));
                }
                Some(panel) => {
                    let cols = Layout::horizontal([
                        Constraint::Percentage(50),
                        Constraint::Percentage(50),
                    ])
                    .split(rows[1]);
                    let pty_inner = cols[0].inner(Margin::new(1, 1));
                    f.render_widget(Block::bordered(), cols[0]);
                    tab.pty
                        .with_screen(|s| f.render_widget(PseudoTerminal::new(s), pty_inner));
                    // `Panel::layout` pre-wraps into 1:1 rows; we only style them here.
                    let inner_w = cols[1].width.saturating_sub(2) as usize; // minus borders
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
                                if panel_focus && r.line == panel.cursor {
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
                    let view = cols[1].height.saturating_sub(2) as usize;
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
                            .block(Block::bordered().title(title))
                            .scroll((offset, 0)),
                        cols[1],
                    );
                    // Right-edge scrollbar, only when the doc overflows the panel.
                    if total_rows > view {
                        let mut sb = ScrollbarState::new(total_rows)
                            .viewport_content_length(view)
                            .position(offset as usize);
                        f.render_stateful_widget(
                            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                                .begin_symbol(Some("↑"))
                                .end_symbol(Some("↓")),
                            cols[1].inner(Margin {
                                horizontal: 0,
                                vertical: 1,
                            }),
                            &mut sb,
                        );
                    }
                }
            }
            let focus_hint;
            let hint = if let Some(d) = &draft {
                focus_hint = match d {
                    Draft::Comment(text) => {
                        let cursor = tabs[active].panel.as_ref().map_or(0, |p| p.cursor);
                        format!("  comment L{}: {text}", cursor + 1)
                    }
                    Draft::Review(body) => format!("  overall (Enter submits): {body}"),
                };
                focus_hint.as_str()
            } else if prefix {
                if tabs[active].panel.is_some() {
                    "  ←/→ browse · Enter select · n new tab · p focus panel · (any other key cancels)"
                } else {
                    "  ←/→ browse · Enter select · n new tab · (any other key cancels)"
                }
            } else if panel_focus {
                if tabs[active].agent {
                    "  ↑/↓ move · c comment · S submit · Esc leave focus"
                } else {
                    "  ↑/↓ move · Esc leave focus · review: run `laura ready`"
                }
            } else {
                "  ^t tabs · ^h help · ⇧drag+^c copy · ^q quit"
            };
            f.render_widget(Paragraph::new(hint).dim(), rows[2]);
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
                    if help {
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
                                    if let Some(p) = tabs[active].panel.as_mut() {
                                        p.add_comment(text);
                                    }
                                }
                                Draft::Review(body) => {
                                    // Assemble + clear comments borrowing the panel, then inject (disjoint from `pty`).
                                    let payload = tabs[active].panel.as_mut().map(|p| {
                                        let bytes = bracketed_paste(&p.assemble_review(&body));
                                        p.comments.clear();
                                        bytes
                                    });
                                    if let Some(payload) = payload {
                                        tabs[active].pty.write(&payload);
                                    }
                                    panel_focus = false;
                                }
                            },
                            KeyCode::Esc => draft = None,
                            _ => {}
                        }
                    } else if panel_focus {
                        match key.code {
                            KeyCode::Up => {
                                if let Some(p) = tabs[active].panel.as_mut() {
                                    p.move_cursor(-1)
                                }
                            }
                            KeyCode::Down => {
                                if let Some(p) = tabs[active].panel.as_mut() {
                                    p.move_cursor(1)
                                }
                            }
                            KeyCode::Char('c')
                                if tabs[active].agent && tabs[active].panel.is_some() =>
                            {
                                draft = Some(Draft::Comment(String::new()))
                            }
                            KeyCode::Char('S')
                                if tabs[active].agent
                                    && tabs[active]
                                        .panel
                                        .as_ref()
                                        .is_some_and(|p| !p.comments.is_empty()) =>
                            {
                                draft = Some(Draft::Review(String::new()))
                            }
                            KeyCode::Esc => panel_focus = false,
                            _ => {}
                        }
                    } else if prefix {
                        // Arrows browse and stay in tab mode; Enter (or anything else) confirms. Reset focus whenever `active` moves.
                        match key.code {
                            KeyCode::Right => {
                                active = (active + 1) % tabs.len();
                                panel_focus = false;
                            }
                            KeyCode::Left => {
                                active = (active + tabs.len() - 1) % tabs.len();
                                panel_focus = false;
                            }
                            KeyCode::Char('n') => {
                                tabs.push(Tab::spawn(default_shell(), area.height, area.width)?);
                                active = tabs.len() - 1;
                                panel_focus = false;
                                prefix = false;
                            }
                            KeyCode::Char('p') => {
                                if tabs[active].panel.is_some() {
                                    panel_focus = true;
                                }
                                prefix = false;
                            }
                            _ => prefix = false, // Enter confirms; anything else cancels
                        }
                    } else if ctrl && key.code == KeyCode::Char('q') {
                        break;
                    } else if ctrl && key.code == KeyCode::Char('t') {
                        prefix = true;
                    } else if let Some(bytes) = key_to_bytes(key.code, key.modifiers) {
                        tabs[active].pty.write(&bytes);
                    }
                }
                Event::Resize(cols, rows) => tabs[active].pty.resize(rows, cols),
                // Wheel scrolls the panel by moving its cursor (the view follows it).
                Event::Mouse(m) => {
                    if let Some(p) = tabs[active].panel.as_mut() {
                        match m.kind {
                            MouseEventKind::ScrollDown => p.move_cursor(3),
                            MouseEventKind::ScrollUp => p.move_cursor(-3),
                            _ => {}
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

/// The PTY's rect within the content area: full width, or left half beside a panel.
fn pty_layout(area: Rect, has_panel: bool) -> Rect {
    if has_panel {
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area)[0]
    } else {
        area
    }
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

/// Draw the global help popup: a centered, bordered list of key bindings, mirroring the contextual hints.
fn render_help(f: &mut ratatui::Frame) {
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
        key("^t", "tab commands"),
        key("^h", "this help"),
        key("⇧drag", "select text, then ^c to copy"),
        key("^q", "quit"),
        Line::raw(""),
        group("Tabs (^t …)"),
        key("←/→", "browse tabs"),
        key("n", "new tab"),
        key("p", "focus panel"),
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

/// Translate a key event into the bytes a terminal child expects on stdin.
fn key_to_bytes(code: KeyCode, mods: KeyModifiers) -> Option<Vec<u8>> {
    Some(match code {
        KeyCode::Char(c) => {
            if mods.contains(KeyModifiers::CONTROL) && c.is_ascii_alphabetic() {
                // Ctrl+A..Z -> control byte 0x01..0x1a.
                vec![(c.to_ascii_uppercase() as u8) & 0x1f]
            } else {
                c.to_string().into_bytes()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        _ => return None,
    })
}
