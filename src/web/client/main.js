// The web app's client starts here: it loads the fonts, measures the grid's cell, and connects to
// Laura. `connect` reads each frame from Laura and hands it to the module that paints it.

import { $, act, acts, pane, root, send, setWs, terms, ui, ws } from "./state.js";
import { fitShell, makeProbe, measure, report } from "./term.js";
import { forgetDrawer, onLayout, paintGlance, resetFocus } from "./panes.js";
import { onAgent, onBrowser, onFrame, onShot, paintMarks } from "./browser-pane.js";
import { paintHint, shellBusy, showNotice, visit } from "./bottom.js";
import { closeDraft } from "./review.js";
import { frameOf, onPage, onPane, place, setCommentMode } from "./pages.js";
import { paintHelp } from "./selector.js";
import { paintSwitcher, showOverlay } from "./workspaces.js";
// The keydown listener and the window's other listeners.
import "./keys.js";

// The `strings` frame, and whether Laura said the workspace ended.
export let strings = {}, ended = false;
// The `workspaces` frame: the running workspaces, this workspace first, then the latest visit
// first, and the Laura server's URL or null.
export let spaces = [], server = null;

export function connect() {
  // The replay assumes a blank terminal; reset after anything still queued is parsed.
  for (const t of terms.values()) t.write("", () => t.reset());
  resetFocus();
  // Laura holds the last view's sizes, and the old frame is no measure of lag: each browser pane
  // sends its size again, and its first frame adds no lag.
  for (const el of root.querySelectorAll(".pane.browser")) {
    delete el.querySelector(".view").dataset.key;
    el.first = true;
  }
  // Laura turned comment mode off for the new view; `send` is a no-op until the socket opens.
  if (ui.commentMode !== null) setCommentMode(null);
  let laid = false, moved = false;
  const url = new URL("ws" + location.search, location);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  const sock = new WebSocket(url);
  setWs(sock);
  sock.binaryType = "arraybuffer";
  // A switch through the workspace switcher loads this page in the same tab: no focus event.
  sock.onopen = () => (report(), fitShell(true), visit());
  sock.onmessage = e => {
    if (typeof e.data !== "string") {
      const id = Number(new DataView(e.data).getBigUint64(0));
      if (pane(id)?.kind === "browser") return onFrame(id, e.data);
      if (id === 0) shellBusy();
      return terms.get(id)?.write(new Uint8Array(e.data, 8));
    }
    const m = JSON.parse(e.data);
    switch (m.t) {
      case "strings":
        strings = m;
        paintHelp(m.help, m.help_note);
        paintSwitcher();
        break;
      case "workspaces":
        spaces = m.list;
        server = m.server;
        paintSwitcher();
        break;
      case "layout":
        // A draft open when the socket dropped: hold again and refocus its pane, or close it.
        if (!laid && ui.draft) {
          if (m.panes.some(p => p.id === ui.draft.pane)) {
            send({ hold: true });
            // Not the user's move, so not `focusPane`: `layout` is still the old frame here.
            act({ act: "focus", pane: ui.draft.pane });
            m.focus = ui.draft.pane;
            ui.draft.busy = false;
          } else closeDraft();
        }
        laid = true;
        onLayout(m);
        break;
      case "glance":
        paintGlance(m);
        break;
      case "pane":
      case "rows":
        onPane(m);
        break;
      case "page":
        onPage(m);
        break;
      case "browser":
        onBrowser(m);
        break;
      case "editable": {
        const el = $("p" + m.id);
        if (el) (el.typing = m.editable), paintHint();
        break;
      }
      case "agent":
        if (pane(m.id)?.kind === "browser") onAgent(m);
        break;
      case "screenshot":
        if (pane(m.id)?.kind === "browser") onShot(m);
        break;
      case "copy":
        navigator.clipboard?.writeText(m.text).catch(() => {});
        break;
      // A browser pane's cursor element's box, and the boxes it outlines.
      case "rect": {
        const f = pane(m.id)?.kind === "browser" && frameOf(m.id);
        if (f) (f.rect = Array.isArray(m.rect) ? m.rect : null), place(m.id);
        break;
      }
      case "marks": {
        const el = pane(m.id)?.kind === "browser" && $("p" + m.id);
        if (el) (el.marks = m.marks), paintMarks(m.id);
        break;
      }
      case "acted": {
        const a = acts.shift();
        if (!m.ok) showNotice(m.text, 5, a?.pane, !!m.retry);
        a?.then?.(m);
        break;
      }
      case "notice":
        showNotice(m.text, 10, m.pane);
        break;
      case "moved":
        moved = true;
        break;
      case "exit":
        ended = true;
        break;
    }
  };
  sock.onclose = () => {
    if (ws !== sock) return;
    acts.length = 0;
    forgetDrawer();
    if (moved) return showOverlay(strings.moved, true);
    if (ended) {
      // `replace`: Back does not return to the workspace that ended.
      if (server) return location.replace(server);
      document.body.classList.add("ended");
      return showOverlay(strings.ended, false);
    }
    // Laura drops a view that falls behind; a fresh connection gets a fresh replay.
    if (laid) return setTimeout(connect, 500);
    // Hard-coded: with no connection, the page never got a `strings` frame.
    showOverlay("This tab cannot connect to the workspace. Make sure that Laura did not stop and that you used the full URL from the terminal.", false);
  };
}

// The cell size comes from the bundled mono font, not a fallback, so load both before measuring.
Promise.all(["14px Inter", '14px "JetBrains Mono"'].map(f => document.fonts.load(f))).finally(() => {
  makeProbe();
  measure();
  connect();
});
