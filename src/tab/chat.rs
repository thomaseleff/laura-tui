//! The chat: what passed through Laura in the current session — the agent's says.

use crate::journal::Event;

#[derive(Default)]
pub(super) struct Chat {
    /// The says over the tab's life, so a front-end tells a new say from the last one it showed.
    says: u64,
    latest: Option<String>,
}

impl Chat {
    /// Add one journaled event.
    pub(super) fn push(&mut self, event: &Event) {
        if let Event::Say { body } = event {
            self.says += 1;
            self.latest = Some(body.clone());
        }
    }

    /// A new session (`ready`): an empty chat. The say count goes on.
    pub(super) fn new_session(&mut self) {
        self.latest = None;
    }

    pub(super) fn latest_say(&self) -> Option<(u64, &str)> {
        Some((self.says, self.latest.as_deref()?))
    }
}
