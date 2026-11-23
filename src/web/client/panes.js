// The panes: each pane's element, the layout that places them, the drawer and its grip, a browser
// pane's pointer, and the DOM focus, which follows the workspace's focus.

import { $, act, drawerOpen, focused, layout, pane, root, send, setLayout, terms, toShell, ui, ws } from "./state.js";
import { ch, fitShell, makeTerm } from "./term.js";
import { fitHead, fitNarrow, head, headClick, ic, paintChipDot, paintHead, tip, tooSmall } from "./head.js";
import { drawFrame, emuFit, paintMarks } from "./browser-pane.js";
import { paintHint, paintNotice, paintStatus } from "./bottom.js";
import { begin, cardClick, draftBox, paintThreads } from "./review.js";
import { frameOf, place, toHighlight } from "./pages.js";
import { paintSelector, pq } from "./selector.js";
import { mods } from "./keys.js";
import { paintTitle } from "./workspaces.js";

// The focus that DOM focus last followed (`followFocus`).
let focusKey = "";

// Panes that reloaded or got a new highlight while another pane had the focus.
export const unseen = new Set();

// The last pane other than the shell that had the focus, where `Ctrl+\`` goes from the shell.
export let lastFocus = null;

// A frozen file pane's banner, which CSS shows; `paintHead` sets the name.
const BANNER = '<div class="banner"><span><b class="bn"></b> Laura keeps showing the snapshot you are reviewing.</span>'
  + '<span class="grow"></span><button class="btn pri" data-act="review">Submit review <kbd>Shift+S</kbd></button>'
  + '<button class="btn" data-act="refresh">Refresh <kbd>Ctrl+R</kbd></button></div>';

function makePane(p) {
  const el = document.createElement("div");
  el.id = "p" + p.id;
  el.className = "pane";
  el.innerHTML = head() + BANNER
    + `<div class="term"></div><div class="view"><canvas></canvas></div><div class="rows"></div><div class="narrow"></div><button class="cbtn" ${tip("Comment  c")} style="visibility: hidden">${ic("plus")}</button>`
    + '<div class="cmhint">Comment mode · click an element to comment <kbd>Esc</kbd><button data-act="cm">Done</button></div>';
  // A clicked button keeps no focus, so a later Enter can't press it again.
  for (const b of el.querySelectorAll(".head, .banner, .rows, .cbtn, .cmhint")) {
    b.addEventListener("mousedown", e => e.target.closest("button") && e.preventDefault());
  }
  const rows = el.querySelector(".rows"), btn = el.querySelector(".cbtn");
  btn.addEventListener("click", () => {
    if (ui.draft) return followFocus(true);
    if (layout.focus !== p.id) act({ act: "focus", pane: p.id });
    act({ act: "set_cursor", pane: p.id, line: +btn.dataset.line });
    begin(p.id, "comment");
  });
  for (const b of el.querySelectorAll(".head, .banner, .cmhint")) b.addEventListener("click", e => headClick(p.id, e));
  // The `+` follows the hovered line: a row, a block, or a block's code line, table row or item.
  rows.addEventListener("mouseover", e => {
    const t = e.target.closest("[data-line]"), u = t?.closest(".rows > [data-row]");
    el.plus = u && { row: u.dataset.row, line: t.dataset.line, inner: t !== u };
    placePlus(p.id);
  });
  rows.addEventListener("scroll", () => placePlus(p.id));
  rows.addEventListener("click", e => cardClick(p.id, e));
  const banner = el.querySelector(".banner");
  new ResizeObserver(() => el.style.setProperty("--bn", banner.offsetHeight + "px")).observe(banner);
  // `data-fit` doesn't resize the pane, so this doesn't loop.
  new ResizeObserver(() => fitHead(el)).observe(el);
  // Any change to the header (a chip, the spinner, the live badge, a name) fits it again. The
  // chip dot's own change is taken, so it doesn't call this again.
  const mo = new MutationObserver(() => (paintChipDot(el), mo.takeRecords()));
  mo.observe(el.querySelector(".head"), { childList: true, characterData: true, subtree: true, attributeFilter: ["hidden", "class"] });
  // A page's thread or comment box grows as the user types.
  new ResizeObserver(() => place(p.id)).observe(rows);
  el.lags = [];
  new ResizeObserver(() => sizeView(p.id)).observe(el.querySelector(".view"));
  // The line's box changes with the pane and when the line shows; `data-clip` changes only the
  // font, so this doesn't loop. A terminal that shows again takes the focus that it could not take
  // while it was hidden, and rows that show again scroll to the highlight they missed.
  new ResizeObserver(() => {
    fitNarrow(el);
    if (layout?.focus === p.id) followFocus(true);
    if (el.hlLater && !tooSmall(el)) toHighlight(p.id, el.hlLater.jump);
  }).observe(el.querySelector(".narrow"));
  if (p.id === 0) grip(el);
  pointer(el, p.id);
  // The address field: a click selects the URL, Enter goes there (the keydown listener), and
  // leaving it puts the pane's URL back.
  const url = el.querySelector(".head .url");
  url.addEventListener("focus", () => (url.select(), paintHint()));
  url.addEventListener("blur", () => ((url.value = pane(p.id)?.path ?? ""), paintHint()));
  el.addEventListener("mousedown", () => {
    const k = pane(p.id)?.kind;
    if (!ui.draft && k && k !== "file" && layout.focus !== p.id) act({ act: "focus", pane: p.id });
  });
  el.addEventListener("click", e => {
    if (!getSelection().isCollapsed) return;
    // A draft keeps its pane and line; a click only gives the draft its focus back.
    if (ui.draft) return followFocus(true);
    if (pane(p.id)?.kind !== "file") return;
    if (layout.focus !== p.id) act({ act: "focus", pane: p.id });
    const row = e.target.closest("[data-line]");
    if (row) act({ act: "set_cursor", pane: p.id, line: +row.dataset.line });
    const a = e.target.closest("a[data-href]");
    if (a) act({ act: "open_link", pane: p.id, href: a.dataset.href });
  });
  return el;
}

// A resize handle: `move(e)` while the user drags `el` with the left button, `done()` on release.
// One helper for every resize handle.
function dragger(el, move, done) {
  el.addEventListener("pointerdown", e => {
    if (e.button !== 0) return;
    e.preventDefault();
    el.setPointerCapture(e.pointerId);
    el.classList.add("on");
    // After `pointerup`, `pointercancel`, or any other loss of the capture, so a drag always ends.
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("lostpointercapture", up);
      el.classList.remove("on");
      done();
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("lostpointercapture", up);
  });
}

// The drawer's heights in %: 20 or the 5 rows' minimum (`--drawer-min`, `fitShell`), to 85.
const drawerRange = () => [
  Math.min(85, Math.max(20, Math.ceil(parseFloat(root.style.getPropertyValue("--drawer-min")) / root.clientHeight * 100) || 0)),
  85,
];

// The height that the user set and Laura hasn't confirmed: a layout sent before Laura took it would
// show the old height.
let heldDrawer = null;
// A new connection: Laura confirms no height that the last one sent.
export function forgetDrawer() {
  heldDrawer = null;
}

// Save the drawer's height, or the default for `undefined`.
export function saveDrawer(h) {
  if (ws?.readyState !== 1) return;
  heldDrawer = h ?? null;
  act({ act: "drawer_height", percent: h }, () => heldDrawer === h && (heldDrawer = null));
}

// The drawer's height in % as the user sees it: the 5 rows can make the drawer taller than `--drawer`.
const shownDrawer = () => Math.max(drawerRange()[0],
  Math.round(parseFloat(root.style.getPropertyValue("--drawer")) || layout.drawer_height));

// Tell a screen reader the height that the drawer shows, and the lowest height the window allows.
export function ariaDrawer(g = $("p0")?.querySelector(".grip")) {
  g?.setAttribute("aria-valuemin", drawerRange()[0]);
  g?.setAttribute("aria-valuenow", shownDrawer());
}

// Show the drawer at `h`%, and tell a screen reader.
const showDrawer = h => (root.style.setProperty("--drawer", h + "%"), ariaDrawer());

// `Ctrl+Shift+↑ ↓`, or `↑ ↓` on the grip: the drawer `d`% taller, within its range. The shell's
// `ResizeObserver` fits the shell.
export function stepDrawer(d) {
  const [lo, hi] = drawerRange();
  const cur = shownDrawer();
  const h = Math.min(hi, Math.max(lo, cur + d));
  showDrawer(h);
  saveDrawer(h);
}

// The drawer's grip on its top edge: a drag sets the drawer's height, which Laura saves for the
// user; a double-click goes back to the default. A screen reader and the keyboard reach the grip as
// a separator: `↑ ↓` step the height, and `Home` resets it. The window's keydown listener takes these
// keystrokes, after the modes that `mode` names.
const GRIP_TIP = "Drag to resize the drawer · double-click to reset  Ctrl+Shift+↑ ↓";

function grip(el) {
  const g = document.createElement("div");
  g.className = "grip tip-c";
  g.dataset.tip = GRIP_TIP;
  g.tabIndex = 0;
  for (const [k, v] of Object.entries({ role: "separator", "aria-orientation": "horizontal", "aria-valuemax": 85,
    "aria-label": "Drawer height" })) g.setAttribute(k, v);
  el.append(g);
  ariaDrawer(g);
  // The pane's own handlers would take the focus or the line.
  for (const t of ["mousedown", "click"]) g.addEventListener(t, e => e.stopPropagation());
  let h = null;
  dragger(g, e => {
    const r = root.getBoundingClientRect();
    // The drawer sits 8px above the workspace's bottom (style.css).
    const [lo, hi] = drawerRange();
    h = Math.round(Math.min(hi, Math.max(lo, (r.bottom - 8 - e.clientY) / r.height * 100)));
    ui.resizing = true;
    showDrawer(h);
    g.dataset.tip = `Drawer · ${h}% of the height`;
  }, () => {
    ui.resizing = false;
    g.dataset.tip = GRIP_TIP;
    if (h !== null) saveDrawer(h);
    h = null;
    fitShell();
  });
  g.addEventListener("dblclick", () => saveDrawer());
}

// The `+` on the hovered line (`el.plus`), just past its text and centred on its first line. A click
// or a reload replaces the line's element, so `onPane` places the `+` again from the new one.
export function placePlus(id) {
  const el = $("p" + id), rows = el?.querySelector(".rows"), btn = el?.querySelector(".cbtn"), q = pane(id), h = el?.plus;
  if (!btn) return;
  const u = h && rows.querySelector(`:scope > [data-row="${h.row}"]`);
  const t = h?.inner ? u?.querySelector(`[data-line="${h.line}"]`) : u;
  // No `+` while a comment is open, in the diff view, or before `laura ready`, which a comment needs.
  const show = t && !ui.draft && q?.kind === "file" && !q.diff && layout.agent;
  btn.style.visibility = show ? "" : "hidden";
  if (!show) return;
  // A block's text, past its gutter and its padding.
  const m = t.querySelector(":scope > .md") ?? t;
  const lh = parseFloat(getComputedStyle(m.firstElementChild ?? m).lineHeight) || ch;
  // A tall block's top can scroll out; the `+` stays on the visible rows, inside the pane.
  const box = el.getBoundingClientRect(), rb = rows.getBoundingClientRect(), r = document.createRange();
  r.selectNodeContents(m);
  const top = Math.max(m.getBoundingClientRect().top, rb.top);
  btn.style.top = top - box.top + (lh - 24) / 2 + "px";
  btn.style.left = Math.min(r.getBoundingClientRect().right + 6, rb.right - 30) - box.left + "px";
  btn.dataset.line = t.dataset.line;
}

export function onLayout(m) {
  if (m.focus !== 0) lastFocus = m.focus;
  for (const p of m.panes) {
    const old = layout && (pane(p.id) ?? {});
    if (old && p.id !== m.focus && p.highlight && String(p.highlight) !== String(old.highlight)) unseen.add(p.id);
  }
  unseen.delete(m.focus);
  setLayout(m);
  // A drag keeps the height under the pointer until the release.
  if (!ui.resizing && heldDrawer === null) showDrawer(m.drawer_height);
  // The list closes with its last thread, or its pane.
  if (!pane(ui.listOpen)?.threads) ui.listOpen = null;
  if (!pane(ui.moreOpen) || ui.moreOpen === 0) ui.moreOpen = null;
  const seen = new Set(), later = root.children.length > 0;
  for (const p of m.panes) {
    // A pane that opens after the first layout eases in.
    const fresh = !$("p" + p.id) && later;
    const el = $("p" + p.id) ?? root.appendChild(makePane(p));
    if (fresh) el.animate([{ opacity: 0, transform: "translateY(6px)" }, {}], { duration: 350, easing: "ease-out" });
    seen.add(el.id);
    if (p.pty && !terms.has(p.id)) makeTerm(p.id, el);
    // CSS places the drawer.
    Object.assign(el.style, p.drawer ? { left: "", top: "", width: "", height: "" } : {
      left: `calc(var(--cw) * ${p.x})`, top: `calc(var(--ch) * ${p.y})`,
      width: `calc(var(--cw) * ${p.w})`, height: `calc(var(--ch) * ${p.h})`,
    });
    el.style.setProperty("--w", p.w);
    // A browser pane that turned into a file pane drops the page's state.
    if (p.kind !== "browser" && el.classList.contains("browser")) {
      el.emulated = el.dialog = el.marks = null;
      el.typing = el.acting = false;
      el.querySelector(".view").classList.remove("emu");
      el.querySelector(".dlgwrap")?.remove();
      el.querySelector(".marks")?.remove();
    }
    el.className = `pane ${p.kind}${p.id === m.focus ? " focused" : ""}${p.frozen ? " frozen" : ""}`
      + (p.drawer ? ` drawer${drawerOpen() ? " open" : ""}` : "")
      + (unseen.has(p.id) ? " unseen" : "") + (ui.commentMode === p.id ? " comment-mode" : "");
    paintHead(el, p);
    const t = p.id !== 0 && terms.get(p.id);
    // The header takes the border's top two rows, and the bottom border one (`Tab::inner`). The
    // shell sizes itself (`fitShell`).
    if (t && (t.cols !== p.w - 2 || t.rows !== p.h - 3)) t.resize(Math.max(1, p.w - 2), Math.max(1, p.h - 3));
    // The focus, the diff view and a freeze change how threads show.
    paintThreads(p.id);
    sizeView(p.id);
  }
  for (const el of [...root.children]) {
    if (seen.has(el.id)) continue;
    const id = +el.id.slice(1);
    if (id === ui.commentMode) ui.commentMode = null;
    terms.get(id)?.dispose();
    terms.delete(id);
    el.remove();
  }
  followFocus();
  paintHint();
  paintStatus();
  paintTitle();
  paintNotice();
  // A pane that closes or freezes changes the pane selector's rows.
  if (ui.popup !== null) paintSelector();
}

// A browser pane's size in CSS pixels, with a whole number of device pixels each way, so a frame
// draws 1:1. Laura's browser renders the page at this size.
export function sizeView(id) {
  const el = $("p" + id), view = el?.querySelector(".view");
  if (pane(id)?.kind !== "browser" || !view.clientWidth) return;
  const dpr = devicePixelRatio || 1;
  const fit = n => {
    // Chrome's zoomed devicePixelRatio is a float32 (1.100000023841858), so match loosely.
    for (let i = 0; i < 64 && Math.abs(n * dpr - Math.round(n * dpr)) > 1e-3; i++) n--;
    return n;
  };
  const [w, h] = [fit(view.clientWidth), fit(view.clientHeight)];
  const key = `${w}x${h}@${dpr}`;
  if (view.dataset.key === key) return;
  view.dataset.key = key;
  const c = view.querySelector("canvas");
  Object.assign(c.style, { width: w + "px", height: h + "px" });
  [c.width, c.height] = [Math.round(w * dpr), Math.round(h * dpr)];
  drawFrame(el);
  place(id);
  paintMarks(id);
  send({ size: { id, w, h, dpr } });
}

// A browser pane's pointer on its canvas, in CSS px from the canvas's top left. Moves and the wheel
// go once per animation frame; the capture keeps a drag that leaves the canvas.
function pointer(el, id) {
  const c = el.querySelector(".view canvas");
  const at = e => {
    const r = c.getBoundingClientRect(), { x, y, s } = emuFit(el);
    return { x: (e.clientX - r.left - x) / s, y: (e.clientY - r.top - y) / s, mods: mods(e) };
  };
  const mouse = m => send({ act: "browser_mouse", pane: id, ...m });
  const flush = () => {
    cancelAnimationFrame(el.raf);
    el.raf = 0;
    if (el.move) mouse(el.move);
    if (el.wheel) mouse(el.wheel);
    el.move = el.wheel = null;
  };
  const later = () => (el.raf ||= requestAnimationFrame(flush));
  const off = () => pane(id)?.kind !== "browser" || el.dialog || ui.draft;
  // In comment mode, a click picks the element under it, and the page gets no press.
  c.addEventListener("pointerdown", e => {
    if (off() || ui.commentMode === id) return;
    // A browser counts a press within 500 ms and 5 px of the last as the next click.
    const p = at(e), last = el.press, now = performance.now();
    const again = last && now - last.t < 500 && Math.hypot(p.x - last.x, p.y - last.y) < 5 && last.button === e.button;
    el.press = { ...p, t: now, button: e.button, clicks: again ? last.clicks + 1 : 1 };
    el.held = true;
    c.setPointerCapture(e.pointerId);
    flush();
    mouse({ kind: "down", ...p, button: e.button, buttons: e.buttons, clicks: el.press.clicks });
  });
  c.addEventListener("pointermove", e => {
    // Where `c` comments.
    el.at = at(e);
    if (off()) return;
    el.move = { kind: "move", ...at(e), buttons: e.buttons };
    later();
  });
  c.addEventListener("pointerup", e => {
    if (off()) return;
    if (ui.commentMode === id && !el.held) {
      const { x, y } = at(e);
      return begin(id, "comment", { act: "browser_pick", pane: id, x, y });
    }
    if (!el.held) return;
    el.held = false;
    flush();
    mouse({ kind: "up", ...at(e), button: e.button, buttons: e.buttons, clicks: el.press.clicks });
  });
  // A point off the page ends the page's hover.
  c.addEventListener("pointerleave", () => {
    el.at = null;
    if (el.held || off()) return;
    el.move = { kind: "move", x: -1, y: -1, buttons: 0 };
    later();
  });
  c.addEventListener("wheel", e => {
    e.preventDefault();
    if (off()) return;
    // A line is 40 px, as in Chrome; a page is the pane's height.
    const k = [1, 40, c.clientHeight][e.deltaMode] ?? 1;
    const w = el.wheel ?? { kind: "wheel", dx: 0, dy: 0 };
    el.wheel = { ...w, ...at(e), dx: w.dx + e.deltaX * k, dy: w.dy + e.deltaY * k };
    later();
  }, { passive: false });
  c.addEventListener("contextmenu", e => e.preventDefault());
}

// DOM focus follows the workspace's focus, only when it changes, so a title or resize frame doesn't
// take focus from anything else.
export function followFocus(force) {
  const f = focused();
  const key = `${layout?.focus}:${f?.kind}:${ui.locked}:${!!ui.draft}`;
  if (!f || (key === focusKey && !force)) return;
  focusKey = key;
  // A frame that moves the focus doesn't take the keystrokes from the pane selector.
  if (ui.popup !== null) return pq.focus();
  const box = draftBox();
  if (ui.draft && !ui.locked && box) return box.focus();
  // A click in a page's text field, the address field or a dialog's field keeps the field's focus.
  if (document.activeElement === frameOf(f.id)) return;
  if (document.activeElement?.closest?.(`#p${f.id} :is(.head .url, .dlg input)`)) return;
  const t = toShell() ? terms.get(0) : f.kind !== "file" ? terms.get(f.id) : null;
  if (t) return t.focus();
  for (const t of terms.values()) t.blur();
  // Below the workspace's minimum size the window scrolls; a focus change leaves it where the user put it.
  root.focus({ preventScroll: true });
}
// A new connection moves the DOM focus again.
export function resetFocus() {
  focusKey = "";
}
