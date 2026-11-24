//! The project at a glance: the start canvas after the user's first submitted inline review. It
//! shows three cards for the folder Laura started in: the README's first section, the changes on
//! the branch or the latest commit, and the recent sessions in the folder. A card with nothing to
//! show is left out.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::gitdiff::{self, Project};
use crate::journal::{self, Recent};
use crate::render::{self, Block};

/// The most README blocks the canvas shows.
const README_BLOCKS: usize = 6;
/// The most sessions the canvas lists.
const SESSIONS: usize = 3;

#[derive(Serialize)]
pub(super) struct Glance {
    readme: Option<Readme>,
    project: Option<Project>,
    sessions: Vec<Recent>,
}

/// The README's first section as HTML, and the level-2 headings after it.
#[derive(Serialize)]
struct Readme {
    html: String,
    more: Vec<String>,
}

impl Glance {
    /// The glance at `root`, or `None` when every card is empty. `base` is the file route of
    /// `root`, for the README's images.
    pub(super) fn of(root: &Path, base: &str) -> Option<Glance> {
        let g = Glance {
            readme: readme(root, base),
            project: gitdiff::project(root),
            sessions: journal::recent(root, SESSIONS),
        };
        (g.readme.is_some() || g.project.is_some() || !g.sessions.is_empty()).then_some(g)
    }

    /// The `glance` frame's text.
    pub(super) fn frame(&self) -> String {
        let mut v = serde_json::to_value(self).unwrap_or_default();
        if let Value::Object(o) = &mut v {
            o.insert("t".into(), "glance".into());
        }
        v.to_string()
    }
}

/// `root/README.md`'s blocks up to its second heading of level 1 or 2, at most `README_BLOCKS`.
fn readme(root: &Path, base: &str) -> Option<Readme> {
    let path = root.join("README.md");
    if !path.is_file() {
        return None;
    }
    let r = render::render(&path.to_string_lossy());
    let top = |b: &Block| b.heading().is_some_and(|(level, _)| level <= 2);
    let end = r
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| top(b))
        .nth(1)
        .map_or(r.blocks.len(), |(i, _)| i);
    let html = r.blocks[..end]
        .iter()
        .take(README_BLOCKS)
        .map(|b| b.html().replace('\0', base))
        .collect::<String>()
        // The canvas has no README pane, so a bare `#heading` names the README. File text can't
        // fake `data-href="`: `render` escapes its `"`.
        .replace("data-href=\"#", "data-href=\"README.md#");
    let more = r.blocks[end..]
        .iter()
        .filter_map(Block::heading)
        .filter(|&(level, _)| level == 2)
        .map(|(_, text)| text.to_string())
        .collect();
    (!html.is_empty()).then_some(Readme { html, more })
}
