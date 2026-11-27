// The History in the drawer: the rows of the `history` frame, and their clicks. A click on a command's
// row asks Laura to show the command again (`Tab::replay`). A say is Laura's HTML of the agent's
// markdown; a file path in it opens the file, as the shell's links do.

import { $, act, esc } from "./state.js";
import { PATH } from "./term.js";
import { paintShellButton } from "./bottom.js";

// The History's rows: a `history` frame's rows from its `from` on, after the rows before it.
let rows = [];
// A hidden list doesn't scroll: focusHistory scrolls it once the History shows.
let unread = false;
// A frame came before: the says of the page's first frame are old, and a screen reader doesn't read them.
let heard = false;

export function onHistory(m) {
  const before = latestSay()?.at;
  rows = m.from ? rows.slice(0, m.from).concat(m.rows) : m.rows;
  const say = latestSay();
  if (heard && say && say.at !== before) {
    const text = document.createElement("div");
    text.innerHTML = say.html;
    $("saylive").textContent = text.textContent;
  }
  heard = true;
  paintHistory(undefined, m.from);
  paintShellButton();
}

// The session's latest say, `{at, html}` with Laura's HTML, for the shell button; null before a say.
export const latestSay = () => rows.findLast(r => r.kind === "say") ?? null;

// The row's time, as the user's clock shows it.
const time = at => new Date(at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

// A row of the timeline: the time, a dot on the rail, the verb, then what it acted on. Only a row that
// a click shows again (`replay`, Laura's rule) is blue, with a chevron and its index.
const line = (r, i, cls, verb, rest) => `<div class="te hr ${cls}"${r.replay ? ` data-i="${i}" role="button" tabindex="0"` : ""}>`
  + `<span class="tm">${time(r.at)}</span><i class="pt"></i><span class="verb">${verb}</span>${rest}</div>`;

function row(r, i) {
  switch (r.kind) {
    case "ready": return line(r, i, "ro", "ready", `<span class="tg">${esc(r.title ?? "")}</span>`);
    case "command": return line(r, i, "cmd", esc(r.verb), `<span class="tg">${esc(r.target)}</span>${r.replay ? '<span class="chev">›</span>' : ""}`);
    // The say's HTML is Laura's, as in a `pane` frame.
    case "say": return line(r, i, "say", "say", `<div class="body md">${r.html}</div>`);
    case "review": return line(r, i, "review", "review", `<span class="tg">${esc(r.path)} · ${r.comments} comment${r.comments === 1 ? "" : "s"}</span>`);
  }
  return "";
}

// Each file path in the says' text, as a link that opens the file at its line.
function linkPaths(list) {
  const walk = document.createTreeWalker(list, NodeFilter.SHOW_TEXT, {
    acceptNode: n => n.parentElement.closest(".te.say .body") && !n.parentElement.closest("a") ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT,
  });
  const texts = [];
  while (walk.nextNode()) texts.push(walk.currentNode);
  for (const t of texts) {
    const urls = [...t.data.matchAll(/[a-z][\w+.-]*:\/\/\S+/gi)].map(u => [u.index, u.index + u[0].length]);
    const hits = [...t.data.matchAll(PATH)].filter(m => /[\/\\]/.test(m[0]) && !urls.some(([a, b]) => m.index >= a && m.index < b));
    if (!hits.length) continue;
    const frag = document.createDocumentFragment();
    let at = 0;
    for (const m of hits) {
      frag.append(t.data.slice(at, m.index));
      const [, path, n] = m[0].match(/^(.*?)(?::(\d+))?$/);
      const a = document.createElement("a");
      a.className = "path";
      a.textContent = m[0];
      a.dataset.path = path;
      if (n) a.dataset.line = n;
      frag.append(a);
      at = m.index + m[0].length;
    }
    frag.append(t.data.slice(at));
    t.replaceWith(frag);
  }
}

// `list`: the new shell pane's, before its element joins the page. `from`: the first new row, when
// the list holds the rows before it; the append keeps a focused row focused.
export function paintHistory(list = $("p0")?.querySelector(".history .list"), from = 0) {
  if (!list) return;
  const grew = list.childElementCount !== rows.length;
  if (from && list.childElementCount === from) {
    const t = document.createElement("template");
    t.innerHTML = rows.slice(from).map((r, j) => row(r, from + j)).join("");
    linkPaths(t.content);
    list.append(t.content);
  } else {
    list.innerHTML = rows.map(row).join("");
    linkPaths(list);
  }
  if (grew) {
    list.scrollTop = list.scrollHeight;
    unread = !list.checkVisibility();
  }
}

// The History's list takes the DOM focus while the drawer shows it.
export function focusHistory() {
  const list = $("p0")?.querySelector(".history .list");
  if (unread && list) {
    list.scrollTop = list.scrollHeight;
    unread = false;
  }
  list?.focus({ preventScroll: true });
}

export function historyClick(e) {
  const a = e.target.closest("a.path"), r = e.target.closest(".te.hr[data-i]");
  if (a) return act({ act: "open_path", pane: 0, path: a.dataset.path, line: a.dataset.line ? +a.dataset.line : undefined });
  // A markdown link in a say opens its file at its heading, as on the glance.
  const l = e.target.closest("a[data-href]");
  if (l) return act({ act: "open_link", pane: 0, href: l.dataset.href });
  if (r) act({ act: "replay", pane: 0, row: +r.dataset.i });
}
