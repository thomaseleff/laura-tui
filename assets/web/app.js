// The web app (#88). One websocket: JSON text frames for layout, panes and replies; binary frames
// `[pane id, 8 bytes BE][bytes]` for each terminal. Every string from a frame goes in as text; only
// `pane`/`rows` HTML and `strings.help`, which Laura escapes, go in as markup.
const css = getComputedStyle(document.documentElement);
const v = name => css.getPropertyValue(name).trim();
const $ = id => document.getElementById(id);
const root = $("ws"), hintEl = $("hint"), noticeEl = $("notice"), draftEl = $("draft");
const draftText = draftEl.querySelector("textarea");
const enc = new TextEncoder();
const ANSI = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
const THEME = { background: v("--bg"), foreground: v("--fg"), cursor: v("--fg"), selectionBackground: v("--sel") };
ANSI.forEach((c, i) => {
  THEME[c] = v(`--ansi-${i}`);
  THEME["bright" + c[0].toUpperCase() + c.slice(1)] = v(`--ansi-${i + 8}`);
});

let ws, layout = null, strings = {}, cw = 0, ch = 0;
let focusKey = "", locked = false, help = false, popup = null, confirmQuit = false, ended = false;
let notice = "", noticeTimer = 0, frozenText = "";
// The open comment or review body: `{pane, line, kind, hint, busy}`.
let draft = null;
const terms = new Map(); // pane id → xterm, one per PTY pane
const acts = []; // callbacks for `acted` replies, in send order

const send = m => ws?.readyState === 1 && ws.send(JSON.stringify(m));
const frame = (id, bytes) => {
  const out = new Uint8Array(8 + bytes.length);
  new DataView(out.buffer).setBigUint64(0, BigInt(id));
  out.set(bytes, 8);
  if (ws?.readyState === 1) ws.send(out);
};
function act(m, then) {
  if (ws?.readyState !== 1) return;
  acts.push(then);
  send(m);
}
const pane = id => layout?.panes.find(p => p.id === id);
const focused = () => pane(layout?.focus);
const overlayOpen = () => help || popup !== null || confirmQuit || !$("overlay").hidden;

function showNotice(text, secs) {
  notice = text;
  clearTimeout(noticeTimer);
  noticeTimer = setTimeout(() => ((notice = ""), paintNotice()), secs * 1000);
  paintNotice();
}
// A transient notice wins; the frozen pane's notice shows otherwise.
const paintNotice = () => (noticeEl.textContent = notice || frozenText);

function paintHint() {
  const h = layout?.hints;
  if (!h) return;
  hintEl.textContent = locked ? h.locked : confirmQuit ? h.quit : draft ? draft.hint
    : popup !== null ? h.panes : h.pane;
}

// One cell: xterm's own once it has drawn (it can't be given one), else the font's advance.
function measure() {
  const s = document.createElement("span");
  s.className = "probe";
  s.textContent = "W".repeat(200);
  document.body.append(s);
  const r = s.getBoundingClientRect();
  s.remove();
  [cw, ch] = [r.width / 200, r.height];
  // The font's advance; file-pane text is letter-spaced from it onto the grid's cell.
  document.documentElement.style.setProperty("--pw", cw + "px");
  const t = terms.get(0);
  const screen = t?.element?.querySelector(".xterm-screen");
  if (screen?.offsetWidth && t.cols) [cw, ch] = [screen.offsetWidth / t.cols, screen.offsetHeight / t.rows];
  document.documentElement.style.setProperty("--cw", cw + "px");
  document.documentElement.style.setProperty("--ch", ch + "px");
}

// Cells kept clear around the grid, so the hint line and borders don't touch the window's edges.
const MARGIN = 0.5;
function report() {
  measure();
  const cols = Math.floor(innerWidth / cw - 2 * MARGIN), rows = Math.floor(innerHeight / ch - 2 * MARGIN);
  document.documentElement.style.setProperty("--cols", cols);
  document.documentElement.style.setProperty("--rows", rows);
  // The last row is the hint line.
  send({ view: { cols, rows: rows - 1 } });
}

// Laura's keys inside a terminal: false leaves the key to app.js, unsent.
function termKeys(id, term, e) {
  if (overlayOpen()) return false;
  if (e.key === "F12") return false;
  if (locked) return true;
  if (draft) return false;
  if (e.ctrlKey && !e.altKey && "pPhHqQ".includes(e.key) && e.key.length === 1) return false;
  if (e.ctrlKey && !e.altKey && (e.key === "l" || e.key === "L") && id !== 0) return false;
  if (id === 0 && (e.key === "PageUp" || e.key === "PageDown") && term.buffer.active.type === "normal") {
    // xterm.js asks on keydown, keypress and keyup: scroll once.
    if (e.type === "keydown") term.scrollPages(e.key === "PageUp" ? -1 : 1);
    return false;
  }
  return true;
}

function makeTerm(id, el) {
  const term = new Terminal({
    fontFamily: v("--font"), fontSize: 14, lineHeight: 1, cursorBlink: true, allowProposedApi: true,
    scrollback: 10000, // src/pty.rs SCROLLBACK, so the replay keeps the shell's whole history
    theme: THEME,
  });
  // Emoji are two cells wide in Unicode 11+, as the child and vt100 count them.
  term.loadAddon(new Unicode11Addon.Unicode11Addon());
  term.unicode.activeVersion = "11";
  // The child sees the same terminal as in the TUI: no replies to its queries.
  const p = term.parser, done = () => true;
  for (const q of [{ final: "c" }, { prefix: ">", final: "c" }, { final: "n" }, { prefix: "?", final: "n" },
    { intermediates: "$", final: "p" }, { prefix: "?", intermediates: "$", final: "p" }]) p.registerCsiHandler(q, done);
  p.registerDcsHandler({ intermediates: "$", final: "q" }, done);
  for (const n of [4, 10, 11, 12]) p.registerOscHandler(n, done);
  term.onData(d => {
    // Focus reports: the TUI never sends them.
    if (d === "\x1b[I" || d === "\x1b[O") return;
    frame(id, enc.encode(d));
  });
  term.onBinary(d => frame(id, Uint8Array.from(d, c => c.charCodeAt(0))));
  term.attachCustomKeyEventHandler(e => termKeys(id, term, e));
  term.open(el.querySelector(".term"));
  // The DOM renderer draws block characters from the font, with gaps; WebGL fills the cell.
  // Without WebGL2 (a VM, remote desktop), the addon throws and the DOM renderer stays.
  try {
    const gl = new WebglAddon.WebglAddon();
    gl.onContextLoss(() => gl.dispose());
    term.loadAddon(gl);
  } catch {}
  if (id === 0) {
    const once = term.onRender(() => (once.dispose(), report()));
  }
  // Laura brackets a paste, as the TUI does; xterm.js mustn't send it too.
  el.addEventListener("paste", e => {
    e.preventDefault();
    e.stopPropagation();
    act({ act: "paste", pane: id, text: e.clipboardData.getData("text") });
  }, true);
  terms.set(id, term);
}

function makePane(p) {
  const el = document.createElement("div");
  el.id = "p" + p.id;
  el.className = "pane";
  el.innerHTML = '<div class="title"><span></span><span class="warn"></span></div><div class="term"></div><div class="rows"></div>'
    + '<button class="cbtn" title="Comment">+</button><span class="acts"><button>Submit</button><button>Refresh</button></span>';
  const [cbtn, submit, refresh] = el.querySelectorAll("button");
  // A clicked button keeps no focus, so a later Enter can't press it again.
  for (const b of [cbtn, submit, refresh]) b.addEventListener("mousedown", e => e.preventDefault());
  cbtn.addEventListener("click", () => begin(p.id, "comment"));
  submit.addEventListener("click", () => begin(p.id, "review"));
  refresh.addEventListener("click", () => act({ act: "refresh", pane: p.id }));
  el.querySelector(".rows").addEventListener("scroll", () => placeBtn(p.id));
  el.addEventListener("mousedown", () => {
    const k = pane(p.id)?.kind;
    if (!draft && k && k !== "file" && layout.focus !== p.id) act({ act: "focus", pane: p.id });
  });
  el.addEventListener("click", e => {
    if (!getSelection().isCollapsed) return;
    // A draft keeps its pane and line; a click only gives the draft its focus back.
    if (draft) return followFocus(true);
    if (pane(p.id)?.kind !== "file") return;
    if (layout.focus !== p.id) act({ act: "focus", pane: p.id });
    const row = e.target.closest("[data-line]");
    if (row) act({ act: "set_cursor", pane: p.id, line: +row.dataset.line });
  });
  return el;
}

function onLayout(m) {
  layout = m;
  const seen = new Set();
  for (const p of m.panes) {
    const el = $("p" + p.id) ?? root.appendChild(makePane(p));
    seen.add(el.id);
    if (p.pty && !terms.has(p.id)) makeTerm(p.id, el);
    Object.assign(el.style, {
      left: `calc(var(--cw) * ${p.x})`, top: `calc(var(--ch) * ${p.y})`,
      width: `calc(var(--cw) * ${p.w})`, height: `calc(var(--ch) * ${p.h})`,
    });
    el.style.setProperty("--w", p.w);
    el.className = `pane ${p.kind}${p.id === m.focus ? " focused" : ""}`;
    const [title, warn] = el.querySelector(".title").children;
    title.textContent = ` ${p.title} `;
    warn.textContent = p.frozen ? "⚠ " : "";
    el.querySelector(".acts button").hidden = !p.threads;
    const t = terms.get(p.id);
    if (t && (t.cols !== p.w - 2 || t.rows !== p.h - 2)) t.resize(Math.max(1, p.w - 2), Math.max(1, p.h - 2));
    placeBtn(p.id);
  }
  for (const el of [...root.children]) {
    if (seen.has(el.id)) continue;
    const id = +el.id.slice(1);
    terms.get(id)?.dispose();
    terms.delete(id);
    el.remove();
  }
  paintDraft();
  followFocus();
  paintHint();
}

// The draft box sits over the bottom of the shell, as in the TUI, outside every pane.
function paintDraft() {
  draftEl.hidden = !draft;
  root.classList.toggle("drafting", !!draft);
  const p = pane(0);
  if (!draft || !p) return;
  draftEl.querySelector(".title").textContent = ` ${strings.draft[draft.kind]} `;
  Object.assign(draftEl.style, {
    left: `calc(var(--cw) * ${p.x})`, width: `calc(var(--cw) * ${p.w})`,
    bottom: `calc(100% - var(--ch) * ${p.y + p.h})`, maxHeight: `calc(var(--ch) * ${p.h})`,
  });
}

function begin(id, kind) {
  act({ act: kind === "comment" ? "begin_comment" : "begin_review", pane: id }, r => {
    if (!r.ok) return;
    draft = { pane: id, line: r.line, kind, hint: r.hint, busy: false };
    draftText.value = r.text ?? "";
    paintDraft();
    paintHint();
    followFocus(true);
  });
}

// `Enter`: add or save the comment, or submit the inline review. A failure keeps the draft for a retry.
function confirmDraft() {
  if (draft.busy) return;
  draft.busy = true;
  const { pane: p, line, kind } = draft, text = draftText.value;
  act(kind === "comment" ? { act: "comment", pane: p, line, text } : { act: "submit", pane: p, body: text },
    r => (r.ok ? closeDraft() : draft && (draft.busy = false)));
}

function closeDraft() {
  if (!draft) return;
  draft = null;
  send({ hold: false });
  if (notice === strings.draft.failed) (notice = ""), paintNotice();
  paintDraft();
  paintHint();
  followFocus(true);
}

// The cursor's row or block, or the line inside the block that the cursor is on.
function cursorEl(rows) {
  const unit = rows.children[rows.dataset.cursor];
  return unit?.querySelector(".md .cur") ?? unit;
}

// The `+` button rides the focused file pane's cursor row, hidden while that row is out of view.
function placeBtn(id) {
  const el = $("p" + id), rows = el?.querySelector(".rows"), btn = el?.querySelector(".cbtn");
  if (!btn) return;
  const row = cursorEl(rows);
  // Not `offsetTop`: a table row's offset parent is its table.
  const top = row ? row.getBoundingClientRect().top - rows.getBoundingClientRect().top : -1;
  btn.style.visibility = top >= 0 && top + ch <= rows.clientHeight ? "" : "hidden";
  btn.style.top = `calc(var(--ch) + ${top}px)`;
}

// DOM focus follows the workspace's focus, only when it changes, so a title or resize frame doesn't
// take focus from anything else.
function followFocus(force) {
  const f = focused();
  const key = `${layout?.focus}:${f?.kind}:${locked}:${!!draft}`;
  if (!f || (key === focusKey && !force)) return;
  focusKey = key;
  if (draft && !locked) return draftText.focus();
  const t = f.kind !== "file" ? terms.get(f.id) : locked ? terms.get(0) : null;
  if (t) return t.focus();
  for (const t of terms.values()) t.blur();
  root.focus();
}

function onPane(m) {
  const rows = document.querySelector(`#p${m.id} .rows`);
  if (!rows) return;
  if (m.t === "rows") {
    // Only the changed rows, by index: a cursor move swaps two rows instead of morphing them all.
    for (const [i, html] of m.rows) if (rows.children[i]) rows.children[i].outerHTML = html;
  } else {
    Idiomorph.morph(rows, m.html, { morphStyle: "innerHTML" });
  }
  rows.dataset.cursor = String(m.cursor);
  // A step inside a block keeps the block's index, so the key adds the line.
  const cur = cursorEl(rows), key = `${m.cursor}:${cur?.dataset.line}`;
  if (m.recenter) {
    // ponytail: row-proportional, exact for code, approximate for wrapped prose
    const h = rows.querySelector(".hl");
    if (h) rows.scrollTop = h.offsetTop + h.offsetHeight * +(h.dataset.at ?? .5) - rows.clientHeight / 2;
  } else if (key !== rows.dataset.key) cur?.scrollIntoView({ block: "nearest" });
  rows.dataset.key = key;
  placeBtn(m.id);
}

function paintPopup() {
  const box = $("popup");
  box.hidden = popup === null;
  if (popup === null) return;
  box.replaceChildren(...layout.panes.map(p => {
    const d = document.createElement("div");
    const k = document.createElement("span");
    k.className = "k";
    k.textContent = `  #${p.id}`;
    d.append(k, `  ${p.title.replace(/^#\d+ /, "")}`);
    d.classList.toggle("on", popup !== "" && String(p.id) === popup);
    return d;
  }));
}

// Typed digits are the pane id. Commit as soon as no larger id could still be typed, else on Enter.
function popupKey(e) {
  const ids = layout.panes.map(p => String(p.id));
  const commit = buf => {
    if (ids.includes(buf)) act({ act: "focus", pane: +buf });
    popup = null;
  };
  if (/^[0-9]$/.test(e.key)) {
    popup += e.key;
    if (!ids.some(s => s.length > popup.length && s.startsWith(popup))) commit(popup);
  } else if (e.key === "Backspace") popup = popup.slice(0, -1);
  else if (e.key === "Enter") commit(popup);
  else popup = null;
  paintPopup();
}

const FILE_KEYS = {
  ArrowUp: p => ({ act: "move_cursor", pane: p, delta: -1 }),
  ArrowDown: p => ({ act: "move_cursor", pane: p, delta: 1 }),
  ArrowLeft: p => ({ act: "scroll_h", pane: p, delta: -1 }),
  ArrowRight: p => ({ act: "scroll_h", pane: p, delta: 1 }),
  d: p => ({ act: "toggle_diff", pane: p }),
  r: p => ({ act: "toggle_thread", pane: p }),
  n: p => ({ act: "jump_thread", pane: p, next: true }),
  N: p => ({ act: "jump_thread", pane: p, next: false }),
  x: p => ({ act: "close", pane: p }),
  h: p => ({ act: "clear_highlight", pane: p }),
  Escape: () => ({ act: "focus", pane: 0 }),
};

addEventListener("keydown", e => {
  if (!layout || ended || !$("overlay").hidden || ["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;
  const ctrl = e.ctrlKey && !e.altKey;
  const k = e.key.length === 1 && ctrl ? e.key.toLowerCase() : e.key;
  const take = () => (e.preventDefault(), e.stopPropagation());
  if (e.key === "F12") {
    take();
    locked = !locked;
    followFocus(true);
    return paintHint();
  }
  if (locked) return;
  if (confirmQuit) {
    take();
    confirmQuit = false;
    if (e.key === "y") send({ quit: true });
    return paintHint();
  }
  if (help) {
    take();
    help = false;
    $("help").hidden = true;
    return;
  }
  if (popup !== null) {
    take();
    popupKey(e);
    return paintHint();
  }
  const f = focused();
  if (ctrl && k === "h") {
    take();
    help = true;
    return ($("help").hidden = false);
  }
  if (draft) {
    // Enter confirms; Shift+Enter falls through as a newline. Ctrl+R/L/P/Q and Tab would reach the browser.
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) return take(), confirmDraft();
    if (e.key === "Escape") return take(), closeDraft();
    if ((ctrl && "rlpq".includes(k)) || e.key === "Tab") return take();
    if (document.activeElement !== draftText) draftText.focus();
    return;
  }
  if (ctrl && k === "p") {
    take();
    popup = "";
    paintPopup();
    return paintHint();
  }
  if (ctrl && k === "q") {
    take();
    return act({ act: "quit" }, m => m.ok && ((confirmQuit = true), paintHint()));
  }
  if (ctrl && k === "l" && f && f.pty && f.id !== 0) {
    take();
    return act({ act: "toggle_editor_view", pane: f.id });
  }
  if (f?.kind !== "file") return;
  if (ctrl && k === "r") {
    take();
    return act({ act: "refresh", pane: f.id });
  }
  if (ctrl && k === "l") return take(); // the browser's address bar
  if (!ctrl && !e.altKey && !e.metaKey && (e.key === "c" || e.key === "S")) {
    take();
    return begin(f.id, e.key === "c" ? "comment" : "review");
  }
  const make = !ctrl && !e.altKey && !e.metaKey && FILE_KEYS[e.key];
  if (make) {
    take();
    act(make(f.id));
  }
});

// Selecting copies on release, as in the TUI. A file pane copies its text without the gutter,
// joining the wrapped rows of one line with a space, as `mouse::extract_panel` does.
addEventListener("mouseup", e => {
  const el = e.target.closest?.(".pane");
  if (!el) return;
  const id = +el.id.slice(1);
  setTimeout(() => {
    const t = terms.get(id);
    if (pane(id)?.kind !== "file") {
      if (t?.hasSelection()) navigator.clipboard?.writeText(t.getSelection());
      return;
    }
    const sel = getSelection();
    if (sel.isCollapsed || !sel.rangeCount) return;
    const range = sel.getRangeAt(0);
    const rows = el.querySelector(".rows");
    let out = "", prev = null;
    for (const row of rows.children) {
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
    if (out) navigator.clipboard?.writeText(out);
  });
});

addEventListener("resize", report);
// Ask before a stray Ctrl+W closes the tab.
addEventListener("beforeunload", e => ended || e.preventDefault());

function showOverlay(text, button) {
  $("overlay-text").textContent = text;
  $("continue").hidden = !button;
  $("overlay").hidden = false;
  document.activeElement?.blur();
}
$("continue").addEventListener("click", () => {
  $("overlay").hidden = true;
  connect();
});

function connect() {
  // The replay assumes a blank terminal; reset after anything still queued is parsed.
  for (const t of terms.values()) t.write("", () => t.reset());
  focusKey = "";
  frozenText = "";
  paintNotice();
  let laid = false, moved = false;
  const url = new URL("ws" + location.search, location);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  const sock = (ws = new WebSocket(url));
  sock.binaryType = "arraybuffer";
  sock.onopen = report;
  sock.onmessage = e => {
    if (typeof e.data !== "string") {
      const id = Number(new DataView(e.data).getBigUint64(0));
      return terms.get(id)?.write(new Uint8Array(e.data, 8));
    }
    const m = JSON.parse(e.data);
    switch (m.t) {
      case "strings":
        strings = m;
        $("help").innerHTML = m.help;
        break;
      case "layout":
        // A draft open when the socket dropped: hold again and refocus its pane, or close it.
        if (!laid && draft) {
          if (m.panes.some(p => p.id === draft.pane)) {
            send({ hold: true });
            act({ act: "focus", pane: draft.pane });
            m.focus = draft.pane;
            draft.busy = false;
          } else closeDraft();
        }
        laid = true;
        onLayout(m);
        break;
      case "pane":
      case "rows":
        onPane(m);
        break;
      case "acted": {
        const then = acts.shift();
        if (!m.ok) showNotice(m.text, 5);
        then?.(m);
        break;
      }
      case "notice":
        if (m.frozen) (frozenText = m.text), paintNotice();
        else showNotice(m.text, 10);
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
    if (moved) return showOverlay(strings.moved, true);
    if (ended) {
      document.body.classList.add("ended");
      return showOverlay(strings.ended, false);
    }
    // Laura drops a view that falls behind; a fresh connection gets a fresh replay.
    if (laid) return setTimeout(connect, 500);
    // Hard-coded: with no connection, the page never got a `strings` frame.
    showOverlay("This tab cannot connect to the workspace. Make sure that Laura did not stop and that you used the full URL from the terminal.", false);
  };
}

document.fonts.ready.then(() => {
  measure();
  connect();
});
