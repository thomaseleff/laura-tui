//! The workspace's input modes, which the TUI and the web app both read: an overlay (the theme
//! wizard, the help, the pane selector or the quit question), the lock that `F12` turns on, and
//! the draft of a comment or a review body. The first mode on takes a keystroke before any pane.
//! Beside them, the drawer's view: the History or the shell.

use serde::{Deserialize, Serialize};

use crate::panel::Anchor;
use crate::protocol::{PTY_PANE, PaneId};

use super::{THEME_SET, Tab};

/// A sheet over the workspace that takes every keystroke, one at a time.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Overlay {
    /// The startup wizard, which asks for the theme of editor panes. TUI only.
    Wizard,
    Help,
    Selector,
    Quit,
}

/// The comment or the review body the user types. Laura holds the agent's requests while it is open.
#[derive(Clone, Debug, PartialEq)]
pub enum Draft {
    /// `line`: the cursor's source line at `c`, where the comment goes. `edit`: `c` seeded the
    /// user's own comment, so an empty `Enter` deletes it.
    Comment {
        pane: PaneId,
        line: u32,
        edit: bool,
    },
    Review {
        pane: PaneId,
    },
}

/// The lock: the shell, or an editor pane's Neovim, takes every keystroke but `F12`. `from`: the
/// file or browser pane the lock moved the focus from, which the unlock gives the focus back to.
#[derive(Clone, Copy, Default)]
enum Lock {
    #[default]
    Off,
    On {
        from: Option<PaneId>,
    },
}

/// What the drawer shows: the History, or the shell. `Tab::drawer_shows_shell` gives the layout
/// frame's `drawer_view`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) enum DrawerView {
    History,
    #[default]
    Shell,
}

/// The modes that are on, behind `Tab`'s methods, and the drawer's view, which no input ends.
#[derive(Default)]
pub(super) struct Modes {
    overlay: Option<Overlay>,
    lock: Lock,
    draft: Option<Draft>,
    pub(super) drawer: DrawerView,
}

/// A mode that is on, as `Tab::modes` lists it and the web app's layout frame names it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Wizard,
    Help,
    Selector,
    Quit,
    Locked,
    Draft,
}

impl Tab {
    /// The modes on, first to last: the overlay, then the lock, then the draft. The first takes a
    /// keystroke.
    pub fn modes(&self) -> Vec<Mode> {
        let overlay = self.modes.overlay.map(|o| match o {
            Overlay::Wizard => Mode::Wizard,
            Overlay::Help => Mode::Help,
            Overlay::Selector => Mode::Selector,
            Overlay::Quit => Mode::Quit,
        });
        let locked = self.locked().then_some(Mode::Locked);
        let draft = self.modes.draft.as_ref().map(|_| Mode::Draft);
        [overlay, locked, draft].into_iter().flatten().collect()
    }

    /// The mode that takes a keystroke; `None`: the focused pane takes it.
    pub fn mode(&self) -> Option<Mode> {
        self.modes().first().copied()
    }

    pub fn overlay(&self) -> Option<Overlay> {
        self.modes.overlay
    }

    pub(crate) fn locked(&self) -> bool {
        matches!(self.modes.lock, Lock::On { .. })
    }

    pub fn draft(&self) -> Option<&Draft> {
        self.modes.draft.as_ref()
    }

    /// A browser pane's comment draft names its element.
    pub fn draft_anchor(&self) -> Option<&Anchor> {
        let Some(Draft::Comment { pane, .. }) = self.draft() else {
            return None;
        };
        let p = self.panels.get(pane)?;
        p.anchors.get(p.cursor)
    }

    /// The user types a comment or a review body, or answers the theme wizard: the agent's
    /// requests wait, and the panes don't reload, until the user finishes.
    pub fn held(&self) -> bool {
        self.modes.draft.is_some() || self.modes.overlay == Some(Overlay::Wizard)
    }

    /// Open the theme wizard, which the TUI shows at the start until the user answers.
    pub fn ask_theme(&mut self) {
        self.modes.overlay = Some(Overlay::Wizard);
    }

    /// A view took over or closed: no mode is on, so the agent never waits on a closed browser tab.
    pub(crate) fn end_modes(&mut self) {
        self.modes = Modes {
            drawer: self.modes.drawer,
            ..Modes::default()
        };
    }

    /// `F12`. Alone, the lock moves the focus from a file or browser pane to the shell, and the
    /// unlock moves it back unless the user moved it meanwhile (`drain`). Under an overlay the
    /// user hasn't reached the shell, and a draft holds its pane: there the focus stays.
    pub(super) fn toggle_lock(&mut self) {
        let bells = self.pty.bells();
        self.modes.lock = match self.modes.lock {
            Lock::Off => {
                let alone = self.modes.overlay.is_none() && self.modes.draft.is_none();
                let from = (alone && self.pane_pty(self.focus).is_none()).then_some(self.focus);
                if from.is_some() {
                    self.focus = PTY_PANE;
                }
                Lock::On { from }
            }
            Lock::On { from } => {
                if let Some(p) = from.filter(|p| self.panels.contains_key(p))
                    && self.focus == PTY_PANE
                {
                    self.focus = p;
                }
                Lock::Off
            }
        };
        self.seen_if_shell(bells);
    }

    /// The focus left the shell under the lock: the unlock leaves it where it is.
    pub(super) fn forget_lock_from(&mut self) {
        if self.focus != PTY_PANE
            && let Lock::On { from } = &mut self.modes.lock
        {
            *from = None;
        }
    }

    /// Open the help or the pane selector, or close the overlay. The wizard closes only with the
    /// user's answer (`answer_theme`), and the quit question opens only after `Quit`.
    pub(super) fn set_overlay(&mut self, overlay: Option<Overlay>) {
        let bells = self.pty.bells();
        if self.modes.overlay == Some(Overlay::Wizard)
            || matches!(overlay, Some(Overlay::Wizard | Overlay::Quit))
        {
            return;
        }
        self.modes.overlay = overlay;
        self.seen_if_shell(bells);
    }

    /// `Ctrl+Q` with nothing unsaved: the quit question.
    pub(super) fn ask_quit(&mut self) {
        self.modes.overlay = Some(Overlay::Quit);
    }

    /// The wizard's answer: Laura's theme (`true`), the user's own (`false`), or not now (`None`).
    pub(super) fn answer_theme(&mut self, laura: Option<bool>) {
        if self.modes.overlay != Some(Overlay::Wizard) {
            return;
        }
        self.modes.overlay = None;
        if let Some(yes) = laura {
            // A failed write only means the wizard asks again at the next start.
            let theme = if yes { "laura" } else { "neovim" };
            let _ = crate::settings::set("editor_theme", Some(theme.into()));
            self.editor_theme = yes;
            self.notices.push((None, THEME_SET.into()));
        }
    }

    pub(super) fn open_draft(&mut self, draft: Draft) {
        self.modes.draft = Some(draft);
    }

    /// The comment or the inline review went in, or the user cancelled the draft.
    pub(super) fn close_draft(&mut self) {
        self.modes.draft = None;
    }

    /// After the lock or an overlay's close, the shell has the keystrokes: the user sees the agent.
    fn seen_if_shell(&mut self, bells: u64) {
        let to_shell =
            self.focus == PTY_PANE || (self.locked() && self.pane_pty(self.focus).is_none());
        if self.modes.overlay.is_none() && to_shell {
            self.saw(bells);
        }
    }
}
