//! The Laura engine. A package of cohesive modules the terminal front-end drives:
//!
//! - [`protocol`] — the wire `Message`/`Response` set and per-tab socket transport.
//! - [`pty`] — process/terminal: a PTY-hosted shell/agent and its parsed vt100 screen.
//! - [`layout`] — geometry/split-tree: a tab's recursive pane arrangement and its rects.
//! - [`panel`] — a file's review state: content, cursor, threads, live reload; with `render`, `gitdiff` and `cursor`.
//! - [`journal`] — persisted per-session NDJSON log of composition events.
//! - [`browser`] — a browser pane's page in Laura's own headless Chrome or Edge.
//! - [`cdp_proxy`] — `LAURA_BROWSER_URL`: the agent's browser tools reach Laura's browser through Laura.
//! - [`editor`] — an editor pane's Neovim (experimental) and the startup gate.
//! - [`tab`] — one workspace tab tying the above together over its socket; with `apply`, `act`, `links`, `browsers` and `report`.
//! - [`view`] — pane titles, file pane rows, and the hint, help, and notice text a front-end draws.
//! - [`web`] — the web app: serves a workspace to the browser; with `frames` and `html`.
//! - [`serve`] — the Laura server: one address that lists the workspaces and starts new ones.
//! - [`http`] — HTTP for the web app, the Laura server and the CDP proxy: heads, responses, tokens, origins.
//! - [`platform`] — Laura's folders, private files, and the opener for the user's browser.
//! - [`settings`] — the user's settings for every workspace, `~/.laura/settings.json`.
//!
//! Nothing here depends on the binary; the binary depends on this.

pub mod browser;
pub mod cdp_proxy;
pub mod editor;
pub mod http;
pub mod journal;
pub mod layout;
pub mod panel;
pub mod platform;
pub mod protocol;
pub mod pty;
pub mod serve;
pub mod settings;
pub mod shortcut;
pub mod tab;
pub mod view;
pub mod web;

pub use ratatui::layout::Rect;

pub use panel::{gitdiff, render};

pub use gitdiff::ChangeKind;
pub use journal::Journal;
pub use layout::{Layout, rects};
pub use panel::{
    Author, Note, Panel, PanelLayout, PanelRow, Thread, bracketed_paste, wrap_line, wrap_spans,
};
pub use protocol::{
    Dir, LayoutReport, Message, PTY_PANE, PaneId, PaneKind, PaneReport, Reply, Response, Side,
};
pub use pty::{PtyTab, dsr_reply};
pub use tab::{Tab, UserAction};
