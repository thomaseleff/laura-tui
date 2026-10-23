// The web app (#88). One websocket: JSON text frames for layout, panes and replies; binary frames
// `[pane id, 8 bytes BE][bytes]` for each terminal. Every string from a frame goes in as text or
// through `esc`; only `pane`/`rows` HTML, which Laura escapes, goes in as markup.
const css = getComputedStyle(document.documentElement);
const v = name => css.getPropertyValue(name).trim();
const $ = id => document.getElementById(id);
const root = $("ws"), hintEl = $("hint"), noticeEl = $("notice");
const enc = new TextEncoder();
const ANSI = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
const THEME = { background: v("--term-bg"), foreground: v("--term-fg"), cursor: v("--term-fg"), selectionBackground: v("--term-sel") };
ANSI.forEach((c, i) => {
  THEME[c] = v(`--ansi-${i}`);
  THEME["bright" + c[0].toUpperCase() + c.slice(1)] = v(`--ansi-${i + 8}`);
});

let ws, layout = null, strings = {}, cw = 0, ch = 0;
let focusKey = "", locked = false, help = false, popup = null, confirmQuit = false, ended = false;
let notice = "", noticeTimer = 0;
// `popup` is the pane selector's selected row, or `null` while the pane selector is closed.
// Panes that reloaded or got a new highlight while another pane had the focus.
const unseen = new Set();
// The open comment or review body: `{pane, line, kind, hint, busy, failed, text}`. A comment's box
// is `draftCard`, in its line's thread or below its line; a review body's is in the pane's popover.
let draft = null, draftCard = null;
// The pane whose thread list is open, and the pane whose **More** is open.
let listOpen = null, moreOpen = null;
// The page's pane in comment mode, where a click on the page picks an element to comment on.
let commentMode = null;
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
// The notice keeps its text while it fades out.
function paintNotice() {
  noticeEl.classList.toggle("on", !!notice);
  if (notice) noticeEl.textContent = notice;
}

function paintHint() {
  const h = layout?.hints;
  if (!h) return;
  const pairs = locked ? h.locked : confirmQuit ? h.quit : draft ? draft.hint
    : popup !== null ? h.panes : commentMode !== null && commentMode === layout.focus ? h.comment_mode : h.pane;
  hintEl.replaceChildren(...(pairs ?? []).flatMap(([k, label], i) => {
    const s = document.createElement("span");
    s.textContent = label;
    const sep = document.createElement("i");
    sep.className = "sep";
    return [...(i ? [sep] : []), ...kbds(k), s];
  }));
}

// A keystroke's keys, split on spaces, as `<kbd>` chips.
const kbds = k => k.split(" ").filter(Boolean).map(x => {
  const c = document.createElement("kbd");
  c.textContent = x;
  return c;
});

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

// Cells kept clear left and right of the grid, so the borders don't touch the window's edges.
const MARGIN = 0.5;
// The bottom line's height (`#status`), below the grid.
const STATUS = 34;
function report() {
  measure();
  const cols = Math.floor(innerWidth / cw - 2 * MARGIN);
  const rows = Math.floor((innerHeight - $("top").offsetHeight - STATUS) / ch);
  const style = document.documentElement.style;
  style.setProperty("--cols", cols);
  style.setProperty("--rows", rows);
  style.setProperty("--side", (innerWidth - cols * cw) / 2 + "px");
  send({ view: { cols, rows } });
}

// Laura's keys inside a terminal: false leaves the key to app.js, unsent.
function termKeys(id, term, e) {
  if (overlayOpen()) return false;
  if (e.key === "F12") return false;
  if (locked) return true;
  if (draft) return false;
  if (e.key === "Escape" && (listOpen ?? moreOpen) !== null) return false;
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
    fontFamily: v("--mono"), fontSize: 14, lineHeight: 1, cursorBlink: true, allowProposedApi: true,
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
  // Laura brackets a paste, as the TUI does; xterm.js mustn't send it too. A box in an editor
  // pane (a comment, the review body) takes its own paste.
  el.addEventListener("paste", e => {
    if (e.target.closest?.(".draft, .pop")) return;
    e.preventDefault();
    e.stopPropagation();
    act({ act: "paste", pane: id, text: e.clipboardData.getData("text") });
  }, true);
  terms.set(id, term);
}

// The pane header's icons, from the mockup (plans/web-tui/web-workspace.html).
const I = {
  term: '<path d="m5 16 5-5-5-5M12 18h7"/>',
  file: '<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/>',
  page: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 9h18"/>',
  log: '<path d="M8 6h12M8 12h12M8 18h12M4 6h.01M4 12h.01M4 18h.01"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  more: '<circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>',
  comment: '<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/><path d="M12 7v6M9 10h6"/>',
  diff: '<path d="M12 4v9M7.5 8.5h9M7.5 19h9"/>',
  refresh: '<path d="M21 12a9 9 0 1 1-3-6.7L21 8"/><path d="M21 3v5h-5"/>',
  close: '<path d="M18 6 6 18M6 6l12 12"/>',
  target: '<circle cx="12" cy="12" r="8"/><circle cx="12" cy="12" r="2.5"/>',
  pick: '<path d="m9 9 5 12 1.8-5.2L21 14Z"/><path d="M7.2 2.2 8 5.1M5.1 8l-2.9-.8M14 4.1 12 6M6 12l-1.9 2"/>',
};
const ic = n => `<svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${I[n]}</svg>`;
// A tooltip that shows at once; `aria-label` reads the same text to a screen reader.
const tip = t => `data-tip="${t}" aria-label="${t}"`;
// The header's tools, which a narrow pane moves into **More**: action, icon, tooltip.
const TOOLS = [
  ["comment", "comment", "Comment on the cursor  c"],
  ["cm", "pick", "Comment mode: click an element to comment"],
  ["toggle_diff", "diff", "Diff against git HEAD  d"],
  ["refresh", "refresh", "Refresh: discard the unsubmitted inline review and reload  Ctrl+R"],
  ["close", "close", "Close the pane  x"],
];
// Static markup; `paintHead` sets the frame's strings as text. `.tl` is a label that a narrow
// pane hides.
const head = () => `<div class="head"><span class="pid"></span>${ic("file")}<span class="name tip-l"><i class="dot"></i><span class="nm"></span></span>`
  + '<span class="delta"><span class="add"></span><span class="del"></span></span>'
  + `<span class="seg tip-c" ${tip("Flip the view  Ctrl+L")}><button data-act="editor">E<span class="tl">ditor</span></button><button data-act="file">F<span class="tl">ile</span></button></span>`
  + '<span class="chips"><span class="chip warn unsaved">● Unsaved</span><span class="chip ok reloaded" hidden>Reloaded</span>'
  + '<span class="chip warn frozen">Frozen</span><span class="chip diffc">Diff view</span>'
  + `<button class="chip hlc tip-l" data-act="highlight" data-tip="Scroll to the highlight">${ic("target")}<span class="l"></span>`
  + `<span class="x" data-act="clear_highlight" ${tip("Clear the highlight  h")}>×</span></button>`
  + '<span class="chip bad sfail" hidden>Submission failed</span></span><i class="cdot" role="img"></i><span class="grow"></span>'
  + `<button class="threads" data-act="list" ${tip("All threads  n / N")}><span class="avs"></span><span class="tn"></span><span class="tl"></span></button>`
  + `<button class="submit" data-act="review" ${tip("Submit the inline review  Shift+S")}></button>`
  + `<span class="tools">${TOOLS.map(([a, i, t]) => `<button class="tool" data-act="${a}" ${tip(t)}>${ic(i)}</button>`).join("")}</span>`
  + `<button class="tool more" data-act="more" ${tip("More")}>${ic("more")}</button></div>`;

const hlLabel = ([lo, hi]) => `Highlight L${lo}${hi !== lo ? "–" + hi : ""}`;

// A narrow header folds its chips into one dot, which names them in its tooltip. The chips change
// the header's width, so the header fits again.
function paintChipDot(el) {
  const names = [...el.querySelectorAll(".head .chips > :not([hidden])")].map(c => (c.querySelector(".l") ?? c).textContent);
  const d = el.querySelector(".head .cdot");
  d.hidden = !names.length;
  d.dataset.tip = names.join("\n");
  d.setAttribute("aria-label", names.join(", "));
  fitHead(el);
}

// A narrow pane's header drops its parts in steps until it fits (CSS keys each step on `data-fit`).
// A name may shorten to 120px before a part drops.
function fitHead(el) {
  const h = el.querySelector(".head"), nm = h.querySelector(".nm");
  for (let n = 0; n <= 5; n++) {
    el.dataset.fit = n;
    if (h.scrollWidth <= h.clientWidth && nm.clientWidth >= Math.min(nm.scrollWidth, 120)) break;
  }
}

function paintHead(el, p) {
  const q = s => el.querySelector(".head " + s);
  const show = (s, on) => (q(s).hidden = !on);
  q(".pid").textContent = p.id;
  // `open --panel` can show a page or a log in a file's pane.
  q("> .ic").innerHTML = I[p.id === 0 ? "term" : p.icon ?? "file"];
  q(".nm").textContent = p.name;
  // The shell has no path, and an empty `data-tip` would show an empty box.
  if (p.path) q(".name").dataset.tip = p.path;
  else delete q(".name").dataset.tip;
  q(".add").textContent = `+${p.added}`;
  q(".del").textContent = `−${p.removed}`;
  show(".delta", p.added || p.removed);
  show(".add", p.added);
  show(".del", p.removed);
  show(".seg", p.pty && p.id !== 0);
  q("[data-act=editor]").classList.toggle("on", p.kind === "editor");
  q("[data-act=file]").classList.toggle("on", p.kind !== "editor");
  show(".unsaved", p.unsaved);
  show(".frozen", p.frozen);
  el.querySelector(".banner .bn").textContent = `${p.name} changed on disk.`;
  show(".diffc", p.diff);
  show(".hlc", p.highlight);
  if (p.highlight) q(".hlc .l").textContent = hlLabel(p.highlight);
  q(".threads .tn").textContent = p.threads;
  q(".threads .tl").textContent = ` thread${p.threads === 1 ? "" : "s"}`;
  show(".threads", p.threads);
  show(".submit", p.threads);
  show(".tools", p.kind === "file");
  show(".more", p.id !== 0);
  // An editor pane has no diff view (NO_DIFF_IN_EDITOR), and nor does a file with no changes.
  show(".tools [data-act=toggle_diff]", !p.pty && (p.added || p.removed || p.diff));
  q(".tools [data-act=toggle_diff]").classList.toggle("on", p.diff);
  // The narrowest pane shows only a ⚠, whose tooltip reads the line.
  const nw = el.querySelector(".narrow"), nt = strings.narrow?.replace("{name}", p.name) ?? "";
  nw.textContent = nw.dataset.tip = nt;
  nw.setAttribute("aria-label", nt);
  paintChipDot(el);
}

// A header button does what its keystroke does.
function headClick(id, e) {
  const b = e.target.closest("[data-act]");
  if (!b) return;
  e.stopPropagation();
  // **Retry submit** retries, as the review body's **Retry** does.
  if (b.dataset.act === "review" && draft?.kind === "review" && draft.pane === id && draft.failed) return confirmDraft();
  if (draft) return followFocus(true);
  const a = b.dataset.act;
  if (a === "more") {
    moreOpen = moreOpen === id ? null : id;
    listOpen = null;
    return paintPops();
  }
  if (moreOpen !== null) (moreOpen = null), paintPops();
  // Submit and the thread list work on the file view, as `Shift+S` and `n`/`N` do.
  if ((a === "review" || a === "list") && pane(id)?.kind === "editor") act({ act: "toggle_editor_view", pane: id });
  if (a === "highlight") return toHighlight(id);
  if (a === "list") {
    listOpen = listOpen === id ? null : id;
    return paintPops();
  }
  if (a === "editor" || a === "file") {
    if ((pane(id)?.kind === "editor") !== (a === "editor")) act({ act: "toggle_editor_view", pane: id });
    return;
  }
  if (layout.focus !== id && a !== "close") act({ act: "focus", pane: id });
  if (a === "cm") return setCommentMode(commentMode === id ? null : id);
  if (a === "comment" || a === "review") begin(id, a);
  else act({ act: a, pane: id });
}

// A frozen file pane's banner, which CSS shows; `paintHead` sets the name.
const BANNER = '<div class="banner"><span><b class="bn"></b> Laura keeps showing the snapshot you are reviewing.</span>'
  + '<span class="grow"></span><button class="btn pri" data-act="review">Submit review <kbd>Shift+S</kbd></button>'
  + '<button class="btn" data-act="refresh">Refresh <kbd>Ctrl+R</kbd></button></div>';

function makePane(p) {
  const el = document.createElement("div");
  el.id = "p" + p.id;
  el.className = "pane";
  el.innerHTML = head() + BANNER
    + `<div class="term"></div><div class="rows"></div><div class="narrow"></div><button class="cbtn" ${tip("Comment  c")} style="visibility: hidden">${ic("plus")}</button>`
    + '<div class="cmhint">Comment mode · click an element to comment <kbd>Esc</kbd><button data-act="cm">Done</button></div>';
  // A clicked button keeps no focus, so a later Enter can't press it again.
  for (const b of el.querySelectorAll(".head, .banner, .rows, .cbtn, .cmhint")) {
    b.addEventListener("mousedown", e => e.target.closest("button") && e.preventDefault());
  }
  const rows = el.querySelector(".rows"), btn = el.querySelector(".cbtn");
  btn.addEventListener("click", () => {
    if (draft) return followFocus(true);
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
  // A page's thread or comment box grows as the user types.
  new ResizeObserver(() => place(p.id)).observe(rows);
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
    const a = e.target.closest("a[data-href]");
    if (a) act({ act: "open_link", pane: p.id, href: a.dataset.href });
  });
  return el;
}

// The `+` on the hovered line (`el.plus`), just past its text and centred on its first line. A click
// or a reload replaces the line's element, so `onPane` places the `+` again from the new one.
function placePlus(id) {
  const el = $("p" + id), rows = el?.querySelector(".rows"), btn = el?.querySelector(".cbtn"), q = pane(id), h = el?.plus;
  if (!btn) return;
  const u = h && rows.querySelector(`:scope > [data-row="${h.row}"]`);
  const t = h?.inner ? u?.querySelector(`[data-line="${h.line}"]`) : u;
  // No `+` while a comment is open, in the diff view, or before `laura ready`, which a comment needs.
  const show = t && !draft && q?.kind === "file" && !q.diff && layout.agent;
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

function onLayout(m) {
  for (const p of m.panes) {
    const old = layout && (pane(p.id) ?? {});
    if (old && p.id !== m.focus && p.highlight && String(p.highlight) !== String(old.highlight)) unseen.add(p.id);
  }
  unseen.delete(m.focus);
  layout = m;
  // The list closes with its last thread, or its pane.
  if (!pane(listOpen)?.threads) listOpen = null;
  if (!pane(moreOpen) || moreOpen === 0) moreOpen = null;
  const seen = new Set(), later = root.children.length > 0;
  for (const p of m.panes) {
    // A pane that opens after the first layout eases in.
    const fresh = !$("p" + p.id) && later;
    const el = $("p" + p.id) ?? root.appendChild(makePane(p));
    if (fresh) el.animate([{ opacity: 0, transform: "translateY(6px)" }, {}], { duration: 350, easing: "ease-out" });
    seen.add(el.id);
    if (p.pty && !terms.has(p.id)) makeTerm(p.id, el);
    Object.assign(el.style, {
      left: `calc(var(--cw) * ${p.x})`, top: `calc(var(--ch) * ${p.y})`,
      width: `calc(var(--cw) * ${p.w})`, height: `calc(var(--ch) * ${p.h})`,
    });
    el.style.setProperty("--w", p.w);
    el.className = `pane ${p.kind}${p.id === m.focus ? " focused" : ""}${p.frozen ? " frozen" : ""}`
      + (unseen.has(p.id) ? " unseen" : "") + (commentMode === p.id ? " comment-mode" : "");
    paintHead(el, p);
    const t = terms.get(p.id);
    // The header takes the border's top two rows, and the bottom border one (`Tab::inner`).
    if (t && (t.cols !== p.w - 2 || t.rows !== p.h - 3)) t.resize(Math.max(1, p.w - 2), Math.max(1, p.h - 3));
    // The focus, the diff view and a freeze change how threads show.
    paintThreads(p.id);
  }
  for (const el of [...root.children]) {
    if (seen.has(el.id)) continue;
    const id = +el.id.slice(1);
    if (id === commentMode) commentMode = null;
    terms.get(id)?.dispose();
    terms.delete(id);
    el.remove();
  }
  followFocus();
  paintHint();
  paintStatus();
  // A pane that closes or freezes changes the pane selector's rows.
  if (popup !== null) paintSelector();
}

// The status bar: what waits on the user, in any pane. Names are the frame's, so text only.
function paintStatus() {
  const item = (p, text, dot, tipText) => {
    const b = document.createElement("button");
    b.className = `si tip-up ${dot}`;
    b.dataset.id = p.id;
    b.dataset.tip = `${p.name} · ${tipText}`;
    const d = document.createElement("span");
    d.className = `d ${dot}`;
    const t = document.createElement("span");
    t.className = "t";
    t.textContent = `${p.name} · ${text}`;
    b.setAttribute("aria-label", `${p.name} · ${tipText}`);
    b.append(d, t);
    return b;
  };
  const review = p => {
    const d = draft?.pane === p.id && draft.kind === "review" ? draft : null;
    const state = d?.busy ? "submitting" : d?.failed ? "submission failed" : d ? "review body open" : "in progress";
    return item(p, "unsubmitted inline review", d?.failed ? "bad" : "acc", `Unsubmitted inline review: ${state}`);
  };
  const items = [
    ...layout.panes.filter(p => p.threads).map(review),
    ...layout.panes.filter(p => p.unsaved).map(p => item(p, "unsaved Neovim edits", "warn", "Unsaved Neovim edits")),
  ];
  if (!layout.agent) {
    const s = document.createElement("span");
    s.className = "si";
    s.innerHTML = '<span class="d"></span>Waiting for <code>laura ready</code>';
    items.push(s);
  }
  $("sbar").replaceChildren(...items);
}
$("sbar").addEventListener("mousedown", e => e.preventDefault());
$("sbar").addEventListener("click", e => {
  const b = e.target.closest("[data-id]");
  if (!b) return;
  if (draft) return followFocus(true);
  act({ act: "focus", pane: +b.dataset.id });
});

// Laura reloaded the pane's file: **Reloaded** for a few seconds, and the unseen marker unless
// the user is looking at the pane.
function reloaded(id) {
  const el = $("p" + id), chip = el?.querySelector(".head .reloaded");
  if (!chip) return;
  chip.hidden = false;
  paintChipDot(el);
  clearTimeout(el.reloadTimer);
  el.reloadTimer = setTimeout(() => ((chip.hidden = true), paintChipDot(el)), 4000);
  if (id === layout.focus) return;
  unseen.add(id);
  el.classList.add("unseen");
}

// A row's or block's text, without its gutter.
function unitText(u) {
  const c = u.cloneNode(true);
  c.querySelectorAll(".g").forEach(g => g.remove());
  return c.textContent;
}

const esc = s => String(s).replace(/[&<>"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
// A comment's author: `null` is the user. The agent's names read as in the TUI.
const who = a => (a === null ? "You" : a === "agent" ? "Agent" : a);
const av = a => `<span class="av${a === null ? "" : " agent"}">${esc(([...who(a)][0] ?? "?").toUpperCase())}</span>`;
const avs = list => `<span class="avs">${[...new Set(list)].map(av).join("")}</span>`;
const authors = t => t.notes.map(n => n.author);
const HOLD = "Laura holds the agent’s commands while you type.";
// The rows' units (rows and blocks), without the threads between them.
const units = rows => rows.querySelectorAll(":scope > [data-row]");
const draftBox = () => document.querySelector(".draft textarea, .pop.review textarea");

// `drafting`: the draft is on the thread's line, so the card opens with a slot for it: in place of
// the user's last comment (an edit), or at the end (a reply).
function cardHTML(p, t, on, drafting) {
  const n = t.notes.length, edit = drafting && t.notes.at(-1)?.author === null;
  if (t.collapsed && !drafting) {
    return `<div class="card shut${on ? " on" : ""}" data-th="${t.line}" data-act="expand"><span class="anc">L${t.line}</span>`
      + `${avs(authors(t))}<span class="mute sm">${n} comment${n === 1 ? "" : "s"}</span></div>`;
  }
  return `<div class="card${on || drafting ? " on" : ""}" data-th="${t.line}"><div class="ch"><span class="anc">L${t.line}</span>`
    + (t.label ? `<span class="lbl">${esc(t.label)}</span>` : "")
    + `${p.frozen ? '<span class="chip warn sm">Snapshot</span>' : ""}<span class="grow"></span>`
    + `<button class="mini" data-act="collapse" ${tip("Collapse  r")}>–</button></div>`
    + (edit ? t.notes.slice(0, -1) : t.notes).map(c => `<div class="cm">${av(c.author)}<div><b>${esc(who(c.author))}</b><p>${esc(c.body)}</p></div></div>`).join("")
    + (edit ? `<div class="cm">${av(null)}<div><b>${who(null)}</b><i class="slot"></i></div></div>` : drafting ? '<i class="slot"></i>' : t.notes.at(-1)?.author === null
      ? '<button class="reply" data-act="reply">Edit…</button><button class="reply" data-act="delete">Delete</button>'
      : '<button class="reply" data-act="reply">Reply…</button>') + "</div>";
}

// The draft and the review popover keep the user's text in `draft.text`.
function wireDraft(box) {
  box.querySelector("textarea").addEventListener("input", e => draft && (draft.text = e.target.value));
  box.addEventListener("mousedown", e => e.target.closest("button") && e.preventDefault());
}

// `reply`: the thread on the draft's line, if any. `inside`: the draft goes in that thread's card
// (`cardHTML`'s slot), so it has no card of its own.
function makeDraftCard(reply, inside) {
  const c = document.createElement("div"), edit = reply?.notes.at(-1)?.author === null;
  const verb = edit ? "Edit" : reply ? "Reply" : "Comment", lbl = [reply && verb, reply?.label].filter(Boolean).join(" · ");
  c.className = inside ? "draft" : "card draft on";
  c.innerHTML = (inside ? "" : `<div class="ch"><span class="anc">L${draft.line}</span>${lbl ? `<span class="lbl">${esc(lbl)}</span>` : ""}</div>`)
    + `<textarea spellcheck="false" placeholder="${edit ? "Edit your comment" : `${verb} on L${draft.line}`}"></textarea>`
    + `<div class="holdnote">${HOLD}</div><div class="fa"><button class="btn" data-act="cancel">Cancel <kbd>Esc</kbd></button>`
    + `<button class="btn pri" data-act="save">${edit ? "Save" : verb} <kbd>Enter</kbd></button></div>`;
  c.querySelector("textarea").value = draft.text;
  wireDraft(c);
  c.addEventListener("click", e => {
    e.stopPropagation();
    const a = e.target.closest("[data-act]")?.dataset.act;
    if (a === "cancel") closeDraft();
    else if (a === "save") confirmDraft();
  });
  return c;
}

// Moving the draft box drops its focus: give the box back its focus and selection.
function keepDraft(fn) {
  const box = draftBox(), had = box && document.activeElement === box;
  const sel = box && [box.selectionStart, box.selectionEnd, box.selectionDirection];
  fn();
  if (had && box.isConnected && document.activeElement !== box) {
    box.focus();
    box.setSelectionRange(...sel);
  }
}

// Each thread as a card below its line, after the last unit at or above the line, and the draft
// in its line's thread, or below its line. A page shows the cursor's thread below its element (`place`), unless the thread is
// collapsed or comment mode just ended on it (`el.shut`), so the card doesn't cover the page. The diff view shows none.
function paintThreads(id) {
  const el = $("p" + id), rows = el?.querySelector(".rows"), p = pane(id);
  if (!rows || !p) return;
  const f = frameOf(id), all = el.threads ?? [];
  const d = draft?.pane === id && draft.kind === "comment" ? draft : null;
  let ts = p.diff ? [] : all;
  if (f) ts = ts.filter(t => t.line === el.line && t.line !== el.shut && !t.collapsed);
  const us = [...units(rows)];
  // The cards start at the text column, past the gutter.
  const g = us[0]?.querySelector(".g");
  if (g) rows.style.setProperty("--gx", g.offsetWidth + "px");
  const at = line => (f ? null : us.findLast(u => +u.dataset.line <= line) ?? null);
  // Anchor unit, or `null` for the top of the rows → its cards, by line.
  const groups = new Map();
  const put = (line, item) => {
    const u = at(line);
    if (!groups.has(u)) groups.set(u, []);
    groups.get(u).push([line, item]);
  };
  // A thread the pane doesn't show (the diff view) leaves the draft a card of its own.
  const host = d && ts.find(t => t.line === d.line);
  for (const t of ts) put(t.line, cardHTML(p, t, t.line === el.card && layout.focus === id, t === host));
  if (d) {
    // Laura holds the agent's commands while the user types, so the line's thread can't change mid-draft.
    draftCard ??= makeDraftCard(all.find(t => t.line === d.line), !!host);
    if (!host) put(d.line + 0.5, '<i class="slot"></i>');
  }
  // A unit with a thread below it is tinted; a thread that wasn't there before eases in.
  const anchored = new Set(ts.map(t => at(t.line)));
  for (const u of us) u.classList.toggle("has-th", anchored.has(u));
  const fresh = el.thLines ? ts.filter(t => !el.thLines.has(t.line)) : [];
  el.thLines = new Set(all.map(t => t.line));
  keepDraft(() => {
    const used = new Set();
    for (const [u, items] of groups) {
      let inl = u ? u.nextElementSibling : rows.firstElementChild;
      if (!inl?.classList.contains("inl")) {
        inl = document.createElement("div");
        inl.className = "inl";
        u ? u.after(inl) : rows.prepend(inl);
      }
      used.add(inl);
      const html = items.sort((a, b) => a[0] - b[0]).map(i => i[1]).join("");
      if (inl.html !== html) inl.innerHTML = inl.html = html;
      inl.querySelector(".slot")?.replaceWith(draftCard);
    }
    for (const inl of rows.querySelectorAll(":scope > .inl")) if (!used.has(inl)) inl.remove();
    for (const t of fresh) rows.querySelector(`.card[data-th="${t.line}"]`)?.classList.add("new");
  });
  el.querySelector(".head .threads .avs").outerHTML = avs(all.flatMap(authors));
  paintPop(id);
}

const REVIEW = '<div class="pop review"><div class="ph">Submit inline review <span class="mute"></span></div><p class="mute"></p>'
  + '<div class="err" hidden></div><textarea spellcheck="false" placeholder="Review body (optional): overall direction for the agent"></textarea>'
  + `<div class="holdnote">${HOLD}</div><div class="fa"><button class="btn" data-act="cancel">Cancel <kbd>Esc</kbd></button>`
  + '<button class="btn pri" data-act="save"></button></div></div>';

// The pane's popover under its header: the review body, the thread list, or **More**. The header's
// **Submit** button and **Submission failed** chip follow the review body.
function paintPop(id) {
  const el = $("p" + id), p = pane(id);
  if (!el || !p) return;
  const d = draft?.pane === id && draft.kind === "review" ? draft : null;
  const sb = el.querySelector(".head .submit");
  sb.innerHTML = d?.busy ? "Submitting…" : d?.failed ? 'Retry<span class="tl"> submit</span>' : 'Submit<span class="tl"> review</span>';
  sb.classList.toggle("submitting", !!d?.busy);
  sb.classList.toggle("failed", !!d?.failed);
  el.querySelector(".head .sfail").hidden = !d?.failed;
  paintChipDot(el);
  const kind = d ? "review" : listOpen === id && p.threads ? "list" : moreOpen === id ? "more" : "";
  el.querySelector(".head .threads").classList.toggle("on", kind === "list");
  el.querySelector(".head .more").classList.toggle("on", kind === "more");
  let wrap = el.querySelector(":scope > .popwrap");
  if (wrap && wrap.dataset.kind !== kind) wrap = (wrap.remove(), null);
  if (!kind) return;
  if (!wrap) {
    wrap = document.createElement("div");
    wrap.className = "popwrap";
    wrap.dataset.kind = kind;
    wrap.innerHTML = kind === "review" ? REVIEW : `<div class="pop ${kind}"></div>`;
    if (kind === "review") {
      wrap.querySelector("textarea").value = draft.text;
      wireDraft(wrap);
    } else wrap.addEventListener("mousedown", e => e.target.closest("button") && e.preventDefault());
    wrap.addEventListener("click", e => popClick(id, e));
    el.append(wrap);
  }
  const q = s => wrap.querySelector(s);
  if (kind === "review") {
    const n = p.threads;
    q(".ph .mute").textContent = `· ${p.name}`;
    q("p.mute").innerHTML = `${n} thread${n === 1 ? "" : "s"}${p.frozen ? " on the snapshot" : ""}. Laura writes them into the chat`
      + ` as one <code>[laura review · ${esc(p.path ?? p.name)}]</code> block, then clears the threads.`;
    q(".err").hidden = !draft.failed;
    q(".err").textContent = strings.draft?.failed ?? "";
    q("[data-act=save]").innerHTML = `${draft.busy ? "Submitting…" : draft.failed ? "Retry" : "Submit review"} <kbd>Enter</kbd>`;
    q("[data-act=save]").disabled = draft.busy;
    // The review body can't change while Laura submits it.
    q("textarea").readOnly = draft.busy;
    return;
  }
  if (kind === "more") {
    // The header's tools that this pane shows, which a narrow header hides; the narrowest also
    // hides **Submit**, the thread count and an editor pane's view flip.
    const row = (a, i, t, cls = "") => {
      const [label, key] = t.split("  ");
      return `<button class="mrow ${cls}" data-act="${a}">${ic(i)}<span>${label}</span>${key ? `<kbd>${key}</kbd>` : ""}</button>`;
    };
    // The editor view has no tools (`paintHead`).
    const shown = p.kind !== "file" ? [] : [...el.querySelectorAll(".head .tools .tool")].filter(b => getComputedStyle(b).display !== "none");
    q(".pop").innerHTML = (p.threads ? row("review", "comment", "Submit the inline review  Shift+S", "hd") + row("list", "comment", "All threads  n", "hd") : "")
      + (p.pty && p.id !== 0 ? row(p.kind === "editor" ? "file" : "editor", p.kind === "editor" ? "file" : "term", "Flip the view  Ctrl+L", "hd") : "")
      + shown.map(b => row(...TOOLS.find(t => t[0] === b.dataset.act))).join("");
    // **More** is wider than a narrow pane; it stays inside the window.
    const pop = q(".pop");
    pop.style.right = "";
    const x = pop.getBoundingClientRect().left;
    if (x < 8) pop.style.right = `${x}px`;
    return;
  }
  const ts = el.threads ?? [];
  q(".pop").innerHTML = `<div class="ph">${ts.length} thread${ts.length === 1 ? "" : "s"} <span class="mute">· ${esc(p.name)}</span></div>`
    + ts.map(t => `<button class="trow" data-th="${t.line}"><span class="anc">L${t.line}</span>${avs(authors(t))}`
      + `<span class="tx">${esc(t.notes.at(-1)?.body ?? "")}</span>${t.collapsed ? '<span class="chip sm">collapsed</span>' : ""}</button>`).join("")
    + '<div class="pf"><kbd>n</kbd> <kbd>N</kbd> next / previous thread · <kbd>r</kbd> collapse</div>';
}
const paintPops = () => layout?.panes.forEach(p => paintPop(p.id));

function popClick(id, e) {
  const b = e.target.closest("button");
  // **More**'s rows are the header's buttons.
  if (b?.classList.contains("mrow")) return headClick(id, e);
  e.stopPropagation();
  if (b?.dataset.th) {
    listOpen = null;
    paintPop(id);
    if (layout.focus !== id) act({ act: "focus", pane: id });
    // The cursor may already be on the thread's line and send no frame, so the reply scrolls too.
    $("p" + id).goTh = { th: b.dataset.th, block: "center" };
    return act({ act: "set_cursor", pane: id, line: +b.dataset.th }, () => goThread(id));
  }
  if (b?.dataset.act === "cancel") closeDraft();
  else if (b?.dataset.act === "save") confirmDraft();
}
// After a click in the thread list or a new comment, the pane scrolls the card of the thread into
// view. The pane keeps the request until the card shows, because the thread of a new comment can
// arrive after the reply.
function goThread(id) {
  const el = $("p" + id), go = el?.goTh;
  const card = go && el.querySelector(`.card[data-th="${go.th}"]`);
  if (!card) return;
  el.goTh = null;
  card.scrollIntoView({ block: go.block });
}
// A click outside the thread list or **More** closes it.
addEventListener("mousedown", e => {
  if (listOpen !== null && !e.target.closest(".pop.list, [data-act=list]")) {
    const id = listOpen;
    listOpen = null;
    paintPop(id);
  }
  if (moreOpen !== null && !e.target.closest(".pop.more, [data-act=more]")) {
    const id = moreOpen;
    moreOpen = null;
    paintPop(id);
  }
}, true);

// A thread's collapse button or a collapsed thread toggles that thread alone, even on a block's
// first line, where `r` toggles the block's threads. **Reply…** comments on the thread's line, as `c`;
// **Delete** saves the user's last comment empty, which deletes it.
function cardClick(id, e) {
  const c = e.target.closest(".card[data-th]"), b = e.target.closest("[data-act]");
  if (!c || !b) return;
  e.stopPropagation();
  if (draft) return followFocus(true);
  if (layout.focus !== id) act({ act: "focus", pane: id });
  if (b.dataset.act === "delete") return act({ act: "comment", pane: id, line: +c.dataset.th, text: "" });
  if (b.dataset.act !== "reply") return act({ act: "toggle_thread", pane: id, line: +c.dataset.th });
  act({ act: "set_cursor", pane: id, line: +c.dataset.th });
  begin(id, "comment");
}

function begin(id, kind) {
  act({ act: kind === "comment" ? "begin_comment" : "begin_review", pane: id }, r => {
    if (!r.ok) return;
    listOpen = null;
    draft = { pane: id, line: r.line, kind, hint: r.hint, busy: false, failed: false, text: r.text ?? "" };
    paintThreads(id);
    paintPops();
    paintHint();
    paintStatus();
    followFocus(true);
  });
}

// `Enter`: add or save the comment, or submit the inline review. A failure keeps the draft for a retry.
function confirmDraft() {
  const d = draft;
  if (d.busy) return;
  d.busy = true;
  d.failed = false;
  paintPop(d.pane);
  paintStatus();
  act(d.kind === "comment" ? { act: "comment", pane: d.pane, line: d.line, text: d.text } : { act: "submit", pane: d.pane, body: d.text },
    r => {
      if (r.ok) {
        // The saved card is taller than the draft, so the saved card can overflow the bottom edge of the pane.
        if (d.kind === "comment") $("p" + d.pane).goTh = { th: d.line, block: "nearest" };
        closeDraft();
        return goThread(d.pane);
      }
      if (draft !== d) return;
      d.busy = false;
      d.failed = true;
      paintPop(d.pane);
      paintStatus();
    });
}

function closeDraft() {
  if (!draft) return;
  const id = draft.pane;
  draft = null;
  draftCard?.remove();
  draftCard = null;
  send({ hold: false });
  if (notice === strings.draft.failed) (notice = ""), paintNotice();
  paintThreads(id);
  paintHint();
  paintStatus();
  followFocus(true);
}

// The cursor's open thread, row or block, or the line inside the block that the cursor is on.
function cursorEl(rows) {
  const unit = units(rows)[rows.dataset.cursor];
  return rows.querySelector(".card.on:not(.draft)") ?? unit?.querySelector(".md .cur") ?? unit;
}

// DOM focus follows the workspace's focus, only when it changes, so a title or resize frame doesn't
// take focus from anything else.
function followFocus(force) {
  const f = focused();
  const key = `${layout?.focus}:${f?.kind}:${locked}:${!!draft}`;
  if (!f || (key === focusKey && !force)) return;
  focusKey = key;
  // A frame that moves the focus doesn't take the keystrokes from the pane selector.
  if (popup !== null) return pq.focus();
  const box = draftBox();
  if (draft && !locked && box) return box.focus();
  // A click in a page's text field keeps the field's focus.
  if (document.activeElement === frameOf(f.id)) return;
  const t = f.kind !== "file" ? terms.get(f.id) : locked ? terms.get(0) : null;
  if (t) return t.focus();
  for (const t of terms.values()) t.blur();
  root.focus();
}

function onPane(m) {
  const el = $("p" + m.id), rows = el?.querySelector(".rows");
  if (!rows) return;
  // A page's `pane` frame is its diff view, which has no page to comment on.
  if (commentMode === m.id) setCommentMode(null);
  frameOf(m.id)?.remove();
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
  if (m.recenter) toHighlight(m.id);
  else if (key !== rows.dataset.key) {
    cur?.scrollIntoView({ block: "nearest" });
    // The cursor's row stays in view above its tall thread.
    const u = units(rows)[rows.dataset.cursor];
    if (u?.classList.contains("row")) u.scrollIntoView({ block: "nearest" });
  }
  rows.dataset.key = key;
  goThread(m.id);
  placePlus(m.id);
}

const frameOf = id => $("p" + id)?.querySelector("iframe");

// Scrolls the pane to its highlight; a page's frame scrolls itself.
function toHighlight(id) {
  const f = frameOf(id);
  if (f) return f.contentWindow?.postMessage({ ...f.state, scroll: true }, "*");
  const rows = $("p" + id)?.querySelector(".rows");
  // ponytail: row-proportional, exact for code, approximate for wrapped prose
  const h = rows?.querySelector(".hl");
  if (h) rows.scrollTop = h.offsetTop + h.offsetHeight * +(h.dataset.at ?? .5) - rows.clientHeight / 2;
}

// An HTML file's page in a sandboxed frame over the pane's body, and the cursor's thread over it.
function onPage(m) {
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
  const threads = m.threads.map(t => ({
    line: t.line, n: t.notes.length,
    who: [...new Set(authors(t))].map(a => [([...who(a)][0] ?? "?").toUpperCase(), a !== null]),
  }));
  f.state = { cursor: m.cursor, line: m.line, threads, highlight: m.highlight, comment: commentMode === m.id };
  f.contentWindow?.postMessage(f.state, "*");
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
function setCommentMode(id) {
  const old = commentMode;
  commentMode = id;
  for (const i of [old, id]) {
    if (i === null) continue;
    const el = $("p" + i);
    el?.classList.toggle("comment-mode", i === commentMode);
    if (el) {
      el.shut = i === commentMode ? null : el.line;
      paintThreads(i);
    }
    const f = frameOf(i);
    if (!f?.state) continue;
    f.state.comment = i === commentMode;
    f.contentWindow?.postMessage(f.state, "*");
  }
  paintHint();
}

// A page's cursor thread or comment box floats below the cursor's element (`f.rect`, from the
// page), inside the frame.
function place(id) {
  const f = frameOf(id), rows = $("p" + id)?.querySelector(".rows");
  if (!f || !rows) return;
  const [, left, bottom] = f.rect ?? [0, 0, 0];
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
  if (draft) return followFocus(true);
  if (m.laura === "line") {
    if (layout.focus !== id) act({ act: "focus", pane: id });
    // A pin on the cursor's thread shows or hides its card; another pin moves the cursor to its
    // thread. A collapsed thread expands.
    const el = f.parentElement;
    if (m.pin === true) root.focus();
    if (m.pin === true && el.threads?.find(t => t.line === m.line)?.collapsed) {
      return act({ act: "toggle_thread", pane: id, line: m.line });
    }
    if (m.pin === true && m.line === el.line) {
      el.shut = el.shut === m.line ? null : m.line;
      return paintThreads(id);
    }
    if (Number.isInteger(m.line)) act({ act: "set_cursor", pane: id, line: m.line });
    // A text field or a selection keeps the keystrokes; anything else gives them back to Laura.
    if (!m.editable) root.focus();
    // Only in comment mode, so a page's script can't open a comment box.
    if (m.pick === true && commentMode === id && Number.isInteger(m.line)) begin(id, "comment");
  } else if (m.laura === "esc") {
    root.focus();
    if (commentMode === id) setCommentMode(null);
  }
  else if (m.laura === "link" && typeof m.href === "string") {
    // The workspace's policy lets a frame load only the workspace's own URLs.
    if (/^https?:/i.test(m.href)) open(m.href, "_blank", "noopener");
    else act({ act: "open_link", pane: id, href: m.href });
  }
});

// The pane selector: panes whose name or path holds the query, or whose id starts with it. The id
// that equals the query comes first so that `Enter` picks it.
const pq = $("pq"), veil = $("veil");
function matches() {
  const q = pq.value.trim().toLowerCase(), id = p => String(p.id) === q;
  return layout.panes.filter(p => p.name.toLowerCase().includes(q) || (p.path ?? "").toLowerCase().includes(q) || String(p.id).startsWith(q))
    .sort((a, b) => id(b) - id(a));
}

// Each row shows the header's chips. Static markup; the frame's strings go in as text.
function paintSelector() {
  const ps = matches();
  popup = Math.max(0, Math.min(popup, ps.length - 1));
  const rows = ps.map((p, i) => {
    const b = document.createElement("button");
    b.className = `prow${i === popup ? " on" : ""}`;
    b.dataset.id = p.id;
    b.innerHTML = `<span class="pid"></span>${ic(p.id === 0 ? "term" : p.icon ?? "file")}`
      + '<span class="name"></span><span class="mute"></span><span class="grow"></span>';
    b.querySelector(".pid").textContent = p.id;
    b.querySelector(".name").textContent = p.name;
    b.querySelector(".mute").textContent = p.path ?? "";
    const chip = (kind, text) => {
      const c = document.createElement("span");
      c.className = `chip ${kind}`;
      c.textContent = text;
      b.append(c);
      return c;
    };
    if (unseen.has(p.id)) chip("new", "New");
    if (p.frozen) chip("warn", "Frozen");
    if (p.unsaved) chip("warn", "Unsaved");
    if (p.highlight) chip("hlc", "Highlight");
    if (p.threads) {
      chip("", `${p.threads} unsubmitted`)
        .insertAdjacentHTML("afterbegin", avs(($("p" + p.id)?.threads ?? []).flatMap(authors)));
    }
    return b;
  });
  const empty = document.createElement("div");
  empty.className = "empty";
  empty.textContent = "No pane matches.";
  $("prows").replaceChildren(...(rows.length ? rows : [empty]));
  rows[popup]?.scrollIntoView({ block: "nearest" });
}

function openSelector() {
  pq.value = "";
  popup = 0;
  paintSelector();
  veil.hidden = $("popup").hidden = false;
  pq.focus();
  paintHint();
}

// Closes the pane selector, focusing pane `id` if given.
function closeSelector(id) {
  popup = null;
  veil.hidden = $("popup").hidden = true;
  if (id !== undefined) act({ act: "focus", pane: id });
  followFocus(true);
  paintHint();
}

// As in the TUI, typed digits focus their pane at once when no other id or file name could follow.
pq.addEventListener("input", () => {
  const q = pq.value.trim();
  if (/^\d+$/.test(q)) {
    const hit = layout.panes.find(p => String(p.id) === q);
    const more = layout.panes.some(p => (String(p.id) !== q && String(p.id).startsWith(q)) || p.name.startsWith(q));
    if (hit && !more) return closeSelector(hit.id);
  }
  popup = 0;
  paintSelector();
});
$("prows").addEventListener("click", e => {
  const b = e.target.closest(".prow");
  if (b) closeSelector(+b.dataset.id);
});
$("findbtn").addEventListener("mousedown", e => e.preventDefault());
$("findbtn").addEventListener("click", () => (draft ? followFocus(true) : openSelector()));

function toggleHelp() {
  help = !help;
  veil.hidden = $("help").hidden = !help;
}
$("helpbtn").addEventListener("mousedown", e => e.preventDefault());
$("helpbtn").addEventListener("click", toggleHelp);
// A click outside the help sheet or the pane selector closes it.
veil.addEventListener("mousedown", e => {
  if (e.target !== veil) return;
  e.preventDefault();
  if (help) toggleHelp();
  if (popup !== null) closeSelector();
});

// The help sheet: a table of keystrokes per group.
function paintHelp(groups, note) {
  const p = document.createElement("p");
  p.className = "mute sm note";
  p.textContent = note;
  $("help").replaceChildren(...groups.flatMap(([group, keys]) => {
    const h = document.createElement("h3");
    h.textContent = group;
    const t = document.createElement("table");
    for (const [key, desc] of keys) {
      const r = t.insertRow();
      r.insertCell().append(...kbds(key));
      r.insertCell().textContent = desc;
    }
    return [h, t];
  }), p);
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
  if (help) return take(), toggleHelp();
  // A click can open the pane selector under the lock; it still takes its keystrokes.
  if (popup !== null) {
    // Ctrl+R/L/Q/H and Tab would reach the browser; every other keystroke goes to the input.
    if (e.key === "Escape" || (ctrl && k === "p")) return take(), closeSelector();
    if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      take();
      popup += e.key === "ArrowDown" ? 1 : -1;
      return paintSelector();
    }
    if (e.key === "Enter" && !e.isComposing) {
      take();
      const id = $("prows").children[popup]?.dataset.id;
      return id !== undefined && closeSelector(+id);
    }
    if ((ctrl && "rlqh".includes(k)) || e.key === "Tab") return take();
    if (document.activeElement !== pq) pq.focus();
    return;
  }
  if (locked) return;
  if (confirmQuit) {
    take();
    confirmQuit = false;
    if (e.key === "y") send({ quit: true });
    return paintHint();
  }
  const f = focused();
  if (ctrl && k === "h") {
    take();
    return toggleHelp();
  }
  if (draft) {
    // Enter confirms; Shift+Enter falls through as a newline. Ctrl+R/L/P/Q and Tab would reach the browser.
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) return take(), confirmDraft();
    if (e.key === "Escape") return take(), closeDraft();
    if ((ctrl && "rlpq".includes(k)) || e.key === "Tab") return take();
    const box = draftBox();
    if (box && document.activeElement !== box) box.focus();
    return;
  }
  if (e.key === "Escape" && (listOpen ?? moreOpen) !== null) {
    take();
    listOpen = moreOpen = null;
    return paintPops();
  }
  // As the hint line: comment mode stays on a page the focus left, and `Esc` goes to the focused pane.
  if (e.key === "Escape" && commentMode !== null && commentMode === layout.focus) return take(), setCommentMode(null);
  if (ctrl && k === "p") return take(), openSelector();
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
    // A page with no element at the cursor has nothing to comment on: pick one.
    const fr = frameOf(f.id);
    if (e.key === "c" && layout.agent && fr && !fr.rect) return setCommentMode(f.id);
    return begin(f.id, e.key === "c" ? "comment" : "review");
  }
  // The arrows don't move the cursor in a page, which has no rows.
  if (e.key.startsWith("Arrow") && frameOf(f.id)) return;
  const make = !ctrl && !e.altKey && !e.metaKey && FILE_KEYS[e.key];
  if (make) {
    take();
    act(make(f.id));
  }
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
    if (pane(id)?.kind !== "file") {
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
        paintHelp(m.help, m.help_note);
        $("folder").textContent = m.root;
        document.title = `Laura · ${m.root.split("/").filter(Boolean).at(-1) ?? m.root}`;
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
      case "page":
        onPage(m);
        break;
      case "acted": {
        const then = acts.shift();
        if (!m.ok) showNotice(m.text, 5);
        then?.(m);
        break;
      }
      case "notice":
        showNotice(m.text, 10);
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

// The cell size comes from the bundled mono font, not a fallback, so load both before measuring.
Promise.all(["14px Inter", '14px "JetBrains Mono"'].map(f => document.fonts.load(f))).finally(() => {
  measure();
  connect();
});
