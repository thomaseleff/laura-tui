// The pane selector (`Ctrl+P`) and the help (`Ctrl+H`).

import { $, draft, layout, on, setOverlay, ui } from "./state.js";
import { ic } from "./head.js";
import { focusPane, followFocus, steps, unseen } from "./panes.js";
import { kbds, paintHint } from "./bottom.js";
import { authors, avs } from "./review.js";
import { go, spaceLabel } from "./workspaces.js";
import { server, spaces } from "./main.js";

// The pane selector: panes whose name or path holds the query, or whose id starts with it. The id
// that equals the query comes first so that `Enter` picks it.
export const pq = $("pq"), veil = $("veil");

function matches() {
  const q = pq.value.trim().toLowerCase(), id = p => String(p.id) === q;
  return layout.panes.filter(p => p.name.toLowerCase().includes(q) || (p.path ?? "").toLowerCase().includes(q) || String(p.id).startsWith(q))
    .sort((a, b) => id(b) - id(a));
}

// A row of the pane selector's Workspaces section: another workspace, or the Laura server's page.
function spaceRow(w) {
  const b = document.createElement("button");
  b.className = "prow";
  b.dataset.url = w ? w.url : server;
  b.innerHTML = w ? '<span class="wdot"></span><span class="name"></span><span class="mute"></span>' : '<span class="pid">+</span><span class="name">New workspace…</span>';
  if (w?.waiting) {
    b.querySelector(".wdot").classList.add("w");
    b.insertAdjacentHTML("beforeend", '<span class="wait">Waiting for you</span>');
  }
  if (w) {
    b.querySelector(".name").textContent = spaceLabel(w);
    b.querySelector(".name").classList.toggle("fallback", !w.title);
    b.querySelector(".mute").textContent = w.root;
  }
  return b;
}

// Each row shows the header's chips. Static markup; the frame's strings go in as text. The panes
// come first, so `Ctrl+P` then `Enter` focuses a pane; then the other workspaces whose session
// label or folder holds the query, then **New workspace…** with a Laura server.
export function paintSelector() {
  const ps = matches(), q = pq.value.trim().toLowerCase();
  const others = spaces.filter(w => !w.current && `${spaceLabel(w)} ${w.root}`.toLowerCase().includes(q)).map(spaceRow);
  if (server && "new workspace".includes(q)) others.push(spaceRow());
  ui.row = Math.max(0, Math.min(ui.row, ps.length + others.length - 1));
  const head = text => {
    const h = document.createElement("div");
    h.className = "ph";
    h.textContent = text;
    return h;
  };
  const rows = ps.map(p => {
    const b = document.createElement("button");
    b.className = "prow";
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
    if (p.paused) chip("", "Paused");
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
  empty.textContent = "Nothing matches.";
  const all = [...rows, ...others];
  all[ui.row]?.classList.add("on");
  // The headings show only with a Workspaces section, so one workspace alone looks as before.
  const parts = others.length ? [...(rows.length ? [head("Panes"), ...rows] : []), head("Workspaces"), ...others] : rows;
  $("prows").replaceChildren(...(all.length ? parts : [empty]));
  all[ui.row]?.scrollIntoView({ block: "nearest" });
  paintHint();
}

export function openSelector() {
  pq.value = "";
  ui.row = 0;
  setOverlay("selector");
  paintSelector();
  paintOverlays();
  pq.focus();
  paintHint();
}

// Closes the pane selector, focusing pane `id` if given.
export function closeSelector(id) {
  setOverlay(null);
  paintOverlays();
  if (id !== undefined) focusPane(id);
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
  ui.row = 0;
  paintSelector();
});

// A pane's row focuses the pane. A workspace's row loads the workspace, in a new tab with `tab`.
export function pickRow(b, tab) {
  if (!b) return;
  if (!b.dataset.url) return closeSelector(+b.dataset.id);
  closeSelector();
  go(b.dataset.url, tab);
}

$("prows").addEventListener("click", e => pickRow(e.target.closest(".prow"), e.ctrlKey || e.metaKey));

$("findbtn").addEventListener("mousedown", e => e.preventDefault());

$("findbtn").addEventListener("click", () => (draft() ? followFocus(true) : openSelector()));

export function toggleHelp() {
  setOverlay(on("help") ? null : "help");
  paintOverlays();
}

// The help and the pane selector show while Laura holds them open.
export function paintOverlays() {
  $("help").hidden = !on("help");
  $("popup").hidden = !on("selector");
  veil.hidden = $("help").hidden && $("popup").hidden;
}

$("helpbtn").addEventListener("mousedown", e => e.preventDefault());

$("helpbtn").addEventListener("click", toggleHelp);

// The calm canvas's `?`, as the top bar's; the shell keeps the focus.
const canvasHelp = $("canvas").querySelector("[data-act=help]");
canvasHelp.addEventListener("mousedown", e => e.preventDefault());
canvasHelp.addEventListener("click", toggleHelp);

// A click outside the help sheet or the pane selector closes it.
veil.addEventListener("mousedown", e => {
  if (e.target !== veil) return;
  e.preventDefault();
  if (on("help")) toggleHelp();
  if (on("selector")) closeSelector();
});

// The help sheet: *Get started*'s steps, then a table of keystrokes per group.
export function paintHelp(groups, note) {
  const p = document.createElement("p");
  p.className = "mute sm note";
  p.textContent = note;
  const start = document.createElement("h3");
  start.textContent = "Get started";
  $("help").replaceChildren(start, steps(), ...groups.flatMap(([group, keys]) => {
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
