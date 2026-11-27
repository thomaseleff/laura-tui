// The chat in the drawer: the rows of the `chat` frame, which Laura folds, their clicks, and the
// message box. A say is Laura's HTML of the agent's markdown; a file path in it opens the file, as
// the shell's links do.

import { $, act, esc, layout, pane } from "./state.js";
import { PATH } from "./term.js";
import { ic } from "./head.js";
import { focusPane } from "./panes.js";
import { paintShellButton } from "./bottom.js";

// The rows of the last `chat` frame.
let rows = [];

export function onChat(m) {
  rows = m.rows;
  paintChat();
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
    case "message": return `<div class="te mine"><div><div class="bub"><p>${esc(r.body)}</p></div></div></div>`;
    case "wait": return `<div class="te warn${r.done ? " done" : ""}"><span>The shell waits for you.</span>`
      + (r.done ? "" : '<button class="btn" data-act="flip_drawer">Show the shell <kbd>Ctrl+.</kbd></button>') + "</div>";
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
export function paintChat(list = $("p0")?.querySelector(".chat .list")) {
  if (!list) return;
  const grew = list.childElementCount !== rows.length;
  list.innerHTML = rows.map(row).join("");
  linkPaths(list);
  if (grew) list.scrollTop = list.scrollHeight;
}

// The chat takes the DOM focus while the drawer shows it: the message box, unless Laura turned it off.
export function focusChat() {
  const box = msgBox();
  (box && !box.disabled ? box : $("p0")?.querySelector(".chat .list"))?.focus({ preventScroll: true });
}

const msgBox = () => $("p0")?.querySelector(".msgbox textarea");

// Laura turns the message box off while the shell waits with no say (`Tab::message_gate`): the
// agent CLI may ask a question there, which the message's `Enter` would answer.
export function paintMsgBox() {
  const box = msgBox();
  if (!box) return;
  box.disabled = !layout.message_box;
  box.placeholder = layout.message_box ? "Reply" : `${pane(0)?.name ?? "The agent"} waits for you in the shell`;
  box.closest(".msgbox").querySelector("button").disabled = box.disabled;
}

// `Enter` or **Send**: the message goes to the shell, and the box empties once Laura wrote it.
function sendMessage() {
  const box = msgBox(), text = box.value;
  if (!text.trim() || box.disabled) return;
  act({ act: "message", pane: 0, text }, m => m.ok && box.value === text && (box.value = ""));
}

// The message box's keystrokes: `Enter` sends, `Shift+Enter` adds a line, and the others type.
export function msgKey(e) {
  if (e.key !== "Enter" || e.shiftKey || e.isComposing) return;
  e.preventDefault();
  e.stopPropagation();
  sendMessage();
}

export function chatClick(e) {
  if (e.target.closest("[data-act=send]")) return sendMessage();
  const a = e.target.closest("a.path"), c = e.target.closest(".te.cmd"), b = e.target.closest("[data-act=flip_drawer]");
  if (a) return act({ act: "open_path", pane: 0, path: a.dataset.path, line: a.dataset.line ? +a.dataset.line : undefined });
  // The run's pane may have closed since.
  if (c && pane(+c.dataset.pane)) return focusPane(+c.dataset.pane);
  if (b) act({ act: "flip_drawer" });
}
