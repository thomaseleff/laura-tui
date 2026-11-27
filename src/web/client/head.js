// The pane header: its icons, its buttons and chips, and its fit in a narrow pane.

import { $, act, draft, layout, pane, ui } from "./state.js";
import { focusPane, followFocus } from "./panes.js";
import { begin, confirmDraft, paintPops } from "./review.js";
import { setCommentMode, toHighlight } from "./pages.js";
import { strings } from "./main.js";

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
  pause: '<path d="M9 5v14M15 5v14"/>',
  play: '<path d="M7 4.5v15l12-7.5z"/>',
  dock: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16"/>',
  drawer: '<rect x="3" y="4" width="18" height="16" rx="2"/><rect x="7" y="12" width="10" height="5" rx="1"/>',
};

export const ic = n => `<svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${I[n]}</svg>`;

// A tooltip that shows at once; `aria-label` reads the same text to a screen reader.
export const tip = t => `data-tip="${t}" aria-label="${t}"`;

// The header's tools, which a narrow pane moves into **More**: action, icon, tooltip.
export const TOOLS = [
  ["comment", "comment", "Comment on the cursor  c"],
  ["cm", "pick", "Comment mode: click an element to comment"],
  ["toggle_pause", "pause", "Pause  p"],
  ["toggle_diff", "diff", "Diff against git HEAD  d"],
  ["refresh", "refresh", "Refresh: discard the unsubmitted inline review and reload  Ctrl+R"],
  ["close", "close", "Close the pane  x"],
];

// Static markup; `paintHead` sets the frame's strings as text. `.tl` is a label that a narrow
// pane hides.
export const head = () => `<div class="head"><span class="pid"></span>`
  + `<span class="navb"><button class="tool" data-act="back" ${tip("Back  Alt+←")}>${ic("back")}</button><button class="tool" data-act="fwd" ${tip("Forward  Alt+→")}>${ic("fwd")}</button></span>`
  + `${ic("file")}<span class="name tip-l"><i class="dot"></i><span class="nm"></span></span><input class="url" spellcheck="false" aria-label="Address">`
  + '<span class="delta"><span class="add"></span><span class="del"></span></span>'
  + `<span class="seg tip-c" ${tip("Flip the view  Ctrl+L")}><button data-act="editor">E<span class="tl">ditor</span></button><button data-act="file">F<span class="tl">ile</span></button></span>`
  + `<span class="seg dview tip-c" ${tip("Flip the drawer  Ctrl+.")}><button data-act="history">History</button><button data-act="shell">Shell</button></span>`
  + '<span class="chips"><span class="chip warn unsaved">● Unsaved</span><span class="chip ok reloaded" hidden>Reloaded</span>'
  + '<span class="chip paused">Paused</span><span class="chip warn frozen">Frozen</span><span class="chip diffc">Diff view</span>'
  + `<button class="chip hlc tip-l" data-act="highlight" data-tip="Scroll to the highlight">${ic("target")}<span class="l"></span>`
  + `<span class="x" data-act="clear_highlight" ${tip("Clear the highlight  h")}>×</span></button>`
  + '<span class="chip bad sfail" hidden>Submission failed</span>'
  + `<span class="chip new agent tip-c" ${tip("The agent's browser tool clicks or types on this page")} hidden>Agent</span>`
  + `<span class="chip new shot tip-c" ${tip("The agent's browser tool took a screenshot of this page")} hidden>Screenshot</span>`
  + `<span class="chip new rec tip-c" ${tip("The agent's browser tool records this page")} hidden>Recording</span>`
  + `<span class="chip emu tip-c" ${tip("The agent's browser tools set the page to this size")} hidden></span></span>`
  + '<i class="cdot" role="img"></i><span class="grow"></span><span class="spin" hidden></span><span class="live tip-c" hidden></span>'
  + `<button class="threads" data-act="list" ${tip("All threads  n / N")}><span class="avs"></span><span class="tn"></span><span class="tl"></span></button>`
  + `<button class="submit" data-act="review" ${tip("Submit the inline review  Shift+S")}></button>`
  + `<button class="tool" data-act="dock" ${tip("Show the shell as a pane")}>${ic("dock")}</button>`
  + `<button class="tool" data-act="drawer" ${tip("Show the shell in a drawer")}>${ic("drawer")}</button>`
  + `<span class="tools">${TOOLS.map(([a, i, t]) => `<button class="tool" data-act="${a}" ${tip(t)}>${ic(i)}</button>`).join("")}</span>`
  + `<button class="tool more" data-act="more" ${tip("More")}>${ic("more")}</button></div>`;

// A file pane's lines, or a browser pane's selector.
const hlLabel = h => (typeof h === "string" ? `Highlight ${h}` : `Highlight L${h[0]}${h[1] !== h[0] ? "–" + h[1] : ""}`);

// A narrow header folds its chips into one dot, which names them in its tooltip. The chips change
// the header's width, so the header fits again (the header's `MutationObserver` in `makePane`).
export function paintChipDot(el) {
  const names = [...el.querySelectorAll(".head .chips > :not([hidden])")].map(c => (c.querySelector(".l") ?? c).textContent);
  const d = el.querySelector(".head .cdot");
  d.hidden = !names.length;
  d.dataset.tip = names.join("\n");
  d.setAttribute("aria-label", names.join(", "));
  fitHead(el);
}

// A narrow pane's header drops its parts in steps until it fits (CSS keys each step on `data-fit`).
// A name may shorten to 120px before a part drops.
export function fitHead(el) {
  const h = el.querySelector(".head"), nm = h.querySelector(".nm");
  for (let n = 0; n <= 5; n++) {
    el.dataset.fit = n;
    if (h.scrollWidth <= h.clientWidth && nm.clientWidth >= Math.min(nm.scrollWidth, 120)) break;
  }
}

// The line shows as a ⚠ when it overflows its box (CSS keys on `data-clip`); a name's length
// decides that, so no container size can. A Range, since `.narrow` centers its text and text
// that overflows at the top doesn't count in `scrollHeight`.
export function fitNarrow(el) {
  const nw = el.querySelector(".narrow"), r = document.createRange();
  el.removeAttribute("data-clip");
  r.selectNodeContents(nw);
  el.toggleAttribute("data-clip", r.getBoundingClientRect().height > nw.clientHeight);
}

// The "too small" line shows only while the pane is too small (style.css).
export const tooSmall = el => el.querySelector(".narrow").offsetHeight > 0;

export function paintHead(el, p) {
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
  show(".dview", p.id === 0);
  q(".dview [data-act=history]").classList.toggle("on", layout.drawer_view === "history");
  q(".dview [data-act=shell]").classList.toggle("on", layout.drawer_view !== "history");
  q("[data-act=editor]").classList.toggle("on", p.kind === "editor");
  q("[data-act=file]").classList.toggle("on", p.kind !== "editor");
  show(".unsaved", p.unsaved);
  show(".paused", p.paused);
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
  show(".shot", browser && !!el.shot);
  show(".rec", browser && !!el.recording);
  show(".more", p.id !== 0);
  show("[data-act=dock]", p.drawer);
  // In the web app, the shell leaves the drawer only when the user docks it, also over the canvas.
  show("[data-act=drawer]", p.id === 0 && !p.drawer);
  // An editor pane has no diff view (NO_DIFF_IN_EDITOR), and nor does a file with no changes.
  show(".tools [data-act=toggle_diff]", !p.pty && (p.added || p.removed || p.diff));
  q(".tools [data-act=toggle_diff]").classList.toggle("on", p.diff);
  // `paused` is null on a pane that can't pause. A paused pane shows **Resume**.
  const pb = q(".tools [data-act=toggle_pause]"), pt = p.paused ? "Resume  p" : "Pause  p";
  pb.hidden = p.paused == null;
  pb.classList.toggle("on", !!p.paused);
  pb.innerHTML = ic(pb.dataset.ic = p.paused ? "play" : "pause");
  pb.dataset.tip = pt;
  pb.setAttribute("aria-label", pt);
  // A pane with no room for the line shows only a ⚠, whose tooltip reads the line. The fit
  // forces a layout, and this runs on each frame.
  const nw = el.querySelector(".narrow"), nt = strings.narrow?.replace("{name}", p.name) ?? "";
  if (nw.textContent !== nt) {
    nw.textContent = nw.dataset.tip = nt;
    nw.setAttribute("aria-label", nt);
    fitNarrow(el);
  }
}

// A header button does what its keystroke does.
export function headClick(id, e) {
  const b = e.target.closest("[data-act]");
  if (!b) return;
  e.stopPropagation();
  // **Retry submit** retries, as the review body's **Retry** does.
  if (b.dataset.act === "review" && draft()?.kind === "review" && draft().pane === id && ui.box?.failed) return confirmDraft();
  if (draft()) return followFocus(true);
  const a = b.dataset.act;
  if (a === "more") {
    ui.moreOpen = ui.moreOpen === id ? null : id;
    ui.listOpen = null;
    return paintPops();
  }
  if (ui.moreOpen !== null) (ui.moreOpen = null), paintPops();
  // Submit and the thread list work on the file view, as `Shift+S` and `n`/`N` do.
  if ((a === "review" || a === "list") && pane(id)?.kind === "editor") act({ act: "toggle_editor_view", pane: id });
  if (a === "highlight") return toHighlight(id);
  if (a === "list") {
    ui.listOpen = ui.listOpen === id ? null : id;
    return paintPops();
  }
  // The drawer's tabs: the one that isn't on flips the drawer.
  if (a === "history" || a === "shell") {
    if (layout.drawer_view !== a) act({ act: "flip_drawer" });
    return;
  }
  if (a === "editor" || a === "file") {
    if ((pane(id)?.kind === "editor") !== (a === "editor")) act({ act: "toggle_editor_view", pane: id });
    return;
  }
  if (a !== "close") focusPane(id);
  if (a === "back" || a === "fwd") return act({ act: "browser_history", pane: id, delta: a === "back" ? -1 : 1 });
  if (a === "cm") return setCommentMode(ui.commentMode === id ? null : id);
  if (a === "comment" || a === "review") begin(id, a);
  else act({ act: a, pane: id });
}
