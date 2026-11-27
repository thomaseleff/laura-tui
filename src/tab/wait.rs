//! The wait: the shell rang the bell or sent a notification since the user last moved to the
//! shell, used it or submitted an inline review, so the agent waits for the user.

/// The workspace's wait, from the shell's bell count.
#[derive(Default)]
pub(super) struct Wait {
    waiting: bool,
    /// The shell's bell count when the current wait began. The layout frame carries this count,
    /// and `seen` names it, so a `seen` from before a new wait does not end the new wait.
    wait: u64,
    /// The shell's bell count that `bell` last saw.
    bells: u64,
}

impl Wait {
    /// `drain` read the shell's bell count `bells`. A new wait begins; a bell during a wait is
    /// the same wait the user sees.
    pub(super) fn bell(&mut self, bells: u64) {
        if bells != self.bells && !self.waiting {
            self.wait = bells;
        }
        self.waiting |= bells != self.bells;
        self.bells = bells;
    }

    /// The user answered the agent or saw the wait: the wait ends. `bells` is the count the
    /// answer covers.
    pub(super) fn answered(&mut self, bells: u64) {
        self.waiting = false;
        self.bells = bells;
    }

    /// The web app's `seen` ends only the wait it names: `wait` is untrusted, and a late `seen`
    /// after the wait ended does nothing, so the answered count never goes down.
    pub(super) fn seen(&mut self, wait: u64, bells: u64) {
        if self.waiting && wait == self.wait {
            self.answered(bells);
        }
    }

    pub(super) fn waiting(&self) -> bool {
        self.waiting
    }

    /// The bell count that names the current wait.
    pub(super) fn id(&self) -> u64 {
        self.wait
    }
}
