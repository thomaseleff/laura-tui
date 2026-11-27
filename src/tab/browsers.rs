//! Laura's browser for the workspace's browser panes: starting it, attaching each pane's page,
//! polling the pages for events, and keeping the panes when Laura's browser exits.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::time::Instant;

use anyhow::Result;
use ratatui::layout::Rect;

use serde_json::json;

use crate::browser::{self, Browser, Cdp, Page};
use crate::cdp_proxy;
use crate::protocol::{PaneId, Response};

use super::Tab;

/// The notice for a browser pane's threads that just lost their element, by first comment.
fn lost_notice(gone: &[String]) -> Option<String> {
    match gone {
        [] => None,
        [one] => {
            let one = one.split_whitespace().collect::<Vec<_>>().join(" ");
            let mut cut: String = one.chars().take(40).collect();
            if cut.len() < one.len() {
                cut.push('…');
            }
            Some(format!(
                "⚠ The thread \"{cut}\" is only in the thread list, because the page lost its element."
            ))
        }
        _ => Some(format!(
            "⚠ {} threads are only in the thread list, because the page lost their elements.",
            gone.len()
        )),
    }
}

impl Tab {
    /// Take each page's news: a new URL goes into its panel's path, a failed load into a notice, and
    /// the agent's capture of it to the view.
    /// When Laura's browser exits, drop it and its pages, and keep the panes for `Ctrl+R`.
    pub(super) fn poll_browsers(&mut self) -> bool {
        if self.browser.as_ref().is_some_and(|b| b.cdp.closed()) {
            // The focused browser pane, else the first one: `Ctrl+R` there starts the browser.
            let browser_pane = |id: &PaneId| self.panels.get(id).is_some_and(|p| p.browser);
            let pane = Some(self.focus).filter(browser_pane).or_else(|| {
                self.panels
                    .keys()
                    .filter(|id| browser_pane(id))
                    .min()
                    .copied()
            });
            self.browsers.clear();
            self.browser = None;
            self.chrome_port.store(0, Ordering::SeqCst);
            self.notices.push((pane, browser::EXITED.into()));
            return true;
        }
        let (now, mut changed, mut closed, mut crashed) = (Instant::now(), false, vec![], vec![]);
        let held = self.held();
        for (id, page) in &mut self.browsers {
            for event in page.poll(now) {
                changed = true;
                match event {
                    browser::Event::Closed => closed.push(*id),
                    browser::Event::Crashed => crashed.push(*id),
                    browser::Event::Url(url) => {
                        if let Some(p) = self.panels.get_mut(id) {
                            p.path = url;
                        }
                    }
                    browser::Event::Failed { url, error } => self.notices.push((
                        Some(*id),
                        format!("⚠ The page {url} did not load ({error})."),
                    )),
                    browser::Event::Download => self.notices.push((
                        Some(*id),
                        "⚠ Laura's browser does not support downloads.".into(),
                    )),
                    browser::Event::Lost { url, lines } => {
                        let gone = match self.panels.get_mut(id) {
                            Some(p) => p.set_lost(&url, &lines),
                            None => vec![],
                        };
                        if let Some(n) = lost_notice(&gone) {
                            self.notices.push((Some(*id), n));
                        }
                    }
                }
            }
            if let Some(p) = self.panels.get(id) {
                page.draw(p.scene(self.focus == *id, held));
            }
        }
        // A capture of a page that no pane shows, such as one the tools opened, shows nothing.
        for c in self.captures.iter().flat_map(Receiver::try_iter) {
            let every = c.target == cdp_proxy::EVERY_PAGE;
            if every && let cdp_proxy::Kind::Recording(on) = c.kind {
                if on {
                    self.traces.insert(c.conn);
                } else {
                    self.traces.remove(&c.conn);
                }
            }
            for page in self
                .browsers
                .values_mut()
                .filter(|p| every || p.target() == c.target)
            {
                match c.kind {
                    cdp_proxy::Kind::Screenshot => page.screenshot(),
                    cdp_proxy::Kind::Recording(on) => page.record(c.conn, on),
                }
                changed = true;
            }
        }
        // No reopen here, so a page that crashes as it loads doesn't loop: `Ctrl+R` opens it.
        for id in crashed {
            self.browsers.remove(&id);
            self.notices.push((
                Some(id),
                "⚠ To load the page again after it stopped, press Ctrl+R.".into(),
            ));
        }
        // The agent's tools closed a pane's page: open it again, and `draw` draws its threads.
        if !closed.is_empty() {
            for id in &closed {
                self.browsers.remove(id);
            }
            // A browser that is exiting closes its pages first; the next poll shows `EXITED`.
            if let Err(e) = self.attach_pages()
                && !self.browser.as_ref().is_some_and(|b| b.cdp.closed())
            {
                self.notices.push((None, format!("⚠ {e}")));
            }
        }
        changed
    }

    /// Take Laura's browser once its start ends: apply the agent's parked `open <url>`, then open
    /// every other browser pane's page. On a failure, journal the cause and answer with the reason.
    pub(super) fn poll_start(&mut self, area: Rect) -> bool {
        let Some((start, pane)) = &self.starting else {
            return false;
        };
        let Some(res) = start.poll() else {
            return false;
        };
        let pane = *pane;
        self.starting = None;
        let parked = self.parked.take();
        match res {
            Ok(b) => {
                self.chrome_port.store(b.port, Ordering::SeqCst);
                self.browser = Some(b);
                // The parked message first: an `open <url> --panel` opens its pane's page at the
                // new URL, and `attach_pages` doesn't open it at the old one.
                if let Some((msg, reply)) = parked {
                    let resp = self.apply(msg, area);
                    reply.send(&resp);
                }
                if let Err(e) = self.attach_pages() {
                    self.notices.push((pane, format!("⚠ {e}")));
                }
            }
            Err(e) => {
                let reason = format!("{:#}", e.reason);
                self.log_event(json!({"type": "browser", "error": reason, "output": e.output}));
                if let Some((_, reply)) = parked {
                    reply.send(&Response::Error {
                        message: format!("couldn't start Laura's browser: {reason}"),
                    });
                }
                // The cause is in the journal; the notice stays fixed.
                if pane.is_some() {
                    self.notices.push((pane, browser::START_FAILED.into()));
                }
            }
        }
        true
    }

    /// Why `open <url>` can't open a browser pane, in the order the user fixes them: the web app,
    /// then a browser to run.
    pub(super) fn browser_gate(&self) -> Result<PathBuf, String> {
        if !self.web {
            return Err(browser::NEED_WEB.into());
        }
        self.browser_path.clone()
    }

    /// Start Laura's browser off the loop, unless it is starting; `poll_start` takes it. `pane`
    /// gets the notice if the start fails.
    pub(super) fn start_browser(&mut self, pane: Option<PaneId>) -> Result<(), String> {
        let path = self.browser_gate()?;
        match &mut self.starting {
            Some((_, p)) => *p = p.or(pane),
            None => {
                let rx = Browser::launch(&path);
                self.starting = Some((rx, pane));
            }
        }
        Ok(())
    }

    /// Laura's browser's CDP client.
    fn cdp(&self) -> Result<Cdp, String> {
        self.browser
            .as_ref()
            .map(|b| b.cdp.clone())
            .ok_or_else(|| browser::EXITED_AGENT.to_string())
    }

    /// Open a page for every browser pane without one, at its URL. A page that fails to open
    /// doesn't stop the others.
    pub(super) fn attach_pages(&mut self) -> Result<(), String> {
        self.cdp()?;
        let missing: Vec<(PaneId, String)> = self
            .panels
            .iter()
            .filter(|(id, p)| p.browser && !self.browsers.contains_key(id))
            .map(|(id, p)| (*id, p.path.clone()))
            .collect();
        let mut failures = vec![];
        for (id, url) in missing {
            match self.open_page(id, &url) {
                Ok(page) => {
                    self.browsers.insert(id, page);
                }
                Err(e) => failures.push(e),
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }

    /// Open pane `id`'s page at `url`, at the pane's size.
    /// shortcut: `Page::open`'s setup calls still block the loop, up to 5 s each on a slow
    /// browser; move them off the loop, as `Browser::launch` is, if that is reported.
    pub(super) fn open_page(&mut self, id: PaneId, url: &str) -> Result<Page, String> {
        let cdp = self.cdp()?;
        let size = self
            .browser_sizes
            .get(&id)
            .copied()
            .unwrap_or(browser::DEFAULT_SIZE);
        let mut page =
            Page::open(&cdp, url, size).map_err(|e| format!("couldn't open {url}: {e:#}"))?;
        for &conn in &self.traces {
            page.record(conn, true);
        }
        if let Some(b) = &mut self.browser {
            b.close_start();
        }
        Ok(page)
    }
}
