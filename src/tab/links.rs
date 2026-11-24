//! Links the user follows from a pane: a link to a file with an optional `#heading`, or a file path
//! with an optional line. Laura opens the file in a new pane, or moves to the pane that shows it.

use std::path::{Component, Path};

use anyhow::Result;

use serde_json::json;

use crate::http;
use crate::protocol::{PTY_PANE, PaneId, Response};

use super::{Error, NO_ROOM, Tab, no_pane};

impl Tab {
    /// Open `href`'s file, without its `?query`, beside pane `pane`'s, as `laura open` would, or move to the pane already
    /// showing it, and put the cursor on its `#heading`. A bare `#heading` moves pane `pane`'s
    /// cursor. A missing heading is a notice for the pane that shows the file.
    pub(super) fn open_link(&mut self, pane: PaneId, href: &str) -> Result<(), String> {
        let missing = || format!("⚠ The link target {href} does not exist.");
        let (path, heading) = href.split_once('#').unwrap_or((href, ""));
        let path = path.split_once('?').map_or(path, |(p, _)| p);
        let (path, heading) = (
            http::pct_decode(path).ok_or_else(missing)?,
            http::pct_decode(heading).ok_or_else(missing)?,
        );
        let mut target = pane;
        let dir = Path::new(&self.panel_mut(pane)?.path)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        if !path.is_empty() {
            // A root or drive (`/x`, `//host/share`, `C:x`) would replace the folder in `join`.
            let rel = Path::new(&path);
            if !rel.components().all(|c| {
                matches!(
                    c,
                    Component::Normal(_) | Component::CurDir | Component::ParentDir
                )
            }) {
                return Err(missing());
            }
            // As `laura open` names it, so `..` and `/` match the CLI's path on Windows.
            let file = std::path::absolute(dir.join(rel)).map_err(|_| missing())?;
            if file.is_dir() {
                return Err(format!("⚠ The link target {href} is a folder."));
            }
            if !file.is_file() {
                return Err(missing());
            }
            target = self.open_file(file.to_string_lossy().into_owned(), false)?;
        }
        // Not `panel_mut`: an editor pane in its editor view keeps the heading for its file view.
        let p = self
            .panels
            .get_mut(&target)
            .ok_or_else(|| no_pane(target))?;
        if !heading.is_empty() && !p.blocks().is_empty() {
            match p.heading_line(&heading) {
                Some(line) => p.jump_to_line(line),
                // A notice, not an error: an error shows in the link's pane, and `target` can be another.
                None => {
                    let name = Path::new(&p.path).file_name().unwrap_or_default();
                    let text = format!(
                        "⚠ The file {} has no heading #{heading}.",
                        name.to_string_lossy()
                    );
                    self.notices.push((Some(target), text));
                }
            }
        }
        Ok(())
    }

    /// Focus the pane already showing the absolute `file`, else open it as `laura open` would,
    /// in the diff view when `diff`.
    fn open_file(&mut self, file: String, diff: bool) -> Result<PaneId, String> {
        if let Some(id) = self.showing(&file, PTY_PANE) {
            self.focus = id;
            return Ok(id);
        }
        let msg = serde_json::from_value(json!({"type": "open", "path": file, "diff": diff}))
            .expect("a path-only open always parses");
        match self.open(msg, self.area) {
            Ok(Response::Opened { pane, .. }) => Ok(pane),
            Ok(Response::Error { message }) => Err(message),
            // The user clicked a link and set no --ratio: the agent's split text doesn't help.
            Err(Error::SplitTooSmall) => Err(NO_ROOM.into()),
            Err(e) => Err(e.to_string()),
            Ok(_) => unreachable!("an open without dry_run answers Opened or Error"),
        }
    }

    /// Open `path` (relative to Laura's folder), in the diff view when `diff`, and put the cursor on `line`.
    pub(super) fn open_path(
        &mut self,
        path: &str,
        line: Option<u32>,
        diff: bool,
    ) -> Result<(), String> {
        let missing = || format!("⚠ The link target {path} does not exist.");
        let file = std::path::absolute(path).map_err(|_| missing())?;
        if file.is_dir() {
            return Err(format!("⚠ The link target {path} is a folder."));
        }
        if !file.is_file() {
            return Err(missing());
        }
        let id = self.open_file(file.to_string_lossy().into_owned(), diff)?;
        // Not `panel_mut`: an editor pane in its editor view keeps the line for its file view.
        if let (Some(l), Some(p)) = (line, self.panels.get_mut(&id)) {
            p.jump_to_line(l);
        }
        Ok(())
    }
}
