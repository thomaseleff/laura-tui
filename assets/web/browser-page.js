// Laura's script in each document of a browser pane, in the page's own world, before the page's
// scripts. Laura's browser is headless, so a new tab and a dropdown list's popup never reach the
// screencast. `window.open` and a middle-click on a link open the page in the same pane; a link
// with `target=_blank` reports the page as its opener, so `Page::on_popup` catches it. A click on a
// dropdown list only focuses it, so the keys change the choice. The keys that open the popup
// (Space, Enter, Alt+↑/↓, F4) do nothing, as the user wouldn't see the popup that takes the next
// keystrokes; Enter doesn't submit a form from a dropdown list in Chrome either.
//
// The rule: where this script stands in for a Chrome default action, it acts only after all of the
// page's own listeners for the event have run, only when the page didn't cancel the event, and only
// for a trusted event, as Chrome runs no default action for the page's own events. `last` runs the
// action after the event's last listener: a chain of tails, one per node and phase on the event's
// path, each added when the dispatch reaches the node before it, so each runs after that node's own
// listeners. A page that stops the event ends the chain early; the patched `stop*` methods catch it.
//
// On macOS (`mac`, or `LAURA_EMULATE_MAC_SELECT` in the tests), Chrome opens the popup on ↑ and ↓
// instead of changing the choice, so the script changes the choice as Chrome does on Linux:
//   ↓ →        the next valid choice      ↑ ←        the previous one
//   PageDown   3 list items on            PageUp     3 list items back (Blink's NextValidOption)
//   Home       the first valid choice     End        the last valid choice
//   Shift, Ctrl or Meta + ↑/↓: no change, and no popup. Any other key with a modifier: Chrome's own.
// A valid choice is enabled and shown, itself and in its group; a list item is an option, an
// optgroup or an hr.
//
// shortcut: R1, a page that stops the event where no tail follows (`stopImmediatePropagation()`,
// its own `window` capture listener, a closed shadow root) and cancels it with `return false` in
// the same handler gets Laura's action: Laura decides in a microtask, before Chrome reads
// `return false`. R2, a page that stops the event there and cancels it in a later listener on the
// same node gets it too. R3, a listener the page adds in a microtask, between 2 tails on one node,
// runs after Laura's tail. R4, Laura's `input` and `change` are untrusted. R5, the page can read
// the patched `Event.prototype` methods with `toString`. R6, a document after `document.open()`
// has no listeners of Laura's, so on macOS ↑ and ↓ open the hidden popup there. Check each
// dispatch's end from outside the page (CDP) if users hit one.
// ponytail: `window.open` returns `null`, and a Ctrl+click still opens a hidden tab; open new tabs
// as panes if users miss them.

const add = EventTarget.prototype.addEventListener;
const remove = EventTarget.prototype.removeEventListener;
const E = Event.prototype;
const cancelBubble = Object.getOwnPropertyDescriptor(E, "cancelBubble");
// The events whose tail is pending: the tail's node, phase and listener, and `finish`.
const pending = new WeakMap();

// Runs `act` once after `e`'s last listener, unless the page cancelled `e`.
const last = (e, act) => {
  const p = e.composedPath();
  const steps = [...p.slice(0, -1).reverse().map(n => [n, true]), ...p.map(n => [n, false])];
  const t = {};
  t.finish = () => {
    if (t.done) return;
    t.done = true;
    remove.call(t.node, e.type, t.fn, t.capture);
    pending.delete(e);
    if (!e.defaultPrevented) act();
  };
  const step = k => {
    if (t.fn) remove.call(t.node, e.type, t.fn, t.capture);
    [t.node, t.capture] = steps[k];
    t.fn = ev => {
      if (ev !== e) return; // a dispatch nested in this one
      if (e.cancelBubble || k == steps.length - 1) t.finish();
      else step(k + 1);
    };
    add.call(t.node, e.type, t.fn, t.capture);
  };
  pending.set(e, t);
  step(0);
};

// A stop where the pending tail won't run next: decide once this listener returns.
const stopped = (e, now) => {
  const t = pending.get(e);
  const here = t?.node === e.currentTarget && (e.eventPhase == 2 || t.capture == (e.eventPhase == 1));
  if (t && (now || !here)) queueMicrotask(t.finish);
};
for (const [name, now] of [["stopPropagation", false], ["stopImmediatePropagation", true]]) {
  const f = E[name];
  E[name] = function () {
    stopped(this, now);
    return f.call(this);
  };
}
Object.defineProperty(E, "cancelBubble", {
  ...cancelBubble,
  set(v) {
    if (v) stopped(this, false);
    cancelBubble.set.call(this, v);
  },
});

// A dropdown list whose popup is hidden; `appearance: base-select` shows its picker in the page.
const isMenu = s =>
  s instanceof HTMLSelectElement && !s.multiple && s.size <= 1 &&
  getComputedStyle(s).appearance != "base-select";
const live = s => isMenu(s) && s.isConnected && !s.matches(":disabled") && s.getClientRects().length > 0;
// `getComputedStyle`, as a closed list's choices have no layout box to check.
const hid = o => getComputedStyle(o).display == "none";
const valid = o => o instanceof HTMLOptionElement && !o.matches(":disabled") && !hid(o) && !hid(o.parentElement);
// Blink's NextValidOption: `skip` list items from `i` in `dir`, or the last valid choice before the end.
const next = (items, i, dir, skip) => {
  let found = null;
  for (i += dir; i >= 0 && i < items.length; i += dir) {
    skip--;
    if (!valid(items[i])) continue;
    found = items[i];
    if (skip <= 0) break;
  }
  return found;
};
// macOS's keys: direction, list items to skip, and whether to start from the list's end.
const moves = {
  ArrowDown: [1, 1], ArrowRight: [1, 1], ArrowUp: [-1, 1], ArrowLeft: [-1, 1],
  PageDown: [1, 3], PageUp: [-1, 3], Home: [1, 1, true], End: [-1, 1, true],
};
// Each list's last printable keypress that typed: Chrome's type-ahead takes a Space for 1 s after it.
const typed = new WeakMap();

const on = (type, f) => add.call(window, type, e => e.isTrusted && f(e), true);
on("mousedown", e => {
  const s = e.button === 0 && e.composedPath()[0].closest?.("select");
  if (isMenu(s)) last(e, () => live(s) && (e.preventDefault(), s.focus()));
});
on("keydown", e => {
  const s = e.composedPath()[0], k = e.key;
  if (!isMenu(s)) return;
  if (k == "F4" || e.altKey && /^Arrow(Up|Down)$/.test(k)) return last(e, () => live(s) && e.preventDefault());
  const m = moves[k];
  if (!mac || !m || e.altKey) return;
  if (e.shiftKey || e.ctrlKey || e.metaKey) {
    if (/^Arrow(Up|Down)$/.test(k)) last(e, () => live(s) && e.preventDefault());
    return;
  }
  last(e, () => {
    if (!live(s)) return;
    e.preventDefault();
    const [dir, skip, end] = m;
    const items = [...s.querySelectorAll("option, optgroup, hr")];
    const from = end ? (dir > 0 ? -1 : items.length) : items.indexOf(s.options[s.selectedIndex]);
    const o = next(items, from, dir, skip);
    if (!o || o.selected) return;
    s.selectedIndex = o.index;
    // Composed, as Chrome sends it, so a page outside a web component sees it; `change` isn't.
    s.dispatchEvent(new Event("input", { bubbles: true, composed: true }));
    s.dispatchEvent(new Event("change", { bubbles: true }));
  });
});
on("keypress", e => {
  const s = e.composedPath()[0];
  if (!isMenu(s)) return;
  last(e, () => {
    if (!live(s)) return;
    const session = e.timeStamp - (typed.get(s) ?? -Infinity) < 1000;
    if (e.key == "Enter" || e.key == " " && !session) e.preventDefault();
    else typed.set(s, e.timeStamp);
  });
});
on("auxclick", e => {
  const a = e.button === 1 && e.composedPath()[0].closest?.("a[href]");
  if (a) last(e, () => (e.preventDefault(), location.assign(a.href)));
});
window.open = u => (u && location.assign(u), null);
