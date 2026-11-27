// The other workspaces: the workspace switcher and its menu, the browser tab's title and icon, a
// visit to another workspace, and the overlay when this view ends.

import { $, draft, layout, on, ui } from "./state.js";
import { followFocus } from "./panes.js";
import { paintHint } from "./bottom.js";
import { paintSelector } from "./selector.js";
import { connect, server, spaces, strings } from "./main.js";

// Set by a switch to another workspace, which skips the leave question.
export let leaving = false;

// Load another workspace, or the Laura server's page, in this tab; `tab` opens a new tab. The
// workspace that the user leaves keeps running. The browser asks before it leaves only while a
// comment box is open, so the comment is not lost. `<base target="_blank">` would open a new tab.
export function go(url, tab) {
  // The user leaves for another workspace.
  if (ui.press) ui.press.acted = true;
  if (tab) return window.open(url, "_blank");
  leaving = !draft();
  location.assign(url);
}

const folderName = root => root.split(/[\\/]/).filter(Boolean).at(-1) ?? root;

export const spaceLabel = w => w.title ?? folderName(w.root);

// The tab icon with an amber dot in the top-right corner for the wait, made from the plain icon so
// that the pebble path stays in one file; null until the fetch ends. A mask cuts a ring around the
// dot, so the dot reads on a light and on a dark tab bar.
const icon = document.querySelector("link[rel=icon]"), plainIcon = icon.href;

let waitIcon = null;

fetch(plainIcon).then(r => r.text()).then(svg => {
  const warn = getComputedStyle(document.documentElement).getPropertyValue("--warn").trim();
  waitIcon = "data:image/svg+xml," + encodeURIComponent(svg
    .replace("<g ", '<mask id="b"><rect x="0" y="0" width="64" height="64" fill="#fff"/>'
      + '<circle cx="48.9" cy="15.1" r="14"/></mask><g mask="url(#b)" ')
    .replace("</svg>", `<circle cx="48.9" cy="15.1" r="11" fill="${warn}"/></svg>`));
  paintTitle();
}).catch(() => {});  // without the badged icon, the tab keeps the plain icon

// The browser tab: the session label, or the folder name before the agent sets one, and the
// badged icon while the agent waits for the user.
export function paintTitle() {
  document.title = `Laura · ${$("wlabel").textContent}`;
  const href = layout?.waiting && waitIcon || plainIcon;
  if (icon.href !== href) icon.href = href;  // the same href again makes the browser fetch the icon again
}

// The workspace switcher: the session label and the folder, or the folder name in grey before the
// agent sets a session label. With one workspace and no Laura server, the button has no menu.
export function paintSwitcher() {
  const root = strings.root ?? "", title = spaces.find(w => w.current)?.title;
  $("wlabel").textContent = title ?? folderName(root);
  paintTitle();
  $("wlabel").classList.toggle("fallback", !title);
  $("wroot").textContent = title ? root : "";
  const menu = spaces.length > 1 || !!server;
  $("caret").hidden = $("wsw").disabled = !menu;
  $("wwait").hidden = !spaces.some(w => !w.current && w.waiting);
  if (menu) $("wsw").dataset.tip = "Switch workspace";
  else delete $("wsw").dataset.tip;
  // With a Laura server, the brand opens its page; with no `href`, the brand is plain text.
  const brand = $("brand");
  if (server) {
    brand.href = server;
    brand.ariaLabel = brand.dataset.tip = "Show the workspaces";
  } else {
    brand.removeAttribute("href");
    brand.removeAttribute("aria-label");
    delete brand.dataset.tip;
  }
  if (!menu) closeMenu();
  else if (!$("menu").hidden) paintMenu();
  if (on("selector")) paintSelector();
}

// A middle click opens a new tab through the `href`.
$("brand").onclick = e => {
  if (!server) return;
  e.preventDefault();
  go(server, e.ctrlKey || e.metaKey);
};

// Static markup; the frame's strings go in as text.
function paintMenu() {
  const on = $("menu").querySelector(".mi.on")?.dataset.url;
  const div = (cls, text) => {
    const d = document.createElement("div");
    d.className = cls;
    if (text !== undefined) d.textContent = text;
    return d;
  };
  const rows = spaces.map(w => {
    const mi = div(`mi${w.current ? " cur" : ""}${w.url === on ? " on" : ""}`);
    mi.setAttribute("role", "menuitem");
    const wait = w.waiting && !w.current;
    mi.innerHTML = `<span class="wdot${wait ? " w" : ""}"></span><div class="mt"><div class="wl"></div><div class="p"></div></div>`
      + (wait ? '<span class="wait">Waiting for you</span>' : "")
      + (w.current ? '<span class="chk" aria-label="This workspace">✓</span>' : '<button class="nt" aria-label="Open in a new tab" data-tip="Open in a new tab">↗</button>');
    if (w.current) mi.setAttribute("aria-current", "true");
    else mi.dataset.url = w.url;
    mi.querySelector(".wl").textContent = spaceLabel(w);
    mi.querySelector(".wl").classList.toggle("fallback", !w.title);
    mi.querySelector(".p").textContent = `${w.root} · ${w.program}`;
    return mi;
  });
  let last = div("mfoot", "Open Laura from your desktop or app menu, or run laura --serve, to start more workspaces.");
  if (server) {
    last = div(`mi mnew${server === on ? " on" : ""}`, "+ New workspace…");
    last.setAttribute("role", "menuitem");
    last.dataset.url = server;
  }
  $("menu").replaceChildren(div("mh", "Workspaces"), ...rows, div("msep"), last);
}

function openMenu() {
  paintMenu();
  // `Enter` then loads the first other workspace, or the Laura server's page.
  $("menu").querySelector(".mi.on")?.classList.remove("on");
  $("menu").querySelector(".mi[data-url]")?.classList.add("on");
  $("menu").style.left = $("wsw").offsetLeft + "px";
  $("menu").hidden = false;
  $("wsw").setAttribute("aria-expanded", "true");
  $("menu").focus();
  paintHint();
}

export function closeMenu() {
  if ($("menu").hidden) return;
  $("menu").hidden = true;
  $("wsw").setAttribute("aria-expanded", "false");
  followFocus(true);
  paintHint();
}

$("wsw").addEventListener("mousedown", e => e.preventDefault());

$("wsw").addEventListener("click", () => ($("menu").hidden ? openMenu() : closeMenu()));

// A row loads its workspace in this tab; ↗, `Ctrl`+click, `Cmd`+click and a middle click open a new tab.
function menuClick(e, tab) {
  const mi = e.target.closest(".mi[data-url]");
  if (!mi) return;
  closeMenu();
  go(mi.dataset.url, tab || !!e.target.closest(".nt") || e.ctrlKey || e.metaKey);
}

$("menu").addEventListener("click", e => menuClick(e, false));

$("menu").addEventListener("auxclick", e => e.button === 1 && menuClick(e, true));

// A click outside the menu closes it.
addEventListener("mousedown", e => e.target.closest("#menu, #wsw") || closeMenu(), true);

export function showOverlay(text, button) {
  $("overlay-text").textContent = text;
  $("continue").hidden = !button;
  $("showws").hidden = !server;
  closeMenu();
  $("overlay").hidden = false;
  document.activeElement?.blur();
}

$("continue").addEventListener("click", () => {
  $("overlay").hidden = true;
  connect();
});

$("showws").addEventListener("click", () => go(server));
