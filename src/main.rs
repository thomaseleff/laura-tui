//! `laura` — the terminal front-end that drives the engine ([`laura`] the library).
//!
//! Two modules split by concern: [`tui`] is the draw loop and its widgets (it hosts the tabs,
//! renders panes, routes input); [`keys`] turns a key event into the bytes a child expects on
//! stdin. `main` here is just args + dispatch: with no subcommand it launches the TUI, otherwise
//! it sends one request to the current tab's socket and prints the reply. Nothing in the engine
//! depends on this crate.

mod keys;
mod tui;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
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
    /// Mark this tab as hosting an agent (enables review submission).
    Ready,
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
            dry_run,
        }) => client_request(Message::Open {
            path,
            split,
            dir,
            ratio,
            side,
            focus: !no_focus,
            dry_run,
        }),
        Some(Cmd::Close { id, all }) => client_request(Message::Close { pane: id, all }),
        Some(Cmd::Focus { id }) => client_request(Message::Focus { pane: id }),
        Some(Cmd::Layout) => client_request(Message::Layout),
        Some(Cmd::Ready) => client_request(Message::Ready),
        None => {
            let mut terminal = ratatui::init();
            // Capture the wheel so panels scroll; the shell gets no mouse anyway (keys only).
            let _ = execute!(std::io::stdout(), EnableMouseCapture);
            let result = tui::run(&mut terminal, cli.program);
            let _ = execute!(std::io::stdout(), DisableMouseCapture);
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
        Response::Error { message } => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    }
    Ok(())
}
