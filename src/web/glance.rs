//! The project at a glance: the start canvas after the user's first submitted inline review. It
//! shows three cards for the folder Laura started in: the README, the changes on
//! the branch or the latest commit, and the recent sessions in the folder. A card with nothing to
//! show is left out.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::gitdiff::{self, Project};
use crate::http;
use crate::journal::{self, Recent};
use crate::render;

/// The most sessions the canvas lists.
const SESSIONS: usize = 3;

#[derive(Serialize)]
pub(super) struct Glance {
    readme: Option<Readme>,
    project: Option<Project>,
    sessions: Vec<Recent>,
}

/// The README's file name and its blocks as HTML.
#[derive(Serialize)]
struct Readme {
    name: String,
    html: String,
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

/// The blocks of `root`'s README (`README.md` in any case), without its HTML blocks, which would
/// show as source. The card scrolls, as a file pane does.
fn readme(root: &Path, base: &str) -> Option<Readme> {
    let name = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| n.eq_ignore_ascii_case("README.md"))?;
    let path = root.join(&name);
    if !path.is_file() {
        return None;
    }
    let r = render::render(&path.to_string_lossy());
    let html = r
        .blocks
        .iter()
        .filter(|b| !b.is_html())
        .map(|b| b.html().replace('\0', base))
        .collect::<String>()
        // The canvas has no README pane, so a bare `#heading` names the README. File text can't
        // fake `data-href="`: `render` escapes its `"`.
        .replace(
            "data-href=\"#",
            &format!("data-href=\"{}#", http::pct_encode(&name)),
        );
    (!html.is_empty()).then_some(Readme { name, html })
}
