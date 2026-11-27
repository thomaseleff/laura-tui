// The web app's client (#88): the state that every module reads. Laura's layout, the socket, the
// client's draw state in `ui`, and `mode`, which tells which mode takes a keystroke. Every module imports this
// module, and this module imports none, so it runs first and its capture listeners (the wait rule)
// see each event first.
//
// One websocket: JSON text frames for layout, panes and replies; binary frames `[pane id, 8 bytes
// BE][bytes]` for each terminal, and each browser pane's frames (#107). Every string from a frame
// goes in as text or through `esc`; only `pane`/`rows` HTML, which Laura escapes, goes in as markup.

export let ws, layout = null;
// Laura's state, as the client keeps it: `setWs` and `setLayout` set it again (`connect`, `onLayout`).
export const setWs = s => (ws = s);
// A `layout` frame comes before the `acted` replies to the acts that Laura had not read when it built
// the frame (`WebServer::sync` replies, then sends frames), so the acts in `acts` patch it again.
export const setLayout = m => {
  layout = m;
  for (const a of acts) if (a.patch) patchModes(...a.patch);
};
export const $ = id => document.getElementById(id);
export const root = $("ws");

// What the client draws of the modes that Laura holds (`layout.modes`), and the state of the pane in use.
export const ui = {
  // The pane selector's selected row.
  row: 0,
  // The open draft's box (`draft()`): `{text, busy, failed}`. A comment's box is `draftCard`, in its
  // line's thread or below its line; a review body's is in the pane's popover.
  box: null,
  // The pane whose thread list is open, and the pane whose **More** is open.
  listOpen: null,
  moreOpen: null,
  // The page's pane in comment mode, where a click on the page picks an element to comment on.
  commentMode: null,
  // The user's last keystroke or mouse press, for the wait rule below `overlayOpen`.
  press: null,
  // The user drags a resize handle (`dragger`).
  resizing: false,
};

export const terms = new Map(); // pane id → xterm, one per PTY pane

export const acts = []; // `{ then, pane, patch }` for `acted` replies, in send order



export const send = m => ws?.readyState === 1 && ws.send(JSON.stringify(m));
export const frame = (id, bytes) => {
  const out = new Uint8Array(8 + bytes.length);
  new DataView(out.buffer).setBigUint64(0, BigInt(id));
  out.set(bytes, 8);
  if (ws?.readyState === 1) ws.send(out);
};
// `patch`: `patchModes`'s `[add, ...drop]` for the mode that the act changes, kept until Laura's reply.
export function act(m, then, patch) {
  // The user's press sent Laura an action: the server, not the wait rule below, decides the wait.
  if (ui.press && m.act !== "seen") ui.press.acted = true;
  if (patch) patchModes(...patch);
  if (ws?.readyState !== 1) return;
  acts.push({ then, pane: m.pane, patch });
  send(m);
}

export const pane = id => layout?.panes.find(p => p.id === id);
export const focused = () => pane(layout?.focus);
// A mode that Laura holds is on: "help", "selector", "quit", "locked" or "draft".
export const on = m => !!layout?.modes.includes(m);
// The open comment or review body that Laura holds: `{pane, kind, line, anchor}`, else null.
export const draft = () => layout?.draft ?? null;
// Laura's order of its modes (`Tab::modes`).
const ORDER = ["wizard", "help", "selector", "quit", "locked", "draft"];
// The client's copy of Laura's modes, with `add` and without `drop`: the next keystroke goes to the
// new mode before Laura's reply. A patch passed to `act` holds until Laura's `acted` for that act;
// a direct call holds only until the next `layout` frame.
export function patchModes(add, ...drop) {
  if (!layout) return;
  const now = new Set(layout.modes.filter(m => !drop.includes(m)));
  if (add) now.add(add);
  layout.modes = ORDER.filter(m => now.has(m));
  if (!now.has("draft")) layout.draft = null;
}
// Open the help or the pane selector, or close the overlay (`null`).
export function setOverlay(o) {
  act({ act: "overlay", overlay: o }, undefined, [o, "help", "selector", "quit"]);
}
// The lock sends a file pane's or a browser pane's keystrokes to the shell; an editor pane keeps them.
export const toShell = () => on("locked") && focused()?.kind !== "editor";
// The drawer shows while the shell has the focus or the lock's keystrokes.
export const drawerOpen = () => layout.focus === 0 || toShell();
// The modes on now, first to last: the client's overlay and menu, then Laura's. The first takes a
// keystroke before any pane.
export function modes() {
  return [!$("overlay").hidden && "overlay", !$("menu").hidden && "menu", ...(layout?.modes ?? [])].filter(Boolean);
}
// The mode that takes a keystroke; null when the focused pane takes it.
export function mode() {
  return modes()[0] ?? null;
}
const overlayOpen = () => ["overlay", "menu", "help", "selector", "quit"].includes(mode());
export const MODIFIERS = ["Control", "Shift", "Alt", "Meta"];
// The user's keystroke or click that leaves the shell with the keystrokes ends the wait. The rule runs
// after the event's handlers, so the close of the pane selector, the help or the menu counts, and the
// open doesn't. It runs on `click`, not on `mousedown`, which comes before the click that opens the
// help. A click counts in the shell, or when an overlay was open at the press. An event that sends
// Laura an action or opens another workspace leaves the wait to the server, which ends it on the
// user's move to the shell: the focus moves only after the reply. `seen` names the wait shown at the
// press, so a wait that began later stays. Not a focus listener: the agent's `laura focus` and a
// return to the browser tab don't end the wait.
const pressed = () => (ui.press = { overlay: overlayOpen(), wait: layout?.wait, acted: false });
function seenAfter(p, counts) {
  setTimeout(() => counts && p?.wait !== undefined && !p.acted && layout?.waiting && !overlayOpen()
    && $("p0")?.contains(document.activeElement) && act({ act: "seen", wait: p.wait }));
}
addEventListener("mousedown", pressed, true);
addEventListener("keydown", e => MODIFIERS.includes(e.key) || (pressed(), seenAfter(ui.press, true)), true);
addEventListener("click", e => seenAfter(ui.press, ui.press?.overlay || $("p0")?.contains(e.target)), true);

export const esc = s => String(s).replace(/[&<>"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

// The client tests (tests/client.rs) and the browser tests (tests/browser.rs) read layout, terms, ui
// and acts, call act, close ws, and take the shell's link provider as the last of terms.get(0)'s link
// providers. A module's names are not globals, so these lines put them on globalThis: layout and ws
// as getters, since the client sets them again.
Object.defineProperty(globalThis, "layout", { get: () => layout });
Object.defineProperty(globalThis, "ws", { get: () => ws });
Object.assign(globalThis, { terms, ui, acts, act });
