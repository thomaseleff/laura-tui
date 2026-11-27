//! The wait: the shell rang the bell or sent a notification since the user last moved to the
//! shell, used it or submitted an inline review, so the agent waits for the user. Also whether the
//! agent said something (`laura say`) since the user last wrote to the shell.

/// The workspace's wait, from the shell's bell count.
#[derive(Default)]
pub(super) struct Wait {
    waiting: bool,
    /// The shell's bell count when the current wait began. The layout frame carries this count,
    /// and `seen` names it, so a `seen` from before a new wait does not end the new wait.
    wait: u64,
    /// The shell's bell count that `bell` last saw.
    bells: u64,
    /// A say came since the user's last write to the shell.
    said: bool,
}

impl Wait {
    /// `drain` read the shell's bell count `bells`. A new wait begins, and `bell` returns true; a
    /// bell during a wait is the same wait the user sees.
    pub(super) fn bell(&mut self, bells: u64) -> bool {
        let new = bells != self.bells && !self.waiting;
        if new {
            self.wait = bells;
        }
        self.waiting |= bells != self.bells;
        self.bells = bells;
        new
    }

    /// The user saw the wait: a move into the shell. The wait ends. `bells` is the count the
    /// answer covers.
    pub(super) fn saw(&mut self, bells: u64) {
        self.waiting = false;
        self.bells = bells;
    }

    /// The user wrote to the shell: keystrokes, a paste or an inline review. The wait
    /// ends, and a say must come again before the next wait counts as said.
    pub(super) fn answered(&mut self, bells: u64) {
        self.saw(bells);
        self.said = false;
    }

    /// The web app's `seen` ends only the wait it names: `wait` is untrusted, and a late `seen`
    /// after the wait ended does nothing, so the answered count never goes down.
    pub(super) fn seen(&mut self, wait: u64, bells: u64) {
        if self.waiting && wait == self.wait {
            self.saw(bells);
        }
    }

    /// The agent said something with `laura say`.
    pub(super) fn say(&mut self) {
        self.said = true;
    }

    pub(super) fn waiting(&self) -> bool {
        self.waiting
    }

    pub(super) fn said(&self) -> bool {
        self.said
    }

    /// The shell waits with no say since the user last wrote to it: a permission prompt, or a turn
    /// that ended in the shell alone.
    pub(super) fn unsaid(&self) -> bool {
        self.waiting && !self.said
    }

    /// The bell count that names the current wait.
    pub(super) fn id(&self) -> u64 {
        self.wait
    }
}
