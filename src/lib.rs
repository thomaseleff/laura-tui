//! The Laura engine. A package of cohesive modules the terminal front-end drives:
//!
//! - [`protocol`] — the wire `Message`/`Response` set and per-tab socket transport.
//! - [`pty`] — process/terminal: a PTY-hosted shell/agent and its parsed vt100 screen.
//! - [`layout`] — geometry/split-tree: a tab's recursive pane arrangement and its rects.
//! - [`render`] — file→styled: read a file into styled lines plus a plain-text projection.
//! - [`panel`] — a file's review state: content, cursor, comment threads, and live reload.
//! - [`journal`] — persisted per-session NDJSON log of composition events.
//! - [`browser`] — a browser pane's page in Laura's own headless Chrome or Edge (experimental).
//! - [`editor`] — an editor pane's Neovim (experimental) and the startup gate.
//! - [`tab`] — one workspace tab tying the above together over its socket.
//! - [`view`] — pane titles, file pane rows, and the hint, help, and notice text a front-end draws.
//! - [`web`] — the web app (experimental): serves a workspace to the browser.
//!
//! Nothing here depends on the binary; the binary depends on this.

pub mod browser;
pub mod editor;
pub mod gitdiff;
pub mod journal;
pub mod layout;
pub mod panel;
pub mod protocol;
pub mod pty;
pub mod render;
pub mod tab;
pub mod view;
pub mod web;

pub use ratatui::layout::Rect;

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
