// A file pane's rows and pages: the rows from Laura, an HTML file's page in its frame, the
// highlight's scroll, comment mode, the place of the cursor's thread on a page, and a page's pins.

import { $, act, layout, pane, root, send, ui } from "./state.js";
import { tooSmall } from "./head.js";
import { focusPane, followFocus, placePlus } from "./panes.js";
import { emuFit, pinWho } from "./browser-pane.js";
import { paintHint, reloaded } from "./bottom.js";
import { begin, goThread, keepDraft, paintThreads, reveal, units } from "./review.js";

// A row's or block's text, without its gutter.
function unitText(u) {
  const c = u.cloneNode(true);
  c.querySelectorAll(".g").forEach(g => g.remove());
  return c.textContent;
}

// The cursor's open thread, row or block, or the line inside the block that the cursor is on.
function cursorEl(rows) {
  const unit = units(rows)[rows.dataset.cursor];
  return rows.querySelector(".card.on:not(.draft)") ?? unit?.querySelector(".md .cur") ?? unit;
}

export function onPane(m) {
  const el = $("p" + m.id), rows = el?.querySelector(".rows");
  if (!rows) return;
  // A page's `pane` frame is its diff view, which has no page to comment on.
  if (ui.commentMode === m.id) setCommentMode(null);
  el.querySelector(":scope > iframe")?.remove();
  // ponytail: compares text, so a reload that only changes a color doesn't flash
  let before = m.reloaded && m.t === "pane" && new Set([...units(rows)].map(unitText));
  el.threads = m.threads;
  el.card = m.card;
  keepDraft(() => {
    if (m.t === "rows") {
      // Only the changed rows, by index: a cursor move swaps two rows instead of morphing them all,
      // and a tail's new line appends one row. The new rows past the old end are in order.
      const us = units(rows);
      if (m.reloaded) before = new Set(m.rows.filter(([i]) => us[i]).map(([i]) => unitText(us[i])));
      for (let i = m.len; i < us.length; i++) us[i].remove();
      for (const [i, html] of m.rows) {
        if (us[i] && i < m.len) us[i].outerHTML = html;
        else rows.insertAdjacentHTML("beforeend", html);
      }
    } else {
      // Idiomorph could morph a thread into a row; `paintThreads` puts the threads back.
      for (const inl of rows.querySelectorAll(":scope > .inl")) inl.remove();
      Idiomorph.morph(rows, m.html, { morphStyle: "innerHTML" });
    }
    paintThreads(m.id);
  });
  if (before) {
    // Not by index: a line inserted mid-file moves every row below it, and a moved row doesn't flash.
    const us = units(rows);
    for (const u of m.t === "rows" ? m.rows.map(([i]) => us[i]).filter(Boolean) : us) {
      if (before.has(unitText(u))) continue;
      u.classList.remove("flash");
      void u.offsetWidth; // restarts the animation
      u.classList.add("flash");
    }
    reloaded(m.id);
  }
  rows.dataset.cursor = String(m.cursor);
  // A step inside a block, or onto its thread, keeps the block's index, so the key adds the line.
  const cur = cursorEl(rows), key = `${m.cursor}:${cur?.dataset.line ?? "L" + cur?.dataset.th}`;
  if (m.recenter) toHighlight(m.id, m.recenter === "line");
  else if (key !== rows.dataset.key) {
    reveal(cur, "nearest");
    // The cursor's row stays in view above its tall thread.
    const u = units(rows)[rows.dataset.cursor];
    if (u?.classList.contains("row")) reveal(u, "nearest");
  }
  rows.dataset.key = key;
  goThread(m.id);
  placePlus(m.id);
}

// A page's frame: an HTML file pane's iframe, or a browser pane's view.
export const frameOf = id => $("p" + id)?.querySelector(pane(id)?.kind === "browser" ? ":scope > .view" : ":scope > iframe");

// Scrolls the pane to its highlight; a page's frame scrolls itself. A `jump` (a frame's
// `recenter: "line"`, from `Panel::jump_to_line`) centers the cursor's line, also the line inside
// a markdown block, only when that line is off screen. A page gets the jump in `page`'s `jump`,
// and `again` when its pane grows after a jump.
export function toHighlight(id, jump) {
  // Page input gets no `acted` reply (`web.rs`), so `send`, not `act`.
  if (pane(id)?.kind === "browser") return send({ act: "browser_reveal", pane: id });
  const el = $("p" + id);
  // A too-small pane hides its rows and page, which can't scroll; the `.narrow` observer scrolls again.
  if (!el || (el.hlLater = tooSmall(el) && { jump })) return;
  const f = frameOf(id);
  if (f) return f.contentWindow?.postMessage({ ...f.state, [jump ? "again" : "scroll"]: true }, "*");
  const rows = el.querySelector(".rows");
  if (!rows) return;
  if (jump) {
    // A block's first line has no `.cur`; its element carries the block's `data-line`.
    const u = units(rows)[rows.dataset.cursor];
    const t = u?.querySelector(".md .cur") ?? u?.querySelector(`.md [data-line="${u.dataset.line}"]`) ?? u;
    if (!t) return;
    // The bounding box, since a `.cur`'s `offsetTop` can be relative to its block. An `<li>`'s box
    // holds its nested list, which can be taller than the pane, so measure the item's own text.
    const g = document.createRange(), sub = t.tagName === "LI" && t.querySelector(":scope > ul, :scope > ol");
    // A wrapped line is one unit per row, all with the line's `data-row`.
    let end = t;
    if (t === u) while (end.nextElementSibling?.matches(`[data-row="${u.dataset.row}"]`)) end = end.nextElementSibling;
    g.selectNodeContents(t);
    if (sub) g.setEndBefore(sub);
    else if (end !== t) g.setEndAfter(end);
    let r = g.getBoundingClientRect();
    if (!r.height) r = t.getBoundingClientRect(); // an item with no text before its list
    const b = rows.getBoundingClientRect();
    // A target taller than the pane shows its top, as the TUI and an HTML page do.
    if (r.top < b.top || r.bottom > b.bottom) rows.scrollTop += r.top - b.top - Math.max(0, (rows.clientHeight - r.height) / 2);
    return;
  }
  const h = rows.querySelector(".hl");
  // ponytail: row-proportional, exact for code, approximate for wrapped prose
  if (h) rows.scrollTop = h.offsetTop + h.offsetHeight * +(h.dataset.at ?? .5) - rows.clientHeight / 2;
}

// An HTML file's page in a sandboxed frame over the pane's body, and the cursor's thread over it.
export function onPage(m) {
  const el = $("p" + m.id), rows = el?.querySelector(".rows");
  if (!rows) return;
  let f = frameOf(m.id);
  if (!f) {
    f = document.createElement("iframe");
    f.setAttribute("sandbox", "allow-scripts allow-forms");
    // The first post can come before the page's script listens.
    f.addEventListener("load", () => f.contentWindow?.postMessage(f.state, "*"));
    el.insertBefore(f, rows);
  }
  // Setting `src` reloads the page, so only when the page changed.
  if (f.getAttribute("src") !== m.page) {
    // The URL's query changes only with the content; its path changes with `open --panel`.
    if (f.getAttribute("src")?.split("?")[0] === m.page.split("?")[0]) reloaded(m.id);
    f.src = m.page;
  }
  // Target "*": the frame's origin is opaque, and line numbers aren't secret.
  const threads = m.threads.map(t => ({ line: t.line, n: t.notes.length, who: pinWho(t) }));
  // A new frame loads after later frames came, so the jump stays until the cursor moves.
  const jump = m.jump || (!!f.state?.jump && f.state.cursor === m.cursor);
  f.state = { cursor: m.cursor, line: m.line, threads, highlight: m.highlight, comment: ui.commentMode === m.id, jump };
  f.contentWindow?.postMessage(f.state, "*");
  // A too-small pane hides its page, which can't scroll; the `.narrow` observer scrolls again.
  if (m.jump && tooSmall(el)) el.hlLater = { jump: true };
  // The diff view's rows, if the page was in it.
  for (const u of units(rows)) u.remove();
  rows.dataset.cursor = "";
  // A cursor move, or `r` on the cursor's thread, shows the card comment mode hid.
  const folded = ts => ts?.find(t => t.line === m.line)?.collapsed;
  if (el.line !== m.line || folded(el.threads) !== folded(m.threads)) el.shut = null;
  el.threads = m.threads;
  el.card = m.card;
  el.line = m.line;
  paintThreads(m.id);
}

// Turns comment mode on for page pane `id`, or off with `null`. Leaving it hides the cursor's
// thread until the cursor moves, so the user can click what the card covered.
export function setCommentMode(id) {
  const old = ui.commentMode;
  ui.commentMode = id;
  for (const i of [old, id]) {
    if (i === null) continue;
    const el = $("p" + i);
    el?.classList.toggle("comment-mode", i === ui.commentMode);
    if (el) {
      el.shut = i === ui.commentMode ? null : el.line;
      paintThreads(i);
    }
    if (pane(i)?.kind === "browser") send({ act: "browser_comment_mode", pane: i, on: i === ui.commentMode });
    const f = frameOf(i);
    if (!f?.state) continue;
    f.state.comment = i === ui.commentMode;
    f.contentWindow?.postMessage(f.state, "*");
  }
  paintHint();
}

// A page's cursor thread or comment box floats below the cursor's element (`f.rect`, from the
// page), inside the frame.
export function place(id) {
  const el = $("p" + id), f = frameOf(id), rows = el?.querySelector(".rows");
  if (!f || !rows) return;
  const [, l, b] = f.rect ?? [0, 0, 0], t = emuFit(el);
  const [left, bottom] = [t.x + l * t.s, t.y + b * t.s];
  const x = Math.max(8, Math.min(left, f.clientWidth - rows.offsetWidth - 8));
  const y = Math.max(8, Math.min(bottom + 10, f.clientHeight - rows.offsetHeight - 8));
  rows.style.setProperty("--fx", f.offsetLeft + x + "px");
  rows.style.setProperty("--fy", f.offsetTop + y + "px");
}

// A page's click, link, Esc or cursor element's rect. Only from a frame Laura made; the page's own
// script can post these too, which only does what a click on the page could.
addEventListener("message", e => {
  const f = [...root.querySelectorAll(".pane iframe")].find(f => f.contentWindow === e.source);
  const m = e.data;
  if (!f || !layout || typeof m !== "object" || !m) return;
  const id = +f.parentElement.id.slice(1);
  if (pane(id)?.kind !== "file") return;
  // The comment box follows its element while the user types.
  if (m.laura === "rect") {
    const r = [m.top, m.left, m.bottom, m.right];
    f.rect = r.every(Number.isFinite) ? r : null;
    return place(id);
  }
  if (ui.draft) return followFocus(true);
  if (m.laura === "line") {
    if (m.pin === true) return pinClick(id, m.line);
    focusPane(id);
    if (Number.isInteger(m.line)) act({ act: "set_cursor", pane: id, line: m.line });
    // A text field or a selection keeps the keystrokes; anything else gives them back to Laura.
    if (!m.editable) root.focus({ preventScroll: true });
    paintHint();
    // Only in comment mode, so a page's script can't open a comment box.
    if (m.pick === true && ui.commentMode === id && Number.isInteger(m.line)) begin(id, "comment");
  } else if (m.laura === "esc") {
    // The field takes `Esc` first: comment mode stays on until the next `Esc`.
    root.focus({ preventScroll: true });
    paintHint();
  }
  else if (m.laura === "link" && typeof m.href === "string") {
    // The workspace's policy lets a frame load only the workspace's own URLs.
    if (/^https?:/i.test(m.href)) open(m.href, "_blank", "noopener");
    else act({ act: "open_link", pane: id, href: m.href });
  }
});

// A click on a page's pin: on the cursor's thread, it shows or hides the thread's card; on another
// thread, it moves the cursor there. A collapsed thread expands.
export function pinClick(id, line) {
  const el = $("p" + id);
  if (!el || !Number.isInteger(line)) return;
  focusPane(id);
  root.focus({ preventScroll: true });
  if (el.threads?.find(t => t.line === line)?.collapsed) return act({ act: "toggle_thread", pane: id, line });
  if (line === el.line) {
    el.shut = el.shut === line ? null : line;
    return paintThreads(id);
  }
  act({ act: "set_cursor", pane: id, line });
}
