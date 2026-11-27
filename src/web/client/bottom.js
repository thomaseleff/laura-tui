// The workspace's bottom line and corner: the notice, the hint line, the status bar and the shell
// button.

import { $, draft, drawerOpen, focused, layout, modes, pane, send, ui } from "./state.js";
import { ch } from "./term.js";
import { focusPane, followFocus, lastFocus, unseen } from "./panes.js";
import { frameOf } from "./pages.js";
import { latestSay } from "./history.js";

const hintEl = $("hint"), noticeEl = $("notice");
let notice = "", noticeTimer = 0, noticePane;

// The notice shows a failed submit, which closing the draft takes down.
let noticeRetry = false;

// `retry`: the notice shows a failed submit.
export function showNotice(text, secs, pane, retry = false) {
  noticeRetry = retry;
  notice = text;
  noticePane = pane;
  clearTimeout(noticeTimer);
  noticeTimer = setTimeout(() => ((notice = ""), paintNotice()), secs * 1000);
  paintNotice();
}

// The notice keeps its text while it fades out. A notice for a pane shows in that pane, and any
// other in the workspace's corner, as does one whose pane closed or is too small.
export function paintNotice() {
  noticeEl.classList.toggle("on", !!notice);
  if (notice) noticeEl.textContent = notice;
  const el = noticePane !== undefined && $("p" + noticePane);
  // A closed drawer hides its notice, so the notice goes to the corner.
  const wide = el && el.offsetWidth >= 300 && !el.matches(".drawer:not(.open)");
  if (wide && noticeEl.parentNode !== el) el.appendChild(noticeEl);
  // The card fits below the pane header, else it goes to the workspace's corner.
  const fits = wide && noticeEl.offsetHeight <= el.offsetHeight - 3 * ch - 20;
  if (!fits && noticeEl.parentNode !== $("grid")) $("grid").appendChild(noticeEl);
}
// Closing the draft takes down the notice of a failed submit.
export function clearRetryNotice() {
  if (noticeRetry) (notice = ""), (noticeRetry = false), paintNotice();
}

export function paintHint() {
  const h = layout?.hints;
  if (!h) return;
  const f = focused(), b = f?.kind === "browser" && $("p" + f.id);
  // A page's frame with the keyboard is a field that has the keystrokes, as in a browser pane.
  const typing = b?.typing || (f && document.activeElement === frameOf(f.id));
  const here = document.activeElement?.matches?.(".head .url") ? h.browser_url
    : b?.dialog ? (b.dialog.type === "beforeunload" ? h.browser_leave : h.browser_dialog) : typing ? h.browser_typing : h.pane;
  // As the keydown listener: comment mode's hint only over a file or browser pane's own plain hint.
  const own = ui.commentMode !== null && (f?.kind === "file" || b) && here === h.pane ? h.comment_mode : here;
  // The first mode with a hint shows it, so under the help and the overlay the hint line shows the
  // mode below them; with no such mode, the hint line shows the pane's.
  // Laura's `mode` hint is its first mode's with a hint; until Laura's reply to a new mode, the pane's.
  const by = {
    quit: h.mode, locked: h.mode, draft: h.mode, menu: h.workspace,
    selector: $("prows").querySelector(".prow.on")?.dataset.url ? h.workspace : h.mode,
  };
  const m = modes().find(k => k in by), pairs = (m && by[m]) ?? own;
  const groups = (pairs ?? []).map(([k, label], i) => {
    const s = document.createElement("span");
    s.textContent = label;
    const sep = document.createElement("i");
    sep.className = "sep";
    // Before `laura ready`, `c` and `Shift+S` show dimmed; a hover gives the reason.
    if (!layout.agent && (k === "c" || k === "Shift+S")) {
      const o = document.createElement("span");
      o.className = "off";
      o.append(...kbds(k), s);
      return [...(i ? [sep] : []), o];
    }
    return [...(i ? [sep] : []), ...kbds(k), s];
  });
  hintEl.replaceChildren(...groups.flat());
  // Whole keystrokes only: drop the last ones that don't fit, such as beside the shell button.
  while (groups.length > 1 && hintEl.scrollWidth > hintEl.clientWidth) groups.pop().forEach(n => n.remove());
  // A repaint after `laura ready` takes away the dimmed keystroke under a shown tooltip.
  if (!hintEl.querySelector(".off")) $("htip").hidden = true;
}

// The shell button, the shell's name and the status bar change the hint line's width.
new ResizeObserver(() => paintHint()).observe(hintEl);

// A dimmed keystroke's reason. Not a `data-tip`: `#hint` clips its children, and `laura ready` shows as code.
hintEl.addEventListener("mouseover", e => {
  const o = e.target.closest?.(".off"), t = $("htip");
  t.hidden = !o;
  if (o) t.style.left = `${o.offsetLeft}px`;
});

hintEl.addEventListener("mouseleave", () => ($("htip").hidden = true));

// Focus back from a page's frame, such as a click on its pane header: the typing hint goes.
addEventListener("focus", () => {
  paintHint();
  visit();
});

// A visit to this workspace, which the workspace switcher and the Laura server list first.
export const visit = () => document.visibilityState === "visible" && send({ visit: true });

document.addEventListener("visibilitychange", visit);

// A keystroke's keys, split on spaces, as `<kbd>` chips.
export const kbds = k => k.split(" ").filter(Boolean).map(x => {
  const c = document.createElement("kbd");
  c.textContent = x;
  return c;
});

// The status bar: what waits on the user, in any pane. Names are the frame's, so text only.
export function paintStatus() {
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
    const d = draft()?.pane === p.id && draft().kind === "review" ? ui.box : null;
    const state = d?.busy ? "submitting" : d?.failed ? "submission failed" : d ? "review body open" : "in progress";
    return item(p, "unsubmitted inline review", d?.failed ? "bad" : "acc", `Unsubmitted inline review: ${state}`);
  };
  const items = [
    ...layout.panes.filter(p => p.threads).map(review),
    ...layout.panes.filter(p => p.unsaved).map(p => item(p, "unsaved Neovim edits", "warn", "Unsaved Neovim edits")),
  ];
  $("sbar").replaceChildren(...items);
  paintShellButton();
}

// The shell button's say folds to one line while the user works in a pane, so the card doesn't cover
// the pane (#275). `sayAt`: the time of the say the user last saw open, so a new say opens in full.
let folded = false, sayAt = null, hoverTimer = 0;
const fold = on => {
  clearTimeout(hoverTimer);
  if (folded !== on) (folded = on), paintShellButton();
};

// The shell button, while the drawer is in use: the shell's name, and `open` while the drawer is.
// With the drawer closed, and only while Laura's `said` holds, the agent's latest say: a long one
// grows upward as a card until the user folds it. While the shell waits with no say since the user
// last wrote to the shell, `Waiting for you` in its place.
export function paintShellButton() {
  const b = $("sbtn"), s = pane(0);
  b.hidden = !s?.drawer;
  if (!layout || b.hidden) return;
  // On the canvas, the drawer stays open: `Ctrl+\`` and the button do nothing.
  const open = drawerOpen(), t = layout.canvas ? s.name : `${open ? "Hide" : "Show"} the drawer  Ctrl+\``;
  const say = latestSay();
  if (say && say.at !== sayAt) {
    sayAt = say.at;
    folded = false;
  }
  const html = !open && layout.said && !layout.unsaid ? say?.html ?? null : null, said = b.querySelector(".said");
  if (said.dataset.html !== (html ?? "")) {
    said.innerHTML = html ?? "";
    said.dataset.html = html ?? "";
  }
  said.hidden = !html;
  b.classList.toggle("says", !!html);
  b.classList.toggle("long", !!html && !folded && (said.childElementCount > 1 || said.textContent.length > 70));
  b.classList.toggle("open", open);
  b.classList.toggle("wait", layout.unsaid);
  b.querySelector(".say").hidden = !layout.unsaid;
  b.querySelector("b").textContent = s.name;
  for (const k of b.querySelectorAll("kbd")) k.hidden = !!layout.canvas;
  b.dataset.tip = t;
  b.setAttribute("aria-label", layout.unsaid ? `${t} · Waiting for you` : html ? `${t} · ${said.textContent}` : t);
}

$("sbtn").addEventListener("mousedown", e => e.preventDefault());

// A mouse over the shell button opens the say after a moment, so a mouse that passes over the bottom
// line doesn't flash the card; keyboard focus opens it at once, and `Esc` or leaving it folds it
// (WCAG 1.4.13).
$("sbtn").addEventListener("mouseenter", () => (clearTimeout(hoverTimer), (hoverTimer = setTimeout(() => fold(false), 400))));
$("sbtn").addEventListener("mouseleave", () => fold(true));
$("sbtn").addEventListener("focus", () => fold(false));
$("sbtn").addEventListener("blur", () => fold(true));
$("sbtn").addEventListener("keydown", e => {
  if (e.key !== "Escape" || folded) return;
  e.preventDefault();
  e.stopPropagation();
  fold(true);
});
// The first click or keystroke in a pane folds the say.
addEventListener("pointerdown", e => e.target.closest?.(".pane") && fold(true), true);
addEventListener("keydown", e => e.target !== $("sbtn") && fold(true), true);
// A page's frame sends its click as a message, which the window's `pointerdown` doesn't see.
export const foldSay = () => fold(true);

$("sbtn").addEventListener("click", () => (draft() ? followFocus(true) : toggleShell()));

// The dot pulses for a second after each of the shell's frames.
export function shellBusy() {
  const b = $("sbtn");
  b.classList.add("busy");
  clearTimeout(b.busyTimer);
  b.busyTimer = setTimeout(() => b.classList.remove("busy"), 1000);
}

// `Ctrl+\`` and the shell button: from the shell, back to the last pane that had the focus, or the
// first other pane when that one closed; from any other pane, to the shell, which opens the drawer.
export function toggleShell() {
  if (layout.focus !== 0) return focusPane(0);
  const to = pane(lastFocus) ? lastFocus : layout.panes.find(p => p.id !== 0)?.id;
  if (to !== undefined) focusPane(to);
}

$("sbar").addEventListener("mousedown", e => e.preventDefault());

$("sbar").addEventListener("click", e => {
  const b = e.target.closest("[data-id]");
  if (!b) return;
  if (draft()) return followFocus(true);
  focusPane(+b.dataset.id);
});

// Laura reloaded the pane's file: **Reloaded** for a few seconds, and the unseen marker unless
// the user is looking at the pane.
export function reloaded(id) {
  const el = $("p" + id), chip = el?.querySelector(".head .reloaded");
  if (!chip) return;
  chip.hidden = false;
  clearTimeout(el.reloadTimer);
  el.reloadTimer = setTimeout(() => (chip.hidden = true), 4000);
  if (id === layout.focus) return;
  unseen.add(id);
  el.classList.add("unseen");
}
