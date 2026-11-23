// A browser pane: the screen frames from Laura's browser, the live badge, the agent's marks, the
// page's outlines and pins, and the page's dialog.

import { $, act, layout, send, ui } from "./state.js";
import { followFocus } from "./panes.js";
import { paintHint } from "./bottom.js";
import { authors, paintThreads, units, who } from "./review.js";
import { pinClick, place } from "./pages.js";

// The spinner in a browser pane's header: while the page loads, handles a click, or waits on a
// request the click started.
function paintSpin(el) {
  el.querySelector(".head .spin").hidden = !el.busy;
}

// A browser pane's frame: `[paint time, 8 bytes BE f64 ms][PNG]`. The ack goes at once, so Laura's
// browser paints the next frame while this one decodes. Only the newest frame waiting decodes.
export function onFrame(id, data) {
  send({ ack: id });
  const el = $("p" + id);
  if (!el || data.byteLength < 16) return;
  el.next = { painted: new DataView(data, 8, 8).getFloat64(0), png: new Uint8Array(data, 16) };
  if (!el.decoding) decodeFrames(el);
}

async function decodeFrames(el) {
  el.decoding = true;
  while (el.next) {
    const { painted, png } = el.next;
    el.next = null;
    try {
      const b = await createImageBitmap(new Blob([png], { type: "image/png" }));
      // A view's first frame can be the page's last, painted long ago: it says nothing of lag.
      if (el.bitmap && !el.first) el.lags.push([Date.now(), Date.now() - painted]);
      el.first = false;
      el.bitmap?.close();
      el.bitmap = b;
      drawFrame(el);
      paintLive(el);
    } catch {}
  }
  el.decoding = false;
}

// An emulated page shows at its own size, centered in the canvas and scaled down to
// fit with a 12 px margin: the transform from the page's CSS px to the canvas's. The frame, the
// pointer and the floating card all use it.
export function emuFit(el) {
  const e = el.emulated, c = el.querySelector(".view canvas");
  if (!e) return { x: 0, y: 0, s: 1 };
  const s = Math.min(1, (c.clientWidth - 24) / e.w, (c.clientHeight - 24) / e.h);
  return { x: (c.clientWidth - e.w * s) / 2, y: (c.clientHeight - e.h * s) / 2, s };
}

// The last frame, 1:1 from the top left, or an emulated page in its box with a shadow; a resize
// draws it again until the page's next frame.
// ponytail: Chrome sends an emulated page's frame at up to the pane's device pixels, a little more
// than the box draws; restart the screencast at the box's size if the softness shows.
export function drawFrame(el) {
  const c = el.querySelector(".view canvas"), g = c.getContext("2d");
  g.clearRect(0, 0, c.width, c.height);
  if (!el.bitmap) return;
  if (!el.emulated) return g.drawImage(el.bitmap, 0, 0);
  const { x, y, s } = emuFit(el), d = devicePixelRatio || 1;
  g.save();
  Object.assign(g, { shadowColor: "rgb(16 24 40 / .14)", shadowBlur: 14 * d, shadowOffsetY: 4 * d });
  g.drawImage(el.bitmap, x * d, y * d, el.emulated.w * s * d, el.emulated.h * s * d);
  g.restore();
}

// The live badge: "live", or the worst lag of the last half second, in amber for a second, when it
// passes 100 ms. Its tooltip counts the frames of the last second.
function paintLive(el) {
  const now = Date.now(), live = el.querySelector(".head .live");
  el.lags = el.lags.filter(([t]) => now - t < 1000);
  const lag = Math.max(0, ...el.lags.filter(([t]) => now - t < 500).map(([, l]) => l));
  if (lag > 100) [el.lagUntil, el.lagShown] = [now + 1000, lag];
  const slow = now < (el.lagUntil ?? 0);
  // A new text fits the header again, so one lag gives way to the next at most 4 times a second;
  // "live" and a lag swap at once. `liveTimer` paints the last lag.
  const text = slow ? `${Math.round(el.lagShown)} ms` : "live";
  const swap = slow !== live.classList.contains("lag");
  if (live.textContent !== text && (swap || now - (el.liveAt ?? 0) >= 250)) {
    live.textContent = text;
    el.liveAt = now;
  }
  live.classList.toggle("lag", slow);
  const t = `${el.lags.length} frames a second · lag ${Math.round(lag)} ms`;
  live.dataset.tip = t;
  live.setAttribute("aria-label", t);
  // A still page sends no frames: the badge settles without them.
  clearTimeout(el.liveTimer);
  if (el.lags.length || slow) el.liveTimer = setTimeout(() => paintLive(el), 500);
}

// A browser pane's page: the badge shows while Laura's browser runs the page; the pointer, the
// spinner, back and forward, and the page's dialog follow the page.
export function onBrowser(m) {
  const el = $("p" + m.id);
  if (!el) return;
  // A file pane that turned into a browser pane keeps no rows.
  for (const u of units(el.querySelector(".rows"))) u.remove();
  // A new page has no focused field.
  if (el.url !== m.url) el.typing = false;
  el.url = m.url;
  el.live = m.live;
  el.busy = !!m.busy;
  el.querySelector(".head .live").hidden = !m.live;
  // The agent's tool records the page until the recording stops or the tool disconnects.
  el.recording = !!m.recording;
  el.querySelector(".head .rec").hidden = !el.recording;
  el.querySelector(".view canvas").style.cursor = m.cursor ?? "";
  el.querySelector(".head [data-act=back]").disabled = !m.back;
  el.querySelector(".head [data-act=fwd]").disabled = !m.forward;
  paintDialog(el, m.id, m.dialog ?? null);
  // The agent's tools emulate a device: the page shows at its size, which the pane header names.
  const emu = m.emulated ?? null;
  if (JSON.stringify(emu) !== JSON.stringify(el.emulated ?? null)) {
    el.emulated = emu;
    el.querySelector(".view").classList.toggle("emu", !!emu);
    const chip = el.querySelector(".head .emu");
    chip.hidden = !emu;
    if (emu) chip.textContent = `${emu.w}×${emu.h}`;
    drawFrame(el);
    place(m.id);
    paintMarks(m.id);
  }
  paintSpin(el);
  paintHint();
  // The cursor's thread floats below its element, as on an HTML page (`onPage`).
  const folded = ts => ts?.find(t => t.line === m.line)?.collapsed;
  if (el.line !== m.line || folded(el.threads) !== folded(m.threads)) el.shut = null;
  el.threads = m.threads;
  el.card = m.card;
  el.line = m.line;
  paintThreads(m.id);
  // A pin shows its thread's authors and count.
  paintMarks(m.id);
}

// The agent's click and typing in a browser pane: a ring at the click, an outline on the field. The
// web app draws the marks over the canvas, not in the page, so the agent's screenshots don't show them.
export function onAgent(m) {
  const el = $("p" + m.id), view = el?.querySelector(".view");
  if (!view) return;
  // The pane header's Agent chip stays 5 s after the agent's last click or typing.
  const chip = () => el.querySelector(".head .agent");
  el.acting = true;
  chip().hidden = false;
  clearTimeout(el.actTimer);
  el.actTimer = setTimeout(() => (el.acting = false, chip().hidden = true), 5000);
  const t = emuFit(el), at = (d, x, y) => Object.assign(d.style, { left: t.x + x * t.s + "px", top: t.y + y * t.s + "px" });
  if (m.click) {
    const d = document.createElement("div");
    d.className = "amark ring";
    at(d, ...m.click);
    d.addEventListener("animationend", () => d.remove());
    view.append(d);
  }
  if (m.type) {
    let d = view.querySelector(".amark.field");
    if (!d) (d = document.createElement("div")), (d.className = "amark field"), view.append(d);
    const [x, y, w, h] = m.type;
    at(d, x, y);
    Object.assign(d.style, { width: w * t.s + "px", height: h * t.s + "px" });
    d.classList.add("on");
    clearTimeout(d.timer);
    d.timer = setTimeout(() => d.classList.remove("on"), 1000);
  }
}

// The agent's screenshot of a browser pane's page: one flash over the page, and **Screenshot** in
// the pane header for 4 s, as **Reloaded**.
export function onShot(m) {
  const el = $("p" + m.id), view = el?.querySelector(".view");
  if (!view) return;
  const chip = () => el.querySelector(".head .shot");
  el.shot = true;
  chip().hidden = false;
  clearTimeout(el.shotTimer);
  el.shotTimer = setTimeout(() => (el.shot = false, chip().hidden = true), 4000);
  // A reflow between, so a second screenshot flashes again.
  view.classList.remove("shot");
  void view.offsetWidth;
  view.classList.add("shot");
}

// A pin's authors: each one's initial, and whether it's an agent.
export const pinWho = t => [...new Set(authors(t))].map(a => [([...who(a)][0] ?? "?").toUpperCase(), a !== null]);

// A browser pane's outlines, tags and pins, drawn over the canvas as the agent's marks are: the page
// holds none of them, so the agent's screenshots and snapshots show the page as the user's browser
// would. browser.js sends the boxes in the frame's px.
export function paintMarks(id) {
  const el = $("p" + id), view = el?.querySelector(".view");
  if (!view) return;
  let layer = view.querySelector(".marks");
  if (!layer) (layer = document.createElement("div")), (layer.className = "marks"), view.append(layer);
  layer.replaceChildren();
  el.pinAt = null;
  const m = el.marks;
  if (!m) return;
  const t = emuFit(el), x = n => t.x + n * t.s, y = n => t.y + n * t.s;
  const add = (cls, style, text) => {
    const d = document.createElement("div");
    d.className = cls;
    Object.assign(d.style, style);
    if (text) d.textContent = text;
    layer.append(d);
    return d;
  };
  const box = (cls, [top, l, b, r], tag) => {
    add("mk " + cls, { left: x(l) + "px", top: y(top) + "px", width: (r - l) * t.s + "px", height: (b - top) * t.s + "px" });
    if (tag) add("mk-tag", { left: x(l) - 4 + "px", top: Math.max(0, y(top) - 20) + "px" }, tag);
  };
  for (const [, ...b] of m.th) box("th", b);
  if (m.hl) box("hl", m.hl);
  if (m.cur) box("cur", ...m.cur);
  if (m.hov) box("hov", ...m.hov);
  // Each thread's pin at its element's top-right corner. A click opens its thread.
  for (const [line, top, , , r] of m.th) {
    const th = el.threads?.find(t => t.line === line);
    if (!th) continue;
    const pin = add("mk-pin", { left: Math.max(0, Math.min(x(r) - 8, view.clientWidth - 60)) + "px", top: Math.max(0, y(top) - 12) + "px" });
    for (const [initial, agent] of pinWho(th)) {
      pin.append(Object.assign(document.createElement("span"), { className: agent ? "av agent" : "av", textContent: initial }));
    }
    pin.append(String(th.notes.length));
    pin.addEventListener("pointerenter", () => (el.pinAt = line));
    pin.addEventListener("pointerleave", () => (el.pinAt = null));
    pin.addEventListener("click", () => (ui.draft ? followFocus(true) : pinClick(id, line)));
  }
}

// The page's dialog, which the screencast doesn't draw: over the canvas, as in a browser tab.
function paintDialog(el, id, d) {
  if (JSON.stringify(d) === JSON.stringify(el.dialog ?? null)) return;
  el.dialog = d;
  el.querySelector(".dlgwrap")?.remove();
  if (!d) return followFocus(true);
  // Chrome gives a leave-page dialog no message.
  const leave = d.type === "beforeunload";
  const w = document.createElement("div");
  w.className = "dlgwrap";
  w.innerHTML = '<div class="pop dlg"><div class="ph"></div><p></p>'
    + (d.type === "prompt" ? '<input spellcheck="false">' : "") + '<div class="fa">'
    + (d.type === "alert" ? "" : `<button class="btn" data-ok="">${leave ? "Stay" : "Cancel"} <kbd>Esc</kbd></button>`)
    + `<button class="btn pri" data-ok="1">${leave ? "Leave" : "OK"} <kbd>Enter</kbd></button></div></div>`;
  let host = "";
  try { host = new URL(el.url).host; } catch {}
  w.querySelector(".ph").textContent = `${host} says`;
  w.querySelector("p").textContent = leave ? "Leave this page? Changes you made may not be saved." : d.message;
  const field = w.querySelector("input");
  if (field) field.value = d.default ?? "";
  w.addEventListener("mousedown", e => e.target.closest("button") && e.preventDefault());
  w.addEventListener("click", e => {
    const b = e.target.closest("button");
    if (b) answer(id, !!b.dataset.ok);
  });
  el.querySelector(".view").append(w);
  if (field && layout?.focus === id) field.focus(), field.select();
}

export function answer(id, accept) {
  const text = $("p" + id)?.querySelector(".dlg input")?.value ?? "";
  act({ act: "browser_dialog", pane: id, accept, text });
}
