// The History in the drawer: the rows of the `history` frame, which Laura folds, and their clicks. A say is Laura's HTML of the agent's markdown; a file path in it opens the file, as
// the shell's links do.

import { $, act, esc, pane } from "./state.js";
import { PATH } from "./term.js";
import { ic } from "./head.js";
import { focusPane } from "./panes.js";
import { paintShellButton } from "./bottom.js";

// The rows of the last `history` frame.
let rows = [];
// A hidden list doesn't scroll: focusHistory scrolls it once the History shows.
let unread = false;

export function onHistory(m) {
  rows = m.rows;
  paintHistory();
  paintShellButton();
}

// The session's latest say, as Laura's HTML, for the shell button; null before a say.
export const latestSay = () => rows.findLast(r => r.kind === "say")?.html ?? null;

const name = path => path.split(/[\/\\]/).pop();

function row(r) {
  switch (r.kind) {
    // The say's HTML is Laura's, as in a `pane` frame.
    case "say": return `<div class="te say"><div class="md">${r.html}</div></div>`;
    case "commands": return `<button class="te cmd" data-pane="${r.pane ?? ""}">${ic("target")}<span>${esc(r.text)}</span><span class="chev">›</span></button>`;
    case "review": {
      const n = `${r.comments} thread${r.comments === 1 ? "" : "s"}`;
      return `<div class="te mine"><div><div class="who">Inline review · ${esc(name(r.path))} · ${n}</div>`
        + (r.body ? `<div class="bub"><p>${esc(r.body)}</p></div>` : "") + "</div></div>";
    }
  }
  return "";
}

// Each file path in the says' text, as a link that opens the file at its line.
function linkPaths(list) {
  const walk = document.createTreeWalker(list, NodeFilter.SHOW_TEXT, {
    acceptNode: n => n.parentElement.closest(".te.say") && !n.parentElement.closest("a") ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT,
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

// `list`: the new shell pane's, before its element joins the page.
export function paintHistory(list = $("p0")?.querySelector(".history .list")) {
  if (!list) return;
  const grew = list.childElementCount !== rows.length;
  list.innerHTML = rows.map(row).join("");
  linkPaths(list);
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
  const a = e.target.closest("a.path"), c = e.target.closest(".te.cmd");
  if (a) return act({ act: "open_path", pane: 0, path: a.dataset.path, line: a.dataset.line ? +a.dataset.line : undefined });
  // A markdown link in a say opens its file at its heading, as on the glance.
  const l = e.target.closest("a[data-href]");
  if (l) return act({ act: "open_link", pane: 0, href: l.dataset.href });
  // The run's pane may have closed since.
  if (c && pane(+c.dataset.pane)) focusPane(+c.dataset.pane);
}
