// Threads and the inline review: each thread's card, the comment's box, the pane's popovers (the
// review body, the thread list and **More**), and the draft from `c` or `Shift+S` to its save or
// submit.

import { $, act, esc, layout, pane, send, ui } from "./state.js";
import { headClick, ic, tip, TOOLS } from "./head.js";
import { focusPane, followFocus } from "./panes.js";
import { clearRetryNotice, paintHint, paintStatus } from "./bottom.js";
import { frameOf } from "./pages.js";
import { strings } from "./main.js";

// The comment's box (`ui.draft`).
let draftCard = null;

// A comment's author: `null` is the user. The agent's names read as in the TUI.
export const who = a => (a === null ? "You" : a === "agent" ? "Agent" : a);

const av = a => `<span class="av${a === null ? "" : " agent"}">${esc(([...who(a)][0] ?? "?").toUpperCase())}</span>`;

export const avs = list => `<span class="avs">${[...new Set(list)].map(av).join("")}</span>`;

export const authors = t => t.notes.map(n => n.author);

// A thread's chip: a browser pane's element, cut from the left to `n` characters, else its line.
const chip = (anchor, line, n) => esc(!anchor ? "L" + line : anchor.length > n ? "…" + anchor.slice(1 - n) : anchor);

const HOLD = "Laura holds the agent’s commands while you type.";

// The rows' units (rows and blocks), without the threads between them.
export const units = rows => rows.querySelectorAll(":scope > [data-row]");

export const draftBox = () => document.querySelector(".draft textarea, .pop.review textarea");

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
  box.querySelector("textarea").addEventListener("input", e => ui.draft && (ui.draft.text = e.target.value));
  box.addEventListener("mousedown", e => e.target.closest("button") && e.preventDefault());
}

// `reply`: the thread on the draft's line, if any. `paintThreads` puts the box in that thread's card
// (`cardHTML`'s slot) or gives the box a card of its own.
function makeDraftCard(reply) {
  const c = document.createElement("div"), edit = reply?.notes.at(-1)?.author === null;
  const verb = edit ? "Edit" : reply ? "Reply" : "Comment";
  const lbl = [reply && verb, reply?.label ?? ui.draft.anchor?.[1]].filter(Boolean).join(" · ");
  const at = chip(ui.draft.anchor?.[0], ui.draft.line, 40);
  c.innerHTML = `<div class="ch"><span class="anc">${at}</span>${lbl ? `<span class="lbl">${esc(lbl)}</span>` : ""}</div>`
    + `<textarea spellcheck="false" placeholder="${edit ? "Edit your comment" : `${verb} on ${at}`}"></textarea>`
    + `<div class="holdnote">${HOLD}</div><div class="fa"><button class="btn" data-act="cancel">Cancel <kbd>Esc</kbd></button>`
    + `<button class="btn pri" data-act="save">${edit ? "Save" : verb} <kbd>Enter</kbd></button></div>`;
  c.querySelector("textarea").value = ui.draft.text;
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
export function keepDraft(fn) {
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
export function paintThreads(id) {
  const el = $("p" + id), rows = el?.querySelector(".rows"), p = pane(id);
  if (!rows || !p) return;
  const f = frameOf(id), all = el.threads ?? [];
  const d = ui.draft?.pane === id && ui.draft.kind === "comment" ? ui.draft : null;
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
  // A draft whose element the page lost, also mid-draft, goes in its thread's card in the thread list.
  const lost = d && all.find(t => t.line === d.line && t.lost);
  if (lost) (ui.listOpen = id), (el.lostOpen = lost.line);
  const host = d && (ts.find(t => t.line === d.line) ?? lost);
  for (const t of ts) put(t.line, cardHTML(p, t, t.line === el.card && layout.focus === id, t === host));
  if (d) {
    // Laura holds the agent's commands while the user types, so the line's thread can't change mid-draft.
    draftCard ??= makeDraftCard(all.find(t => t.line === d.line));
    // The thread's card can go mid-draft (collapsed, lost, back on the page): the box takes or drops a card of its own.
    draftCard.className = host ? "draft" : "card draft on";
    if (!host) put(d.line + 0.5, '<i class="slot"></i>');
  }
  // The thread's line is tinted, not a removed-lines row below it; a thread that wasn't there before eases in.
  const anchored = new Set(ts.map(t => f ? null : us.findLast(u => +u.dataset.line <= t.line && !u.classList.contains("gap")) ?? null));
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
  const d = ui.draft?.pane === id && ui.draft.kind === "review" ? ui.draft : null;
  const sb = el.querySelector(".head .submit");
  sb.innerHTML = d?.busy ? "Submitting…" : d?.failed ? 'Retry<span class="tl"> submit</span>' : 'Submit<span class="tl"> review</span>';
  sb.classList.toggle("submitting", !!d?.busy);
  sb.classList.toggle("failed", !!d?.failed);
  el.querySelector(".head .sfail").hidden = !d?.failed;
  // A lost thread's card closes with the list, or when the page shows the thread's element again.
  if (ui.listOpen !== id || !el.threads?.some(t => t.line === el.lostOpen && t.lost)) el.lostOpen = null;
  const kind = d ? "review" : ui.listOpen === id && p.threads ? "list" : ui.moreOpen === id ? "more" : "";
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
      wrap.querySelector("textarea").value = ui.draft.text;
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
    q(".err").hidden = !ui.draft.failed;
    q(".err").textContent = strings.draft?.failed ?? "";
    q("[data-act=save]").innerHTML = `${ui.draft.busy ? "Submitting…" : ui.draft.failed ? "Retry" : "Submit review"} <kbd>Enter</kbd>`;
    q("[data-act=save]").disabled = ui.draft.busy;
    // The review body can't change while Laura submits it.
    q("textarea").readOnly = ui.draft.busy;
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
      + shown.map(b => row(b.dataset.act, b.dataset.ic ?? TOOLS.find(t => t[0] === b.dataset.act)[1], b.dataset.tip)).join("");
    // **More** is wider than a narrow pane; it stays inside the window.
    const pop = q(".pop");
    pop.style.right = "";
    const x = pop.getBoundingClientRect().left;
    if (x < 8) pop.style.right = `${x}px`;
    return;
  }
  // A lost thread's card, and its draft, open below its row (`el.lostOpen`).
  const ts = el.threads ?? [], dl = ui.draft?.pane === id && ui.draft.kind === "comment" ? ui.draft.line : null;
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

export const paintPops = () => layout?.panes.forEach(p => paintPop(p.id));

function popClick(id, e) {
  const b = e.target.closest("button");
  // **More**'s rows are the header's buttons.
  if (b?.classList.contains("mrow")) return headClick(id, e);
  e.stopPropagation();
  const el = $("p" + id), c = b && e.target.closest(".card[data-th]");
  // While a draft is open, a row or a card doesn't move the cursor or close the draft's card, as on the page.
  // headClick and cardClick use the same guard, so the only list open during a draft is the draft's own.
  if (ui.draft && (c || b?.dataset.th)) return followFocus(true);
  // A lost thread's card in the list: **–** closes it; **Reply…**, **Edit…** and **Delete** act as on the page.
  if (c && b.dataset.act === "collapse") return (el.lostOpen = null), paintPop(id);
  if (c) return cardClick(id, e);
  if (b?.dataset.th && el.threads?.find(t => t.line === +b.dataset.th)?.lost) {
    el.lostOpen = el.lostOpen === +b.dataset.th ? null : +b.dataset.th;
    return paintPop(id);
  }
  if (b?.dataset.th) {
    ui.listOpen = null;
    paintPop(id);
    focusPane(id);
    // The cursor may already be on the thread's line and send no frame, so the reply scrolls too.
    const line = +b.dataset.th;
    el.goTh = { th: b.dataset.th, block: "center" };
    // Comment mode hid the card of the cursor's thread (`el.shut`): the click shows it again.
    if (el.shut === line) (el.shut = null), paintThreads(id);
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
export function goThread(id) {
  const el = $("p" + id), go = el?.goTh;
  const card = go && el.querySelector(`.card[data-th="${go.th}"]`);
  if (!card) return;
  el.goTh = null;
  reveal(card, go.block);
}

// Below the workspace's minimum size the window scrolls (style.css); a pane's scroll leaves it
// where the user put it.
export function reveal(el, block) {
  const { scrollX, scrollY } = window;
  el?.scrollIntoView({ block });
  scrollTo(scrollX, scrollY);
}

// A click outside the thread list or **More** closes it.
addEventListener("mousedown", e => {
  if (ui.listOpen !== null && !e.target.closest(".pop.list, [data-act=list]") && !draftCard?.closest(".pop.list")) {
    const id = ui.listOpen;
    ui.listOpen = null;
    paintPop(id);
  }
  if (ui.moreOpen !== null && !e.target.closest(".pop.more, [data-act=more]")) {
    const id = ui.moreOpen;
    ui.moreOpen = null;
    paintPop(id);
  }
}, true);

// A thread's collapse button or a collapsed thread toggles that thread alone, even on a block's
// first line, where `r` toggles the block's threads. **Reply…** comments on the thread's line, as `c`;
// **Delete** saves the user's last comment empty, which deletes it.
export function cardClick(id, e) {
  const c = e.target.closest(".card[data-th]"), b = e.target.closest("[data-act]");
  if (!c || !b) return;
  e.stopPropagation();
  if (ui.draft) return followFocus(true);
  focusPane(id);
  if (b.dataset.act === "delete") return act({ act: "comment", pane: id, line: +c.dataset.th, text: "" });
  if (b.dataset.act !== "reply") return act({ act: "toggle_thread", pane: id, line: +c.dataset.th });
  act({ act: "set_cursor", pane: id, line: +c.dataset.th });
  begin(id, "comment");
}

// `msg`: the act that starts the draft, when not `begin_comment` or `begin_review`, such as a
// browser pane's `browser_pick`.
export function begin(id, kind, msg) {
  act(msg ?? { act: kind === "comment" ? "begin_comment" : "begin_review", pane: id }, r => {
    if (!r.ok) return;
    ui.listOpen = null;
    ui.draft = { pane: id, line: r.line, anchor: r.anchor, kind, hint: r.hint, busy: false, failed: false, text: r.text ?? "" };
    paintThreads(id);
    paintPops();
    paintHint();
    paintStatus();
    followFocus(true);
  });
}

// `Enter`: add or save the comment, or submit the inline review. A failure keeps the draft for a retry.
export function confirmDraft() {
  const d = ui.draft;
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
      if (ui.draft !== d) return;
      d.busy = false;
      d.failed = true;
      paintPop(d.pane);
      paintStatus();
    });
}

export function closeDraft() {
  if (!ui.draft) return;
  const id = ui.draft.pane;
  ui.draft = null;
  draftCard?.remove();
  draftCard = null;
  send({ hold: false });
  clearRetryNotice();
  paintThreads(id);
  paintHint();
  paintStatus();
  followFocus(true);
}
