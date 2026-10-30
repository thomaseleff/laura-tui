// The web app (#88). One websocket: JSON text frames for layout, panes and replies; binary frames
// `[pane id, 8 bytes BE][bytes]` for each terminal, and each browser pane's frames (#107). Every string from a frame goes in as text or
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
// An event's modifiers as CDP counts them: Alt 1, Ctrl 2, Meta 4, Shift 8.
const mods = e => e.altKey | e.ctrlKey << 1 | e.metaKey << 2 | e.shiftKey << 3;
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
  const f = focused(), b = f?.kind === "browser" && $("p" + f.id);
  // A page's frame with the keyboard is a field that has the keystrokes, as in a browser pane.
  const typing = b?.typing || (f && document.activeElement === frameOf(f.id));
  const here = document.activeElement?.matches?.(".head .url") ? h.browser_url
    : b?.dialog ? (b.dialog.type === "beforeunload" ? h.browser_leave : h.browser_dialog) : typing ? h.browser_typing : h.pane;
  // As the keydown listener: comment mode's hint only over a file or browser pane's own plain hint.
  const pairs = locked ? h.locked : confirmQuit ? h.quit : draft ? draft.hint
    : popup !== null ? h.panes : commentMode !== null && (f?.kind === "file" || b) && here === h.pane ? h.comment_mode : here;
  hintEl.replaceChildren(...(pairs ?? []).flatMap(([k, label], i) => {
    const s = document.createElement("span");
    s.textContent = label;
    const sep = document.createElement("i");
    sep.className = "sep";
    return [...(i ? [sep] : []), ...kbds(k), s];
  }));
}
// Focus back from a page's frame, such as a click on its pane header: the typing hint goes.
addEventListener("focus", paintHint);

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
  globe: '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/>',
  back: '<path d="M19 12H5M12 19l-7-7 7-7"/>',
  fwd: '<path d="M5 12h14M12 5l7 7-7 7"/>',
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
const head = () => `<div class="head"><span class="pid"></span>`
  + `<span class="navb"><button class="tool" data-act="back" ${tip("Back  Alt+←")}>${ic("back")}</button><button class="tool" data-act="fwd" ${tip("Forward  Alt+→")}>${ic("fwd")}</button></span>`
  + `${ic("file")}<span class="name tip-l"><i class="dot"></i><span class="nm"></span></span><input class="url" spellcheck="false" aria-label="Address">`
  + '<span class="delta"><span class="add"></span><span class="del"></span></span>'
  + `<span class="seg tip-c" ${tip("Flip the view  Ctrl+L")}><button data-act="editor">E<span class="tl">ditor</span></button><button data-act="file">F<span class="tl">ile</span></button></span>`
  + '<span class="chips"><span class="chip warn unsaved">● Unsaved</span><span class="chip ok reloaded" hidden>Reloaded</span>'
  + '<span class="chip warn frozen">Frozen</span><span class="chip diffc">Diff view</span>'
  + `<button class="chip hlc tip-l" data-act="highlight" data-tip="Scroll to the highlight">${ic("target")}<span class="l"></span>`
  + `<span class="x" data-act="clear_highlight" ${tip("Clear the highlight  h")}>×</span></button>`
  + '<span class="chip bad sfail" hidden>Submission failed</span></span><i class="cdot" role="img"></i><span class="grow"></span>'
  + `<span class="chip new agent tip-c" ${tip("The agent's browser tool clicks or types on this page")} hidden>Agent</span>`
  + `<span class="chip emu tip-c" ${tip("The agent's browser tools set the page to this size")} hidden></span><span class="spin" hidden></span><span class="live tip-c" hidden></span>`
  + `<button class="threads" data-act="list" ${tip("All threads  n / N")}><span class="avs"></span><span class="tn"></span><span class="tl"></span></button>`
  + `<button class="submit" data-act="review" ${tip("Submit the inline review  Shift+S")}></button>`
  + `<span class="tools">${TOOLS.map(([a, i, t]) => `<button class="tool" data-act="${a}" ${tip(t)}>${ic(i)}</button>`).join("")}</span>`
  + `<button class="tool more" data-act="more" ${tip("More")}>${ic("more")}</button></div>`;

// A file pane's lines, or a browser pane's selector.
const hlLabel = h => (typeof h === "string" ? `Highlight ${h}` : `Highlight L${h[0]}${h[1] !== h[0] ? "–" + h[1] : ""}`);

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
  show(".tools", p.kind === "file" || p.kind === "browser");
  // A browser pane shows its URL, and its page reloads in place of a file.
  const browser = p.kind === "browser";
  show(".url", browser);
  show(".navb", browser);
  // The user's typing stays until Enter or Esc.
  if (document.activeElement !== q(".url")) q(".url").value = browser ? p.path : "";
  show(".live", browser && el.live);
  show(".emu", browser && el.emulated);
  show(".agent", browser && !!el.acting);
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
  if (a === "back" || a === "fwd") return act({ act: "browser_history", pane: id, delta: a === "back" ? -1 : 1 });
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
    + `<div class="term"></div><div class="view"><canvas></canvas></div><div class="rows"></div><div class="narrow"></div><button class="cbtn" ${tip("Comment  c")} style="visibility: hidden">${ic("plus")}</button>`
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
  el.lags = [];
  new ResizeObserver(() => sizeView(p.id)).observe(el.querySelector(".view"));
  pointer(el, p.id);
  // The address field: a click selects the URL, Enter goes there (the keydown listener), and
  // leaving it puts the pane's URL back.
  const url = el.querySelector(".head .url");
  url.addEventListener("focus", () => (url.select(), paintHint()));
  url.addEventListener("blur", () => ((url.value = pane(p.id)?.path ?? ""), paintHint()));
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
    // A browser pane that turned into a file pane drops the page's state.
    if (p.kind !== "browser" && el.classList.contains("browser")) {
      el.emulated = el.dialog = el.marks = null;
      el.typing = el.acting = false;
      el.querySelector(".view").classList.remove("emu");
      el.querySelector(".dlgwrap")?.remove();
      el.querySelector(".marks")?.remove();
    }
    el.className = `pane ${p.kind}${p.id === m.focus ? " focused" : ""}${p.frozen ? " frozen" : ""}`
      + (unseen.has(p.id) ? " unseen" : "") + (commentMode === p.id ? " comment-mode" : "");
    paintHead(el, p);
    const t = terms.get(p.id);
    // The header takes the border's top two rows, and the bottom border one (`Tab::inner`).
    if (t && (t.cols !== p.w - 2 || t.rows !== p.h - 3)) t.resize(Math.max(1, p.w - 2), Math.max(1, p.h - 3));
    // The focus, the diff view and a freeze change how threads show.
    paintThreads(p.id);
    sizeView(p.id);
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

// A browser pane's size in CSS pixels, with a whole number of device pixels each way, so a frame
// draws 1:1. Laura's browser renders the page at this size.
function sizeView(id) {
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
  const off = () => pane(id)?.kind !== "browser" || el.dialog || draft;
  // In comment mode, a click picks the element under it, and the page gets no press.
  c.addEventListener("pointerdown", e => {
    if (off() || commentMode === id) return;
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
    if (commentMode === id && !el.held) {
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

// The spinner in a browser pane's header: while the page loads, handles a click, or waits on a
// request the click started.
function paintSpin(el) {
  el.querySelector(".head .spin").hidden = !el.busy;
  fitHead(el);
}

// A browser pane's frame: `[paint time, 8 bytes BE f64 ms][PNG]`. The ack goes at once, so Laura's
// browser paints the next frame while this one decodes. Only the newest frame waiting decodes.
function onFrame(id, data) {
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
function emuFit(el) {
  const e = el.emulated, c = el.querySelector(".view canvas");
  if (!e) return { x: 0, y: 0, s: 1 };
  const s = Math.min(1, (c.clientWidth - 24) / e.w, (c.clientHeight - 24) / e.h);
  return { x: (c.clientWidth - e.w * s) / 2, y: (c.clientHeight - e.h * s) / 2, s };
}

// The last frame, 1:1 from the top left, or an emulated page in its box with a shadow; a resize
// draws it again until the page's next frame.
// ponytail: Chrome sends an emulated page's frame at up to the pane's device pixels, a little more
// than the box draws; restart the screencast at the box's size if the softness shows.
function drawFrame(el) {
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
  live.classList.toggle("lag", slow);
  live.textContent = slow ? `${Math.round(el.lagShown)} ms` : "live";
  const t = `${el.lags.length} frames a second · lag ${Math.round(lag)} ms`;
  live.dataset.tip = t;
  live.setAttribute("aria-label", t);
  // A still page sends no frames: the badge settles without them.
  clearTimeout(el.liveTimer);
  if (el.lags.length || slow) el.liveTimer = setTimeout(() => paintLive(el), 500);
}

// A browser pane's page: the badge shows while Laura's browser runs the page; the pointer, the
// spinner, back and forward, and the page's dialog follow the page.
function onBrowser(m) {
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
function onAgent(m) {
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

// A pin's authors: each one's initial, and whether it's an agent.
const pinWho = t => [...new Set(authors(t))].map(a => [([...who(a)][0] ?? "?").toUpperCase(), a !== null]);

// A browser pane's outlines, tags and pins, drawn over the canvas as the agent's marks are: the page
// holds none of them, so the agent's screenshots and snapshots show the page as the user's browser
// would. browser.js sends the boxes in the frame's px.
function paintMarks(id) {
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
    pin.addEventListener("click", () => (draft ? followFocus(true) : pinClick(id, line)));
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

function answer(id, accept) {
  const text = $("p" + id)?.querySelector(".dlg input")?.value ?? "";
  act({ act: "browser_dialog", pane: id, accept, text });
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
// A thread's chip: a browser pane's element, cut from the left to `n` characters, else its line.
const chip = (anchor, line, n) => esc(!anchor ? "L" + line : anchor.length > n ? "…" + anchor.slice(1 - n) : anchor);
const HOLD = "Laura holds the agent’s commands while you type.";
// The rows' units (rows and blocks), without the threads between them.
const units = rows => rows.querySelectorAll(":scope > [data-row]");
const draftBox = () => document.querySelector(".draft textarea, .pop.review textarea");

// `drafting`: the draft is on the thread's line, so the card opens with a slot for it: in place of
// the user's last comment (an edit), or at the end (a reply).
// A browser pane's thread on another page, or whose element the page lost.
const where = t => t.lost ? '<span class="chip sm">not on the page</span>' : t.away ? `<span class="chip sm">${esc(t.away)}</span>` : "";

function cardHTML(p, t, on, drafting) {
  const n = t.notes.length, edit = drafting && t.notes.at(-1)?.author === null;
  if (t.collapsed && !drafting) {
    return `<div class="card shut${on ? " on" : ""}" data-th="${t.line}" data-act="expand"><span class="anc">${chip(t.anchor, t.line, 40)}</span>`
      + `${avs(authors(t))}<span class="mute sm">${n} comment${n === 1 ? "" : "s"}</span></div>`;
  }
  return `<div class="card${on || drafting ? " on" : ""}" data-th="${t.line}"><div class="ch"><span class="anc">${chip(t.anchor, t.line, 40)}</span>`
    + (t.label ? `<span class="lbl">${esc(t.label)}</span>` : "") + where(t)
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
  const verb = edit ? "Edit" : reply ? "Reply" : "Comment";
  const lbl = [reply && verb, reply?.label ?? draft.anchor?.[1]].filter(Boolean).join(" · ");
  const at = chip(draft.anchor?.[0], draft.line, 40);
  c.className = inside ? "draft" : "card draft on";
  c.innerHTML = (inside ? "" : `<div class="ch"><span class="anc">${at}</span>${lbl ? `<span class="lbl">${esc(lbl)}</span>` : ""}</div>`)
    + `<textarea spellcheck="false" placeholder="${edit ? "Edit your comment" : `${verb} on ${at}`}"></textarea>`
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
// collapsed or comment mode just ended on it (`el.shut`), so the card doesn't cover the page. A page shows no thread whose
// element the page lost: that thread and its draft are in the thread list (`paintPop`). The diff view shows none.
function paintThreads(id) {
  const el = $("p" + id), rows = el?.querySelector(".rows"), p = pane(id);
  if (!rows || !p) return;
  const f = frameOf(id), all = el.threads ?? [];
  const d = draft?.pane === id && draft.kind === "comment" ? draft : null;
  let ts = p.diff ? [] : all;
  if (f) ts = ts.filter(t => t.line === el.line && t.line !== el.shut && !t.collapsed && !t.lost);
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
  const host = d && (ts.find(t => t.line === d.line) ?? all.find(t => t.line === d.line && t.lost));
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
      + ` as one <code>[laura review · pane #${p.id} · ${esc(p.path ?? p.name)}]</code> block, then clears the threads.`;
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
    const shown = p.kind !== "file" && p.kind !== "browser" ? [] : [...el.querySelectorAll(".head .tools .tool")].filter(b => getComputedStyle(b).display !== "none");
    q(".pop").innerHTML = (p.threads ? row("review", "comment", "Submit the inline review  Shift+S", "hd") + row("list", "comment", "All threads  n", "hd") : "")
      + (p.pty && p.id !== 0 ? row(p.kind === "editor" ? "file" : "editor", p.kind === "editor" ? "file" : "term", "Flip the view  Ctrl+L", "hd") : "")
      + shown.map(b => row(b.dataset.act, TOOLS.find(t => t[0] === b.dataset.act)[1], b.dataset.tip)).join("");
    // **More** is wider than a narrow pane; it stays inside the window.
    const pop = q(".pop");
    pop.style.right = "";
    const x = pop.getBoundingClientRect().left;
    if (x < 8) pop.style.right = `${x}px`;
    return;
  }
  // A lost thread's card, and its draft, open below its row (`el.lostOpen`).
  const ts = el.threads ?? [], dl = draft?.pane === id && draft.kind === "comment" ? draft.line : null;
  const html = `<div class="ph">${ts.length} thread${ts.length === 1 ? "" : "s"} <span class="mute">· ${esc(p.name)}</span></div>`
    + ts.map(t => `<button class="trow${t.lost ? " lost" : ""}" data-th="${t.line}"><span class="anc">${chip(t.anchor, t.line, 24)}</span>${avs(authors(t))}`
      + `<span class="tx">${esc(t.notes.at(-1)?.body ?? "")}</span>`
      + where(t)
      + `${t.collapsed ? '<span class="chip sm">collapsed</span>' : ""}</button>`
      + (t.lost && el.lostOpen === t.line ? cardHTML(p, { ...t, collapsed: false }, true, dl === t.line) : "")).join("")
    + '<div class="pf"><kbd>n</kbd> <kbd>N</kbd> next / previous thread · <kbd>r</kbd> collapse</div>';
  const pop = q(".pop");
  keepDraft(() => {
    if (pop.html !== html) pop.innerHTML = pop.html = html;
    if (draftCard) pop.querySelector(".slot")?.replaceWith(draftCard);
  });
}
const paintPops = () => layout?.panes.forEach(p => paintPop(p.id));

function popClick(id, e) {
  const b = e.target.closest("button");
  // **More**'s rows are the header's buttons.
  if (b?.classList.contains("mrow")) return headClick(id, e);
  e.stopPropagation();
  const el = $("p" + id), c = b && e.target.closest(".card[data-th]");
  // A lost thread's card in the list: **–** closes it; **Reply…** and **Delete** act as on the page.
  if (c && b.dataset.act === "collapse") return (el.lostOpen = null), paintPop(id);
  if (c) return cardClick(id, e);
  if (b?.dataset.th && el.threads?.find(t => t.line === +b.dataset.th)?.lost) {
    if (draft) return followFocus(true);
    el.lostOpen = el.lostOpen === +b.dataset.th ? null : +b.dataset.th;
    return paintPop(id);
  }
  if (b?.dataset.th) {
    listOpen = null;
    paintPop(id);
    if (layout.focus !== id) act({ act: "focus", pane: id });
    // The cursor may already be on the thread's line and send no frame, so the reply scrolls too.
    const line = +b.dataset.th;
    el.goTh = { th: b.dataset.th, block: "center" };
    // A collapsed thread expands, as a click on its pin or its card does.
    const a = el.threads?.find(t => t.line === line)?.collapsed ? "toggle_thread" : "set_cursor";
    return act({ act: a, pane: id, line }, () => goThread(id));
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
  if (listOpen !== null && !e.target.closest(".pop.list, [data-act=list]") && !draftCard?.closest(".pop.list")) {
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

// `msg`: the act that starts the draft, when not `begin_comment` or `begin_review`, such as a
// browser pane's `browser_pick`.
function begin(id, kind, msg) {
  act(msg ?? { act: kind === "comment" ? "begin_comment" : "begin_review", pane: id }, r => {
    if (!r.ok) return;
    // A lost thread's draft is in its card in the thread list, which stays open.
    const el = $("p" + id), lost = el?.threads?.find(t => t.line === r.line)?.lost;
    listOpen = lost ? id : null;
    if (lost) el.lostOpen = r.line;
    draft = { pane: id, line: r.line, anchor: r.anchor, kind, hint: r.hint, busy: false, failed: false, text: r.text ?? "" };
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
  // A click in a page's text field, the address field or a dialog's field keeps the field's focus.
  if (document.activeElement === frameOf(f.id)) return;
  if (document.activeElement?.closest?.(`#p${f.id} :is(.head .url, .dlg input)`)) return;
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

// A page's frame: an HTML file pane's iframe, or a browser pane's view.
const frameOf = id => $("p" + id)?.querySelector(pane(id)?.kind === "browser" ? ":scope > .view" : ":scope > iframe");

// Scrolls the pane to its highlight; a page's frame scrolls itself.
function toHighlight(id) {
  // Page input gets no `acted` reply (`web.rs`), so `send`, not `act`.
  if (pane(id)?.kind === "browser") return send({ act: "browser_reveal", pane: id });
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
  const threads = m.threads.map(t => ({ line: t.line, n: t.notes.length, who: pinWho(t) }));
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
    if (pane(i)?.kind === "browser") send({ act: "browser_comment_mode", pane: i, on: i === commentMode });
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
  if (draft) return followFocus(true);
  if (m.laura === "line") {
    if (m.pin === true) return pinClick(id, m.line);
    if (layout.focus !== id) act({ act: "focus", pane: id });
    if (Number.isInteger(m.line)) act({ act: "set_cursor", pane: id, line: m.line });
    // A text field or a selection keeps the keystrokes; anything else gives them back to Laura.
    if (!m.editable) root.focus();
    paintHint();
    // Only in comment mode, so a page's script can't open a comment box.
    if (m.pick === true && commentMode === id && Number.isInteger(m.line)) begin(id, "comment");
  } else if (m.laura === "esc") {
    // The field takes `Esc` first: comment mode stays on until the next `Esc`.
    root.focus();
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
function pinClick(id, line) {
  const el = $("p" + id);
  if (!el || !Number.isInteger(line)) return;
  if (layout.focus !== id) act({ act: "focus", pane: id });
  root.focus();
  if (el.threads?.find(t => t.line === line)?.collapsed) return act({ act: "toggle_thread", pane: id, line });
  if (line === el.line) {
    el.shut = el.shut === line ? null : line;
    return paintThreads(id);
  }
  act({ act: "set_cursor", pane: id, line });
}

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
  // The address field: Enter opens the typed URL in its pane, Esc puts the pane's URL back.
  const url = document.activeElement?.matches?.(".head .url") && document.activeElement;
  if (url) {
    if (e.key === "Enter" && !e.isComposing) {
      take();
      if (url.value.trim()) act({ act: "browser_navigate", pane: +url.closest(".pane").id.slice(1), url: url.value });
      return root.focus();
    }
    if (e.key === "Escape") return take(), root.focus();
    if (ctrl && "rl".includes(k)) return take(); // the viewing browser's reload and address bar
    if (!(ctrl && "pq".includes(k))) return; // Ctrl+P and Ctrl+Q go on to Laura
  }
  if (e.key === "Escape" && (listOpen ?? moreOpen) !== null) {
    take();
    listOpen = moreOpen = null;
    return paintPops();
  }
  // As the hint line: `Esc` leaves comment mode from a file or browser pane, after the pane's own dialog or field.
  // The shell and an editor pane's editor view keep `Esc` for their program.
  const b = f?.kind === "browser" && $("p" + f.id);
  if (e.key === "Escape" && commentMode !== null && (f?.kind === "file" || b) && !b?.dialog && !b?.typing) return take(), setCommentMode(null);
  if (ctrl && k === "p") return take(), openSelector();
  if (ctrl && k === "q") {
    take();
    return act({ act: "quit" }, m => m.ok && ((confirmQuit = true), paintHint()));
  }
  if (ctrl && k === "l" && f && f.pty && f.id !== 0) {
    take();
    return act({ act: "toggle_editor_view", pane: f.id });
  }
  if (f?.kind === "browser") {
    const el = $("p" + f.id);
    // The page's dialog takes Enter and Esc; a prompt's field takes the rest.
    if (el.dialog) {
      if (e.key === "Enter" && !e.isComposing) return take(), answer(f.id, true);
      if (e.key === "Escape") return take(), answer(f.id, false);
      return;
    }
    // A field in the page has the keystrokes, all but Esc. Ctrl+V (Cmd+V) goes on as a paste.
    if (el.typing) {
      if (e.key === "Escape") return take(), (el.typing = false), send({ act: "browser_blur", pane: f.id }), paintHint();
      if ((ctrl || e.metaKey) && e.key.toLowerCase() === "v") return;
      return take(), toPage(f.id, e);
    }
    if (e.altKey && !e.ctrlKey && !e.metaKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
      return take(), act({ act: "browser_history", pane: f.id, delta: e.key === "ArrowLeft" ? -1 : 1 });
    }
    if (!mods(e) && SCROLL_KEYS.includes(e.key)) return take(), toPage(f.id, e);
    if (ctrl && k === "r") return take(), act({ act: "refresh", pane: f.id });
    if (ctrl && k === "l") return take(); // the browser's address bar
    const bare = !ctrl && !e.altKey && !e.metaKey;
    // `c` comments on the element under the pointer; off the page, it turns on comment mode.
    // Before `laura ready`, the pick returns the notice that asks for it.
    if (bare && e.key === "c") {
      take();
      // On a pin, `c` comments in the pin's thread.
      if (el.pinAt) return act({ act: "set_cursor", pane: f.id, line: el.pinAt }), begin(f.id, "comment");
      if (!el.at && layout.agent) return setCommentMode(f.id);
      const { x, y } = el.at ?? { x: -1, y: -1 };
      return begin(f.id, "comment", { act: "browser_pick", pane: f.id, x, y });
    }
    if (bare && e.key === "S") return take(), begin(f.id, "review");
    if (bare && ["x", "Escape", "n", "N", "r", "h"].includes(e.key)) return take(), act(FILE_KEYS[e.key](f.id));
    return;
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

// A browser pane's page takes a paste while a field has the keystrokes.
const typing = () => {
  const f = focused();
  return !locked && !draft && !overlayOpen() && f?.kind === "browser" && $("p" + f.id).typing && !$("p" + f.id).dialog && f.id;
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
  // A zoom changes the pixel ratio, not always the pane's size.
  layout?.panes.forEach(p => sizeView(p.id));
});
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
  // Laura holds the last view's sizes, and the old frame is no measure of lag: each browser pane
  // sends its size again, and its first frame adds no lag.
  for (const el of root.querySelectorAll(".pane.browser")) {
    delete el.querySelector(".view").dataset.key;
    el.first = true;
  }
  // Laura turned comment mode off for the new view; `send` is a no-op until the socket opens.
  if (commentMode !== null) setCommentMode(null);
  let laid = false, moved = false;
  const url = new URL("ws" + location.search, location);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  const sock = (ws = new WebSocket(url));
  sock.binaryType = "arraybuffer";
  sock.onopen = report;
  sock.onmessage = e => {
    if (typeof e.data !== "string") {
      const id = Number(new DataView(e.data).getBigUint64(0));
      if (pane(id)?.kind === "browser") return onFrame(id, e.data);
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
