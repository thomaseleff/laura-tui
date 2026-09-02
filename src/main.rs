//! `laura` — the terminal front-end that drives the engine ([`laura`] the library).
//!
//! Two modules split by concern: [`tui`] is the draw loop and its widgets (it hosts the tabs,
//! renders panes, routes input); [`keys`] turns a key event into the bytes a child expects on
//! stdin. `main` here is just args + dispatch: with no subcommand it launches the TUI, otherwise
//! it sends one request to the current tab's socket and prints the reply. Nothing in the engine
//! depends on this crate.

mod keys;
mod mouse;
mod tui;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
// Bracketed paste is a no-op stub on Windows (crossterm #962) that can taint pasted Enter events, so
// we only enable it on Unix and detect paste bursts manually on Windows (see tui::coalesce_paste_burst).
#[cfg(not(windows))]
use ratatui::crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use ratatui::crossterm::execute;

use laura::protocol::{self, Dir, Message, PaneId, Response, Side};

/// Laura hosts your coding agent's shell in a PTY with a live side-panel for showing files and
/// receiving in-place review.
///
/// Agents: install the skill — `claude plugin marketplace add thomaseleff/laura-tui`.
///
/// Docs: https://thomaseleff.github.io/laura-tui/llms.txt
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
    /// Open a file in a new panel, splitting a pane. Prints the new pane id.
    Open {
        path: String,
        /// Pane to split (default: the focused pane).
        #[arg(long)]
        split: Option<PaneId>,
        /// Split orientation: `h` side-by-side, `v` stacked.
        #[arg(long, value_enum, default_value_t = Dir::Horizontal)]
        dir: Dir,
        /// Percent of the split given to the first pane (1..=99).
        #[arg(long, default_value_t = 50)]
        ratio: u16,
        /// Which side the new panel lands on.
        #[arg(long, value_enum, default_value_t = Side::Second)]
        side: Side,
        /// Don't move focus into the panel.
        #[arg(long)]
        no_focus: bool,
        /// Autoscroll: keep the newest line in view as the file grows.
        #[arg(long)]
        follow: bool,
        /// Print the would-be overflow report without opening anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// Close a panel (default: the focused one). `--all` returns to PTY-only.
    Close {
        /// Pane id to close (default: the focused panel).
        id: Option<PaneId>,
        #[arg(long)]
        all: bool,
    },
    /// Focus a pane by id.
    Focus { id: PaneId },
    /// Print the current layout tree + per-pane rects & overflow (JSON).
    Layout,
    /// Mark this tab as hosting an agent (enables review submission). Prints the journal path.
    Ready {
        /// Name the journal session (default: `laura-<pid>-<n>`).
        #[arg(long)]
        session: Option<String>,
        /// Attribute journal events to this agent name.
        #[arg(long)]
        agent: Option<String>,
    },
    /// Append a feedback signal (layout/render quality, a missing tool) to the journal.
    Feedback {
        /// Positive signal.
        #[arg(long, conflicts_with = "negative")]
        positive: bool,
        /// Negative signal.
        #[arg(long)]
        negative: bool,
        /// Free-text note.
        body: Option<String>,
    },
    /// Spool piped stdin to an internal file and show it in a live, autoscrolling panel.
    /// Usage: `some-cmd | laura tail --follow`.
    Tail {
        /// Panel title (also names the spool file).
        #[arg(long)]
        title: Option<String>,
        /// Autoscroll to the newest line as output arrives.
        #[arg(long)]
        follow: bool,
        /// Pane to split (default: the focused pane).
        #[arg(long)]
        split: Option<PaneId>,
        /// Split orientation: `h` side-by-side, `v` stacked.
        #[arg(long, value_enum, default_value_t = Dir::Horizontal)]
        dir: Dir,
        /// Percent of the split given to the first pane (1..=99).
        #[arg(long, default_value_t = 50)]
        ratio: u16,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Cmd::Open {
            path,
            split,
            dir,
            ratio,
            side,
            no_focus,
            follow,
            dry_run,
        }) => {
            // Absolutize against the caller's cwd, not the server's. `absolute` (not
            // `canonicalize`) touches no filesystem, so a missing file still surfaces its error.
            let path = std::path::absolute(&path)?.to_string_lossy().into_owned();
            client_request(Message::Open {
                path,
                split,
                dir,
                ratio,
                side,
                focus: !no_focus,
                follow,
                dry_run,
            })
        }
        Some(Cmd::Close { id, all }) => client_request(Message::Close { pane: id, all }),
        Some(Cmd::Focus { id }) => client_request(Message::Focus { pane: id }),
        Some(Cmd::Layout) => client_request(Message::Layout),
        Some(Cmd::Ready { session, agent }) => client_request(Message::Ready { session, agent }),
        Some(Cmd::Feedback {
            positive,
            negative,
            body,
        }) => {
            let sentiment = match (positive, negative) {
                (true, _) => "+",
                (_, true) => "-",
                _ => bail!("pass --positive or --negative"),
            };
            client_request(Message::Feedback {
                sentiment: sentiment.into(),
                body,
            })
        }
        Some(Cmd::Tail {
            title,
            follow,
            split,
            dir,
            ratio,
        }) => tail(title, follow, split, dir, ratio),
        None => {
            let mut terminal = ratatui::init();
            // Capture the wheel so panels scroll and drags select; bracketed paste keeps a pasted
            // multi-line block one unit instead of a submit per newline.
            let _ = execute!(std::io::stdout(), EnableMouseCapture);
            #[cfg(not(windows))]
            let _ = execute!(std::io::stdout(), EnableBracketedPaste);
            let result = tui::run(&mut terminal, cli.program);
            let _ = execute!(std::io::stdout(), DisableMouseCapture);
            #[cfg(not(windows))]
            let _ = execute!(std::io::stdout(), DisableBracketedPaste);
            ratatui::restore();
            result
        }
    }
}

/// Send one request to the current tab's socket ($LAURA_TAB) and print its response. Never touches ratatui.
fn client_request(msg: Message) -> Result<()> {
    let Ok(tab) = std::env::var("LAURA_TAB") else {
        bail!("not inside a Laura tab (LAURA_TAB unset)");
    };
    match protocol::request(&tab, &msg)? {
        Response::Ok => {}
        Response::Opened { pane, warnings } => {
            for w in warnings {
                eprintln!("{w}");
            }
            println!("{pane}");
        }
        Response::Report(report) => println!("{}", serde_json::to_string_pretty(&report)?),
        Response::Ready { journal } => println!("{journal}"),
        Response::Error { message } => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    }
    Ok(())
}

/// `some-cmd | laura tail`: spool stdin to an internal file, show it as a live follow-panel,
/// then keep copying stdin → file until EOF. The file lives under Laura's runtime dir and is
/// auto-removed when the panel closes.
///
/// ponytail: file-backed, not socket-streamed — deferred. One temp file per invocation.
fn tail(
    title: Option<String>,
    follow: bool,
    split: Option<PaneId>,
    dir: Dir,
    ratio: u16,
) -> Result<()> {
    use std::io::Read;

    let Ok(tab) = std::env::var("LAURA_TAB") else {
        bail!("not inside a Laura tab (LAURA_TAB unset)");
    };
    let runtime = laura::journal::runtime_dir();
    std::fs::create_dir_all(&runtime)?;
    let stem = title.as_deref().unwrap_or("tail");
    let stem: String = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let path = runtime.join(format!("{stem}-{}.txt", std::process::id()));
    let mut file = std::fs::File::create(&path)?;

    // Open the panel first (empty file is fine) so it appears immediately, then stream into it.
    match protocol::request(
        &tab,
        &Message::Open {
            path: path.to_string_lossy().into_owned(),
            split,
            dir,
            ratio,
            side: Side::default(),
            focus: false,
            follow,
            dry_run: false,
        },
    )? {
        Response::Opened { pane, warnings } => {
            for w in warnings {
                eprintln!("{w}");
            }
            println!("{pane}");
        }
        Response::Error { message } => {
            eprintln!("{message}");
            std::process::exit(1);
        }
        _ => {}
    }

    let mut stdin = std::io::stdin().lock();
    let mut buf = [0u8; 8192];
    loop {
        match stdin.read(&mut buf)? {
            0 => break,
            n => {
                use std::io::Write;
                file.write_all(&buf[..n])?;
                file.flush()?;
            }
        }
    }
    Ok(())
}
