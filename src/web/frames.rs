//! The JSON frames that the web app's server sends a view each turn: the layout, each file pane's
//! rows or page, each browser pane, and each pane's threads. `src/web/CLAUDE.md` lists every
//! frame.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use ratatui::text::Line;
use serde_json::{Value, json};

use crate::panel::cursor::card_line;
use crate::panel::{Author, Thread, is_page};
use crate::protocol::{PTY_PANE, PaneId};
use crate::{Panel, PanelRow, Rect, Tab, view};

use super::html::{block_units, row_html};
use super::{Bases, Outbound, View, content_hash};

pub(super) fn layout_json(
    tab: &Tab,
    rects: &HashMap<PaneId, Rect>,
    area: Rect,
    bases: &Bases,
    drawer: u8,
) -> String {
    let mut ids: Vec<_> = rects.keys().copied().collect();
    ids.sort();
    let panes: Vec<Value> = ids
        .iter()
        .map(|id| {
            let r = rects[id];
            let editor = tab.editors.get(id);
            let p = tab.panels.get(id);
            let kind = match (p, editor) {
                (None, _) => "shell",
                (Some(p), _) if p.browser => "browser",
                (_, Some(e)) if e.editor_view => "editor",
                _ => "file",
            };
            // `pty`: the pane streams a terminal, in either view, so the browser keeps its xterm.
            let pty = *id == PTY_PANE || editor.is_some();
            let name = p.map_or_else(
                || tab.shell_name(),
                |p| {
                    if p.browser {
                        // A page without a title, or not loaded yet, goes by its URL.
                        return tab
                            .browsers
                            .get(id)
                            .map(|b| b.title())
                            .filter(|t| !t.is_empty())
                            .unwrap_or(&p.path)
                            .to_string();
                    }
                    Path::new(&p.path)
                        .file_name()
                        .map_or_else(|| p.path.clone(), |n| n.to_string_lossy().into_owned())
                },
            );
            let (added, removed) = p.map_or((0, 0), |p| p.diff_stat);
            let icon = match p {
                Some(p) if p.browser => Some("globe"),
                Some(p) if is_page(p) => Some("page"),
                Some(p) if p.tail => Some("log"),
                _ => None,
            };
            json!({"id": id, "x": r.x, "y": r.y, "w": r.width, "h": r.height,
                   "kind": kind, "pty": pty, "name": name, "icon": icon,
                   // The web app places the drawer by CSS, not by its cells.
                   "drawer": *id == PTY_PANE && tab.drawer_in_use(),
                   "path": p.map(|p| view::display_path(&p.path)),
                   "added": added, "removed": removed,
                   "highlight": p.and_then(|p| match p.highlight {
                       // A browser pane's highlight names its element.
                       Some((row, _)) if p.browser => {
                           p.anchors.get(row).map(|a| json!(view::cut_left(&a.selector, 24)))
                       }
                       _ => p.highlight_lines().map(|l| json!(l)),
                   }),
                   "diff": p.is_some_and(|p| p.diff_view),
                   "frozen": p.is_some_and(|p| p.source_changed),
                   // `null`: the pane can't pause.
                   "paused": p.filter(|p| p.pausable()).map(|p| p.paused),
                   "threads": p.map_or(0, Panel::thread_count),
                   "unsaved": editor.is_some_and(|e| e.modified())})
        })
        .collect();
    // A page takes clicks, not the arrows (app.js); a page with no route shows rows.
    let page = tab
        .panels
        .get(&tab.focus)
        .is_some_and(|p| is_page(p) && !p.diff_view)
        && bases.contains_key(&tab.focus);
    let browser = tab.panels.get(&tab.focus).is_some_and(|p| p.browser);
    let pane = view::hint(
        tab,
        if browser {
            view::Hint::Browser
        } else if page {
            view::Hint::Page
        } else {
            view::Hint::Pane
        },
    );
    let hints = json!({
        "pane": pane,
        "locked": view::hint(tab, view::Hint::Locked),
        "quit": view::hint(tab, view::Hint::Quit),
        "panes": view::hint(tab, view::Hint::Panes),
        "workspace": view::hint(tab, view::Hint::Workspace),
        "comment_mode": view::hint(tab, view::Hint::CommentMode),
        "browser_typing": view::hint(tab, view::Hint::BrowserTyping),
        "browser_url": view::hint(tab, view::Hint::BrowserUrl),
        "browser_dialog": view::hint(tab, view::Hint::BrowserDialog),
        "browser_leave": view::hint(tab, view::Hint::BrowserLeave),
    });
    let (rows, cols) = tab.pty_size();
    json!({"t": "layout", "cols": area.width, "rows": area.height, "focus": tab.focus,
           "drawer_height": drawer,
           // The saved screen's grid, so a new view's xterm parses the replay at its own size.
           "shell": {"cols": cols, "rows": rows},
           "agent": tab.agent, "waiting": tab.waiting, "wait": tab.wait, "hints": hints, "panes": panes})
    .to_string()
}

/// Send each browser pane's URL, title, whether its page is live (Laura's browser runs it), and
/// the page's state (`Page::state`), when one changed since the view last saw it.
pub(super) fn send_browsers(v: &mut View, tab: &Tab) {
    v.browsers
        .retain(|id, _| tab.panels.get(id).is_some_and(|p| p.browser));
    for (&id, p) in tab.panels.iter().filter(|(_, p)| p.browser) {
        let page = tab.browsers.get(&id);
        let mut msg = json!({"t": "browser", "id": id, "url": p.path,
                             "title": page.map(|b| b.title()), "live": page.is_some(),
                             "threads": threads_json(p), "card": card_line(p),
                             "line": p.cursor_line()});
        if let (Some(o), Some(b)) = (msg.as_object_mut(), page) {
            o.extend(b.state(Instant::now()));
        }
        let msg = msg.to_string();
        if v.browsers.get(&id) != Some(&msg) {
            v.send(Outbound::Text(msg.clone()));
            v.browsers.insert(id, msg);
        }
    }
}

/// Send each file-rendered pane's rows that changed since the view last saw them: `rows`, with the
/// new row count, unless the row count changes and more than half the rows change, else the whole
/// `pane`. A page sends `page` when its frame changes. `reloaded`
/// marks a frame whose content changed since the view last saw it, so the browser flashes only a
/// reload, not a scroll or a diff toggle.
pub(super) fn send_panes(v: &mut View, tab: &Tab, rects: &HashMap<PaneId, Rect>, bases: &Bases) {
    let shown: Vec<(PaneId, &Panel, Rect)> = rects
        .iter()
        .filter(|(id, _)| !tab.editors.get(id).is_some_and(|e| e.editor_view))
        .filter_map(|(id, r)| tab.panels.get(id).map(|p| (*id, p, *r)))
        // A browser pane shows frames, not rows.
        .filter(|(_, p, _)| !p.browser)
        .collect();
    v.panes.retain(|id, _| shown.iter().any(|(s, ..)| s == id));
    v.pages.retain(|id, _| shown.iter().any(|(s, ..)| s == id));
    for (id, panel, r) in shown {
        // Inside the border, less the gutter's blank first cell (`GAP`).
        let width = r.width.saturating_sub(3) as usize;
        let layout = panel.layout(width);
        let lines = view::panel_lines(panel, &layout, tab.focus == id, width, true);
        // No route (a missing file) falls back to rows.
        if is_page(panel)
            && !panel.diff_view
            && let Some((_, _, page)) = bases.get(&id)
        {
            let highlight = panel.highlight_lines();
            // A highlight (`Some(true)`) needs no flag: the page centers a new highlight itself.
            let jump = panel.take_recenter() == Some(false);
            // A jump's pane can get the focus after this frame, so the page scrolls now.
            let cursor = (tab.focus == id || jump).then(|| panel.cursor_line());
            let msg = json!({"t": "page", "id": id, "page": page, "cursor": cursor,
                             "line": panel.cursor_line(),
                             "threads": threads_json(panel), "card": card_line(panel),
                             "highlight": highlight, "jump": jump})
            .to_string();
            // A later `d` sends the whole `pane`, not a `rows` diff, and doesn't jump to the
            // highlight the page already scrolled to.
            v.panes.remove(&id);
            if v.pages.get(&id) != Some(&msg) {
                v.send(Outbound::Text(msg.clone()));
                v.pages.insert(id, msg);
            }
            continue;
        }
        v.pages.remove(&id);
        let (rows, cursor) = if !panel.diff_view && !panel.blocks().is_empty() {
            // No route (no randomness, a missing file): `/f/-/` finds nothing.
            let base = bases.get(&id).map_or("/f/-/", |b| b.2.as_str());
            block_units(panel, &layout, &lines, tab.focus == id, base)
        } else {
            // The thread cards go; the browser draws threads from `threads`. The removed-lines rows
            // sit before their line's start, or in `eof` below the last line, so they stay.
            let shown: Vec<(&PanelRow, &Line)> = layout
                .rows
                .iter()
                .zip(&lines)
                .enumerate()
                .filter(|(j, (row, _))| {
                    !row.comment || *j <= layout.starts[row.line] || layout.eof.contains(j)
                })
                .map(|(_, rl)| rl)
                .collect();
            // The row `panel_lines` draws the band on.
            let cursor = shown
                .iter()
                .position(|(r, _)| !r.comment && r.line == panel.cursor);
            let rows = shown
                .iter()
                .map(|(row, line)| row_html(panel, row, line))
                .collect();
            (rows, cursor)
        };
        let recenter = panel.take_recenter();
        // `open --panel` swaps in another file, which isn't a reload. A tail pane's new output is.
        let file = (panel.path.clone(), content_hash(panel));
        let reloaded = v
            .panes
            .get(&id)
            .is_some_and(|(_, _, (path, hash), _)| *path == file.0 && *hash != file.1);
        // A move onto a card, or a toggled thread, can leave the rows as they were.
        let (thread_list, card) = (threads_json(panel), card_line(panel));
        let threads = json!([thread_list, card]).to_string();
        let diff = v.panes.get(&id).and_then(|(old, ..)| {
            // New rows past the old end count as changed, so an append sends only the new rows.
            let d: Vec<Value> = (0..rows.len())
                .filter(|&i| old.get(i) != Some(&rows[i]))
                .map(|i| json!([i, rows[i]]))
                .collect();
            // ponytail: a gutter-width change (99 to 100 lines) re-pads every row; morph once instead.
            (old.len() == rows.len() || d.len() <= rows.len() / 2).then_some(d)
        });
        // The browser centers the highlight, or the cursor's line inside its block.
        let recenter = match recenter {
            None => json!(false),
            Some(true) => json!("highlight"),
            Some(false) => json!("line"),
        };
        let mut msg = match (v.panes.get(&id), diff) {
            (Some((old, old_cursor, _, old_threads)), Some(diff)) => {
                // The length check too, or a trim alone never reaches the browser.
                if old.len() == rows.len()
                    && diff.is_empty()
                    && *old_cursor == cursor
                    && recenter == false
                    && !reloaded
                    && *old_threads == threads
                {
                    continue;
                }
                json!({"t": "rows", "id": id, "len": rows.len(), "rows": diff, "cursor": cursor,
                       "recenter": recenter, "reloaded": reloaded})
            }
            _ => json!({"t": "pane", "id": id, "html": rows.concat(), "cursor": cursor,
                        "recenter": recenter, "reloaded": reloaded}),
        };
        msg["threads"] = thread_list;
        msg["card"] = json!(card);
        v.text(msg);
        v.panes.insert(id, (rows, cursor, file, threads));
    }
}

/// Each thread as the browser draws it: its 1-based source line, and each note's author, `null`
/// for the user.
fn threads_json(p: &Panel) -> Value {
    // By line, as `n`/`N` and the inline review order them; `threads` keeps insertion order.
    let mut ts: Vec<_> = p.threads.iter().collect();
    ts.sort_by_key(|t| t.line);
    json!(
        ts.into_iter()
            .map(|t| {
                let mut v = json!({
                    "line": p.thread_source(t).0 + 1,
                    "collapsed": t.collapsed,
                    "label": label(p, t),
                    "notes": t
                        .notes()
                        .map(|n| json!({"author": match &n.author {
                            Author::User => None,
                            Author::Agent(a) => Some(a),
                        }, "body": n.body}))
                        .collect::<Vec<_>>(),
                });
                // A browser pane's thread: its element, and its page when the pane moved on.
                if let Some(a) = p.anchors.get(t.line) {
                    v["label"] = a.quote.clone().into();
                    v["anchor"] = a.selector.clone().into();
                    v["away"] = away(&p.path, &a.url).into();
                    v["lost"] = a.lost.into();
                }
                v
            })
            .collect::<Vec<_>>()
    )
}

/// A thread's page when the pane shows another: its path on the pane's site, else its URL.
fn away(here: &str, url: &str) -> Option<String> {
    if crate::browser::page_of(here) == url {
        return None;
    }
    // `scheme://host`: what comes before the third `/`.
    fn origin(u: &str) -> &str {
        u.match_indices('/').nth(2).map_or(u, |(i, _)| &u[..i])
    }
    let o = origin(url);
    Some(if origin(here) == o && url.len() > o.len() {
        url[o.len()..].to_string()
    } else {
        url.to_string()
    })
}

/// What a thread sits on, for its card header: a markdown block's kind, or a page's start tag as
/// `<tag#id>` or `<tag.class>`. `None` for code, where `L<n>` says enough. A page names the line's
/// first start tag: a click reports only the line, and the outer element holds the rest of it.
fn label(p: &Panel, t: &Thread) -> Option<String> {
    if p.diff_view {
        return None;
    }
    if is_page(p) {
        let src = p.content.lines().nth(p.thread_source(t).0)?;
        let at = src
            .match_indices('<')
            .find(|(i, _)| src[i + 1..].starts_with(|c: char| c.is_ascii_alphabetic()))?
            .0;
        let tag = &src[at + 1..];
        let name = &tag[..tag
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '-')
            .unwrap_or(tag.len())];
        let head = &tag[..tag.find('>').unwrap_or(tag.len())];
        // The first token of `id` or else `class`, quoted either way, after any whitespace. A whole
        // token, so `data-id=` isn't `id`.
        let attr = |a: &str| {
            let v = head
                .split_ascii_whitespace()
                .find_map(|t| t.strip_prefix(a)?.strip_prefix('='))?;
            let v = v.strip_prefix(['"', '\'']).unwrap_or(v);
            let end = v.find(['"', '\'', '>']).unwrap_or(v.len());
            Some(&v[..end]).filter(|v| !v.is_empty())
        };
        let name = name.to_ascii_lowercase();
        return Some(match (attr("id"), attr("class")) {
            (Some(id), _) => format!("<{name}#{id}>"),
            (_, Some(class)) => format!("<{name}.{class}>"),
            _ => format!("<{name}>"),
        });
    }
    let html = p.blocks().iter().find(|b| b.rows.contains(&t.line))?.html();
    let tag = html.strip_prefix('<')?;
    let tag = &tag[..tag.find(|c: char| !c.is_ascii_alphanumeric())?];
    Some(
        match tag {
            "p" => "paragraph",
            "ul" | "ol" | "li" => "list item",
            "table" => "table row",
            "blockquote" => "quote",
            "pre" => "code line",
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "heading",
            _ => return None,
        }
        .into(),
    )
}
