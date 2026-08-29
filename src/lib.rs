//! The Laura engine. A package of cohesive modules the terminal front-end drives:
//!
//! - [`protocol`] — the wire `Message`/`Response` set and per-tab socket transport.
//! - [`pty`] — process/terminal: a PTY-hosted shell/agent and its parsed vt100 screen.
//! - [`layout`] — geometry/split-tree: a tab's recursive pane arrangement and its rects.
//! - [`render`] — file→styled: read a file into styled lines plus a plain-text projection.
//! - [`panel`] — a file's review state: content, cursor, comments, and live reload.
//! - [`tab`] — one workspace tab tying the above together over its socket.
//!
//! Nothing here depends on the binary; the binary depends on this.

pub mod layout;
pub mod panel;
pub mod protocol;
pub mod pty;
pub mod render;
pub mod tab;

pub use ratatui::layout::Rect;

pub use layout::{Layout, pane_at, rects};
pub use panel::{Panel, PanelLayout, PanelRow, bracketed_paste, wrap_line, wrap_spans};
pub use protocol::{
    Dir, LayoutReport, Message, PTY_PANE, PaneId, PaneKind, PaneReport, Reply, Response, Side,
};
pub use pty::{PtyTab, dsr_reply};
pub use tab::Tab;
