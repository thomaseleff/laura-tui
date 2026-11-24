// The panes: each pane's element, the layout that places them, the start canvas, the drawer with its
// grip and start height, a browser pane's pointer, and the DOM focus, which follows the workspace's
// focus.

import { $, act, drawerOpen, esc, focused, layout, pane, root, send, setLayout, terms, toShell, ui, ws } from "./state.js";
import { ch, fitShell, makeTerm, rowsPx } from "./term.js";
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
    focusPane(p.id);
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
    if (!ui.draft && k && k !== "file") focusPane(p.id);
  });
  el.addEventListener("click", e => {
    if (!getSelection().isCollapsed) return;
    // A draft keeps its pane and line; a click only gives the draft its focus back.
    if (ui.draft) return followFocus(true);
    if (pane(p.id)?.kind !== "file") return;
    focusPane(p.id);
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

// The drawer's height in % from the layout: over the canvas with no height from the user, the
// start height, which holds `drawer_fit.rows` of the shell (`rowsPx`), up to `drawer_fit.max`.
function drawerHeight(m) {
  const f = m.drawer_fit, px = f && rowsPx(f.rows);
  if (!px) return m.drawer_height;
  return Math.min(f.max, Math.max(m.drawer_height, Math.ceil(px / root.clientHeight * 100)));
}

// Show the layout's drawer height, unless the user drags the grip, Laura hasn't confirmed the
// user's height, or the socket is closed: the new connection's layout brings Laura's height.
// `fitShell` calls this again once it measures the shell's rows.
export function fitDrawer() {
  if (!layout || ui.resizing || heldDrawer !== null || ws?.readyState !== 1) return ariaDrawer();
  showDrawer(drawerHeight(layout));
}

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
  fitDrawer();
  paintCanvas(m);
  // The list closes with its last thread, or its pane.
  if (!pane(ui.listOpen)?.threads) ui.listOpen = null;
  if (!pane(ui.moreOpen) || ui.moreOpen === 0) ui.moreOpen = null;
  const seen = new Set(), later = !!root.querySelector(":scope > .pane");
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
  for (const el of root.querySelectorAll(":scope > .pane")) {
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

// The canvas while no pane but the shell is open: *Get started*, the project at a glance or the calm canvas, under the
// drawer or right of the docked shell. Not a pane: no id, no pane header.
function paintCanvas(m) {
  const c = $("canvas"), shell = pane(0);
  c.hidden = !m.canvas;
  if (!m.canvas) return;
  c.dataset.c = m.canvas;
  c.classList.toggle("docked", !shell?.drawer);
  c.style.left = shell?.drawer ? "" : `calc(var(--cw) * ${shell?.w ?? 0})`;
}

// The steps of *Get started*, which the canvas and the help show.
export const steps = () => $("steps").content.cloneNode(true);
$("canvas").querySelector(".foot").before(steps());
// The canvas is not a pane: a click on it gives the keystrokes back to the shell, except a click
// on a button or a link, or one that selects text to copy.
$("canvas").addEventListener("click", (e) => {
  if (!e.target.closest("button, [data-href]") && !getSelection().toString()) followFocus(true);
});

// The project at a glance: the README's first section, the changes or the latest commit, and the
// recent sessions in the folder. Laura leaves out a card with nothing to show.
export function paintGlance(m) {
  const cards = [];
  if (m.readme) {
    const more = m.readme.more.length ? `<p class="more">${m.readme.more.map(esc).join(" · ")}</p>` : "";
    // The README's HTML is Laura's, as in a `pane` frame.
    cards.push(`<div class="gcard"><h3>${esc(m.readme.name)}<span class="sp"></span><button class="lnk" data-path="${esc(m.readme.name)}">Open as a pane</button></h3>`
      + `<div class="md">${m.readme.html}</div>${more}</div>`);
  }
  const col = [];
  if (m.project) col.push(projectCard(m.project));
  if (m.sessions.length) col.push(`<div class="gcard"><h3>Recently in this folder</h3>${m.sessions.map(sessionRow).join("")}</div>`);
  if (col.length) cards.push(`<div class="col">${col.join("")}</div>`);
  glance.innerHTML = cards.join("");
  glance.classList.toggle("one", cards.length === 1);
}

const glance = $("canvas").querySelector(".glance");
const KINDS = { M: "m", A: "a", D: "d", "?": "u" };
const ago = new Intl.RelativeTimeFormat("en", { numeric: "auto", style: "narrow" });
const UNITS = [["year", 31536e3], ["month", 2592e3], ["week", 604800], ["day", 86400], ["hour", 3600], ["minute", 60]];
function age(ts) {
  const s = (ts - Date.now()) / 1000, [unit, n] = UNITS.find(([, n]) => Math.abs(s) >= n) ?? ["second", 1];
  return ago.format(Math.round(s / n), unit);
}

// A changed file's row: a button that opens the file, in the diff view when `diff`. A deleted file
// has nothing to open.
function fileRow(f, diff) {
  const n = f.status === "?" ? "new"
    : (f.add ? `<span class="add">+${f.add}</span> ` : "") + (f.del ? `<span class="del">−${f.del}</span>` : "");
  const row = `<span class="k ${KINDS[f.status] ?? "m"}">${esc(f.status)}</span><span class="p">${esc(f.path)}</span><span class="n">${n}</span>`;
  return f.status === "D" ? `<div class="frow">${row}</div>`
    : `<button class="frow" data-path="${esc(f.path)}"${diff ? " data-diff" : ""}>${row}</button>`;
}

function projectCard(p) {
  // The rows scroll inside the card, below the commit line.
  const rows = `<div class="files">${p.files.map(f => fileRow(f, p.kind === "changes" && f.status !== "?")).join("")}`
    + (p.more ? `<div class="frow"><span></span><span class="n">and ${p.more} more</span></div>` : "") + "</div>";
  if (p.kind === "changes") return `<div class="gcard"><h3>Changes on <span class="br">${esc(p.branch)}</span></h3>${rows}</div>`;
  return `<div class="gcard"><h3>Latest on <span class="br">${esc(p.branch)}</span><span class="sp"></span><span class="br">No changes</span></h3>`
    + `<div class="commit"><b>${esc(p.subject)}</b><div class="n">${esc(p.hash)} · ${esc(p.author)} · ${age(p.ts)}</div></div>${rows}</div>`;
}

function sessionRow(s) {
  const reviews = s.reviews ? ` · ${s.reviews} inline review${s.reviews === 1 ? "" : "s"}` : "";
  const files = s.files.map(f => `<button data-path="${esc(f)}">${esc(f)}</button>`).join("");
  return `<div class="srec"><span class="t">${s.label ? esc(s.label) : "Untitled session"}</span><span class="n">${age(s.ts)}${reviews}</span>`
    + (files ? `<span class="fl">${files}</span>` : "") + "</div>";
}

// A file opens beside the shell. A README link opens its file at its heading, which `open_link`
// finds from Laura's folder, the README's folder.
glance.addEventListener("click", (e) => {
  const b = e.target.closest("[data-path]"), a = e.target.closest("[data-href]");
  if (b) return act({ act: "open_path", pane: 0, path: b.dataset.path, diff: "diff" in b.dataset });
  if (a) act({ act: "open_link", pane: 0, href: a.dataset.href });
});

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

// The user's focus move: Laura moves the focus, ends a wait on a move to the shell, and replies
// with a layout frame. Every focus move the user makes calls this.
export function focusPane(id) {
  if (layout.focus !== id) act({ act: "focus", pane: id });
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
