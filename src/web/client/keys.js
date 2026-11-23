// The keystrokes that Laura takes: one function for each mode (`mode`) and for each kind of
// focused pane, the keystrokes for a browser pane's page, and the window's paste, copy and resize.

import { $, act, drawerOpen, focused, layout, mode, modes, MODIFIERS, pane, root, send, terms, toShell, ui } from "./state.js";
import { fitShell, report } from "./term.js";
import { focusPane, followFocus, lastFocus, saveDrawer, sizeView, stepDrawer } from "./panes.js";
import { answer } from "./browser-pane.js";
import { paintHint, paintShellButton, toggleShell } from "./bottom.js";
import { begin, closeDraft, confirmDraft, draftBox, paintPops, units } from "./review.js";
import { frameOf, setCommentMode } from "./pages.js";
import { closeSelector, openSelector, paintSelector, pickRow, pq, toggleHelp } from "./selector.js";
import { closeMenu, go, leaving } from "./workspaces.js";
import { ended } from "./main.js";

// An event's modifiers as CDP counts them: Alt 1, Ctrl 2, Meta 4, Shift 8.
export const mods = e => e.altKey | e.ctrlKey << 1 | e.metaKey << 2 | e.shiftKey << 3;
// The keystrokes a browser pane's page scrolls with when no field has the keystrokes.
const SCROLL_KEYS = ["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "PageUp", "PageDown", "Home", "End", " "];
// The pane each press went to, by `code`: that pane's page takes the release too, and no other
// page takes a release.
const sent = new Map();
const toPage = (id, e) => (sent.set(e.code, id), send(browserKey(id, "down", e)));
// A keystroke for a browser pane's page. AltGr reads as Ctrl+Alt, and still types its character.
const browserKey = (pane, kind, e) => ({
  act: "browser_key", pane, kind, key: e.key, code: e.code, key_code: e.keyCode, mods: mods(e),
  text: [...e.key].length === 1 && !e.metaKey && e.ctrlKey === e.altKey ? e.key : "",
});

const FILE_KEYS = {
  ArrowUp: p => ({ act: "move_cursor", pane: p, delta: -1 }),
  ArrowDown: p => ({ act: "move_cursor", pane: p, delta: 1 }),
  ArrowLeft: p => ({ act: "scroll_h", pane: p, delta: -1 }),
  ArrowRight: p => ({ act: "scroll_h", pane: p, delta: 1 }),
  d: p => ({ act: "toggle_diff", pane: p }),
  p: p => ({ act: "toggle_pause", pane: p }),
  r: p => ({ act: "toggle_thread", pane: p }),
  n: p => ({ act: "jump_thread", pane: p, next: true }),
  N: p => ({ act: "jump_thread", pane: p, next: false }),
  x: p => ({ act: "close", pane: p }),
  h: p => ({ act: "clear_highlight", pane: p }),
};

// Ends a keystroke that Laura takes: the page and its other listeners don't see it.
function take(e) {
  e.preventDefault();
  e.stopPropagation();
}

// The pane that the lock moved Laura's focus from (`lockKey`).
let lockFrom = null;

// F12 turns the lock on or off. From a file pane or a browser pane, the lock moves Laura's focus to
// the shell, which takes the keystrokes, and the unlock moves it back, unless the user moved it.
// Under the pane selector, the help or the quit question, the user hasn't reached the shell, and
// the wait rule ends the wait on the close. A draft holds its pane. Either way, the focus stays.
function lockKey(e) {
  take(e);
  ui.locked = !ui.locked;
  let back = false;
  // shortcut: a lock under another mode leaves Laura's focus on the pane after the close; move it in the close if users hit it.
  if (modes().join() === "locked" && toShell() && layout.focus !== 0) {
    lockFrom = layout.focus;
    focusPane(0);
  } else if (!ui.locked) {
    // shortcut: an unlock before the lock's reply (F12 F12 within one round trip) leaves the focus in the shell; track the move in flight if users hit it.
    // The user moved the focus to another pane and back to the shell: the focus stays.
    back = lockFrom !== null && layout.focus === 0 && lastFocus === lockFrom && !!pane(lockFrom);
    if (back) focusPane(lockFrom);
    lockFrom = null;
  }
  // An unlock sends no layout, so the drawer and the shell button repaint here.
  if (pane(0)?.drawer) $("p0").classList.toggle("open", drawerOpen()), paintShellButton();
  // Laura's focus is still on the shell until the reply: the shell must not take the keystrokes meanwhile.
  // shortcut: a keystroke between the unlock and Laura's reply goes to no pane; track the move in flight if users hit it.
  if (back) {
    terms.get(0)?.blur();
    root.focus({ preventScroll: true });
  } else followFocus(true);
  paintHint();
}

// The workspace switcher's menu takes every keystroke while it is open.
function menuKey(e) {
  take(e);
  const items = [...$("menu").querySelectorAll(".mi[data-url]")], i = items.findIndex(m => m.classList.contains("on"));
  const mark = n => items.forEach((m, j) => m.classList.toggle("on", j === n));
  if (e.key === "ArrowDown") mark(Math.min(i + 1, items.length - 1));
  else if (e.key === "ArrowUp") mark(Math.max(i - 1, 0));
  else if (e.key === "Enter" && items[i]) (closeMenu(), go(items[i].dataset.url, e.ctrlKey || e.metaKey));
  else if (e.key === "Escape" || e.key === "Tab") closeMenu();
}

// A click can open the pane selector under the lock; it still takes its keystrokes.
function selectorKey(e, ctrl, k) {
  // Ctrl+R/L/Q/H and Tab would reach the browser; every other keystroke goes to the input.
  if (e.key === "Escape" || (ctrl && k === "p")) return take(e), closeSelector();
  if (e.key === "ArrowUp" || e.key === "ArrowDown") {
    take(e);
    ui.popup += e.key === "ArrowDown" ? 1 : -1;
    return paintSelector();
  }
  if (e.key === "Enter" && !e.isComposing) {
    take(e);
    // The headings are not rows: count only the rows.
    return pickRow($("prows").querySelectorAll(".prow")[ui.popup], e.ctrlKey || e.metaKey);
  }
  if ((ctrl && "rlqh".includes(k)) || e.key === "Tab") return take(e);
  if (document.activeElement !== pq) pq.focus();
}

// The quit question: `y` quits, and any other keystroke closes the question.
function quitKey(e) {
  take(e);
  ui.confirmQuit = false;
  if (e.key === "y") send({ quit: true });
  paintHint();
}

function draftKey(e, ctrl, k) {
  // Enter confirms; Shift+Enter falls through as a newline. Ctrl+R/L/P/Q and Tab would reach the browser.
  if (e.key === "Enter" && !e.shiftKey && !e.isComposing) return take(e), confirmDraft();
  if (e.key === "Escape") return take(e), closeDraft();
  if ((ctrl && "rlpq".includes(k)) || e.key === "Tab") return take(e);
  const box = draftBox();
  if (box && document.activeElement !== box) box.focus();
}

// The address field: Enter opens the typed URL in its pane, Esc puts the pane's URL back. True when
// the field keeps the keystroke.
function urlKey(e, ctrl, k, url) {
  if (e.key === "Enter" && !e.isComposing) {
    take(e);
    if (url.value.trim()) act({ act: "browser_navigate", pane: +url.closest(".pane").id.slice(1), url: url.value });
    return root.focus({ preventScroll: true }), true;
  }
  if (e.key === "Escape") return take(e), root.focus({ preventScroll: true }), true;
  if (ctrl && "rl".includes(k)) return take(e), true; // the viewing browser's reload and address bar
  return !(ctrl && ("pq".includes(k) || e.code === "Backquote")); // Ctrl+P, Ctrl+Q and Ctrl+` go on to Laura
}

// A keystroke that no mode takes: the workspace's keystrokes, then the focused pane's.
function paneKey(e, ctrl, k) {
  const f = focused();
  const url = document.activeElement?.matches?.(".head .url") && document.activeElement;
  if (url && urlKey(e, ctrl, k, url)) return;
  if (e.key === "Escape" && (ui.listOpen ?? ui.moreOpen) !== null) {
    take(e);
    ui.listOpen = ui.moreOpen = null;
    return paintPops();
  }
  // As the hint line: `Esc` leaves comment mode from a file or browser pane, after the pane's own dialog or field.
  // The shell and an editor pane's editor view keep `Esc` for their program.
  const b = f?.kind === "browser" && $("p" + f.id);
  if (e.key === "Escape" && ui.commentMode !== null && (f?.kind === "file" || b) && !b?.dialog && !b?.typing) return take(e), setCommentMode(null);
  if (ctrl && k === "p") return take(e), openSelector();
  if (ctrl && e.code === "Backquote") return take(e), toggleShell();
  if (ctrl && e.shiftKey && (e.key === "ArrowUp" || e.key === "ArrowDown") && pane(0)?.drawer && drawerOpen()) {
    return take(e), stepDrawer(e.key === "ArrowUp" ? 5 : -5);
  }
  // The focused grip: ↑ ↓ step the drawer's height, and Home resets it.
  if (document.activeElement?.matches?.("#p0 .grip") && !e.ctrlKey && !e.altKey && !e.metaKey) {
    if (e.key === "ArrowUp" || e.key === "ArrowDown") return take(e), stepDrawer(e.key === "ArrowUp" ? 5 : -5);
    if (e.key === "Home") return take(e), saveDrawer();
  }
  if (ctrl && k === "q") {
    take(e);
    return act({ act: "quit" }, m => m.ok && ((ui.confirmQuit = true), paintHint()));
  }
  if (ctrl && k === "l" && f && f.pty && f.id !== 0) {
    take(e);
    return act({ act: "toggle_editor_view", pane: f.id });
  }
  if (f?.kind === "browser") return browserPaneKey(e, ctrl, k, f.id);
  if (f?.kind === "file") filePaneKey(e, ctrl, k, f.id);
}

function browserPaneKey(e, ctrl, k, id) {
  const el = $("p" + id);
  // The page's dialog takes Enter and Esc; a prompt's field takes the rest.
  if (el.dialog) {
    if (e.key === "Enter" && !e.isComposing) return take(e), answer(id, true);
    if (e.key === "Escape") return take(e), answer(id, false);
    return;
  }
  // A field in the page has the keystrokes, all but Esc. Ctrl+V (Cmd+V) goes on as a paste.
  if (el.typing) {
    if (e.key === "Escape") return take(e), (el.typing = false), send({ act: "browser_blur", pane: id }), paintHint();
    if ((ctrl || e.metaKey) && e.key.toLowerCase() === "v") return;
    return take(e), toPage(id, e);
  }
  if (e.altKey && !e.ctrlKey && !e.metaKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
    return take(e), act({ act: "browser_history", pane: id, delta: e.key === "ArrowLeft" ? -1 : 1 });
  }
  if (!mods(e) && SCROLL_KEYS.includes(e.key)) return take(e), toPage(id, e);
  if (ctrl && k === "r") return take(e), act({ act: "refresh", pane: id });
  if (ctrl && k === "l") return take(e); // the browser's address bar
  const bare = !ctrl && !e.altKey && !e.metaKey;
  // `c` comments on the element under the pointer; off the page, it turns on comment mode.
  // Before `laura ready`, the pick returns the notice that asks for it.
  if (bare && e.key === "c") {
    take(e);
    // On a pin, `c` comments in the pin's thread.
    if (el.pinAt) return act({ act: "set_cursor", pane: id, line: el.pinAt }), begin(id, "comment");
    if (!el.at && layout.agent) return setCommentMode(id);
    const { x, y } = el.at ?? { x: -1, y: -1 };
    return begin(id, "comment", { act: "browser_pick", pane: id, x, y });
  }
  if (bare && e.key === "S") return take(e), begin(id, "review");
  if (bare && e.key === "Escape") return take(e), focusPane(0);
  if (bare && ["x", "n", "N", "r", "h"].includes(e.key)) return take(e), act(FILE_KEYS[e.key](id));
}

function filePaneKey(e, ctrl, k, id) {
  if (ctrl && k === "r") {
    take(e);
    return act({ act: "refresh", pane: id });
  }
  if (ctrl && k === "l") return take(e); // the browser's address bar
  const bare = !ctrl && !e.altKey && !e.metaKey;
  if (bare && e.key === "Escape") return take(e), focusPane(0);
  if (bare && (e.key === "c" || e.key === "S")) {
    take(e);
    // A page with no element at the cursor has nothing to comment on: pick one.
    const fr = frameOf(id);
    if (e.key === "c" && layout.agent && fr && !fr.rect) return setCommentMode(id);
    return begin(id, e.key === "c" ? "comment" : "review");
  }
  // The arrows don't move the cursor in a page, which has no rows.
  if (e.key.startsWith("Arrow") && frameOf(id)) return;
  const make = bare && FILE_KEYS[e.key];
  if (make) {
    take(e);
    act(make(id));
  }
}

addEventListener("keydown", e => {
  const m = mode();
  if (!layout || ended || m === "overlay" || MODIFIERS.includes(e.key)) return;
  const ctrl = e.ctrlKey && !e.altKey;
  const k = e.key.length === 1 && ctrl ? e.key.toLowerCase() : e.key;
  if (e.key === "F12") return lockKey(e);
  if (m === "menu") return menuKey(e);
  if (m === "help") return take(e), toggleHelp();
  if (m === "selector") return selectorKey(e, ctrl, k);
  if (m === "quit") return quitKey(e);
  if (m === "locked") return;
  if (ctrl && k === "h") return take(e), toggleHelp();
  if (m === "draft") return draftKey(e, ctrl, k);
  paneKey(e, ctrl, k);
});

// A browser pane's page takes a paste while a field has the keystrokes.
const typing = () => {
  const f = focused();
  return mode() === null && f?.kind === "browser" && $("p" + f.id).typing && !$("p" + f.id).dialog && f.id;
};

addEventListener("keyup", e => {
  const id = sent.get(e.code);
  if (sent.delete(e.code)) send(browserKey(id, "up", e));
});

addEventListener("paste", e => {
  const id = typing();
  if (!id || e.target.closest?.("input, textarea")) return;
  e.preventDefault();
  send({ act: "browser_text", pane: id, text: e.clipboardData.getData("text") });
});

// Selecting copies on release, as in the TUI. A file pane copies its text without the gutter,
// joining the wrapped rows of one line with a space, as `mouse::extract_panel` does. A selection in
// a thread or popover copies as shown.
addEventListener("mouseup", e => {
  const el = e.target.closest?.(".pane");
  if (!el) return;
  const id = +el.id.slice(1);
  setTimeout(() => {
    const t = terms.get(id);
    if (!["file", "browser"].includes(pane(id)?.kind)) {
      if (t?.hasSelection()) navigator.clipboard?.writeText(t.getSelection());
      return;
    }
    const sel = getSelection();
    if (sel.isCollapsed || !sel.rangeCount) return;
    const range = sel.getRangeAt(0);
    const rows = el.querySelector(".rows");
    let out = "", prev = null;
    for (const row of units(rows)) {
      if (!range.intersectsNode(row)) continue;
      const r = document.createRange();
      r.selectNodeContents(row);
      if (row.contains(range.startContainer)) r.setStart(range.startContainer, range.startOffset);
      if (row.contains(range.endContainer)) r.setEnd(range.endContainer, range.endOffset);
      const frag = r.cloneContents();
      frag.querySelectorAll(".g").forEach(g => g.remove());
      frag.querySelectorAll("td + td, th + th").forEach(c => c.prepend("\t"));
      if (prev !== null) out += row.dataset.row === prev ? " " : "\n";
      // ponytail: strips push_html's leading newline, and with it a code block's leading blank lines
      out += frag.textContent.replace(/^\n+/, "").trimEnd();
      prev = row.dataset.row;
    }
    if (!out && !e.target.closest("textarea")) out = sel.toString();
    if (out) navigator.clipboard?.writeText(out);
  });
});

addEventListener("resize", () => {
  report();
  fitShell();
  paintHint();
  // A zoom changes the pixel ratio, not always the pane's size.
  layout?.panes.forEach(p => sizeView(p.id));
});

// Ask before a stray Ctrl+W closes the tab.
addEventListener("beforeunload", e => ended || leaving || e.preventDefault());
