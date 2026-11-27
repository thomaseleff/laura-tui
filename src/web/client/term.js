// The terminals: the shell's and each editor pane's xterm.js, their themes and keystrokes, the
// shell's links, and the grid's cell, which the probe measures.

import { $, act, frame, layout, mode, pane, root, send, terms, ui } from "./state.js";
import { fitDrawer } from "./panes.js";

const css = getComputedStyle(document.documentElement);
const v = name => css.getPropertyValue(name).trim();

const enc = new TextEncoder();
const ANSI = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
// An xterm.js theme: `base`, and the 16 ANSI colors from a computed style's `--ansi-N`.
function theme(style, base) {
  ANSI.forEach((c, i) => {
    base[c] = style.getPropertyValue(`--ansi-${i}`).trim();
    base["bright" + c[0].toUpperCase() + c.slice(1)] = style.getPropertyValue(`--ansi-${i + 8}`).trim();
  });
  return base;
}
// An editor view keeps Nord, from `:root`.
const THEME = theme(css, { background: v("--term-bg"), foreground: v("--term-fg"), cursor: v("--term-fg"), selectionBackground: v("--term-sel") });
// The shell is light: `.rows`' set, which a short-lived element reads.
const LIGHT = (() => {
  const el = document.body.appendChild(document.createElement("div"));
  el.className = "rows";
  const t = theme(getComputedStyle(el), { background: v("--pane"), foreground: v("--fg"), cursor: v("--acc"), cursorAccent: "#fff", selectionBackground: v("--shell-sel") });
  el.remove();
  return t;
})();

// The grid's cell (`measure`).
export let cw = 0, ch = 0;

// A hidden terminal with an editor pane's type, which measures the grid's cell (`measure`).
let probe = null;

// One cell: xterm's own once the probe has drawn (xterm.js can't be given one), else the font's
// advance. The shell has its own type, so the probe, not the shell, sets the grid.
export function measure() {
  const s = document.createElement("span");
  s.className = "probe";
  s.textContent = "W".repeat(200);
  document.body.append(s);
  const r = s.getBoundingClientRect();
  s.remove();
  [cw, ch] = [r.width / 200, r.height];
  // The font's advance; file-pane text is letter-spaced from it onto the grid's cell.
  document.documentElement.style.setProperty("--pw", cw + "px");
  const r2 = probe?.element?.querySelector(".xterm-screen")?.getBoundingClientRect();
  if (r2?.width) [cw, ch] = [r2.width / probe.cols, r2.height / probe.rows];
  document.documentElement.style.setProperty("--cw", cw + "px");
  document.documentElement.style.setProperty("--ch", ch + "px");
}

// Cells kept clear left and right of the grid, so the borders don't touch the window's edges.
const MARGIN = 0.5;

// The bottom line's height (`#status`), below the grid.
const STATUS = 34;

export function report() {
  measure();
  // The body, not the window: the body keeps the workspace's minimum size (style.css).
  const { clientWidth: w, clientHeight: h } = document.body;
  const cols = Math.floor(w / cw - 2 * MARGIN);
  const rows = Math.floor((h - $("top").offsetHeight - STATUS) / ch);
  const style = document.documentElement.style;
  style.setProperty("--cols", cols);
  style.setProperty("--rows", rows);
  style.setProperty("--side", (w - cols * cw) / 2 + "px");
  send({ view: { cols, rows } });
}

// The probe: an editor pane's options, on screen but hidden, so xterm.js draws it. Each draw is a
// new cell size, such as after a zoom, so the grid follows.
export function makeProbe() {
  const el = document.body.appendChild(document.createElement("div"));
  el.className = "tprobe";
  probe = new Terminal({ ...TERM, cols: 200, rows: 4, disableStdin: true, cursorBlink: false });
  probe.open(el);
  webgl(probe);
  probe.onRender(report);
}

// The shell's row height and its box's top in the drawer, which `fitShell` measures.
let shellRow = 0, shellTop = 0;

// The drawer's height in px that shows `n` rows of the shell: the header, the body's inset and
// padding, and the rows. 0 before `fitShell` measures the shell.
export const rowsPx = n => shellRow && Math.ceil(shellTop + 6 + 32 + n * shellRow);

// The shell's size from its own cell, in its box less the padding (style.css) and the scrollbar.
// `force` sends the size to a new connection.
export function fitShell(force) {
  // While the user drags a size, the agent redraws once, on release.
  if (ui.resizing) return;
  const t = terms.get(0), box = $("p0")?.querySelector(".term");
  const r = t?.element?.querySelector(".xterm-screen")?.getBoundingClientRect();
  if (!r?.width || !box.clientWidth) return;
  // Laura's minimum (`cells` in src/web.rs), so xterm and the shell use one grid.
  const cols = Math.max(20, Math.floor((box.clientWidth - 40 - 10) / (r.width / t.cols)));
  const rows = Math.max(5, Math.floor((box.clientHeight - 32) / (r.height / t.rows)));
  [shellRow, shellTop] = [r.height / t.rows, box.offsetTop];
  // The drawer's minimum shows those 5 rows.
  root.style.setProperty("--drawer-min", rowsPx(5) + "px");
  // The start height needs the row height.
  fitDrawer();
  if (cols === t.cols && rows === t.rows && !force) return;
  t.resize(cols, rows);
  send({ shell: { cols, rows } });
}

// Laura's keys inside a terminal: false leaves the key to the keydown listener (keys.js), unsent.
function termKeys(id, term, e) {
  if (e.key === "F12") return false;
  const m = mode();
  if (m) return m === "locked";
  // `code`, so a layout that types another character on that key still shows or hides the shell.
  if (e.ctrlKey && !e.altKey && (e.code === "Backquote" || e.code === "Period")) return false;
  if (id === 0 && e.ctrlKey && e.shiftKey && !e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown") && pane(0)?.drawer) return false;
  if (e.key === "Escape" && (ui.listOpen ?? ui.moreOpen) !== null) return false;
  if (e.ctrlKey && !e.altKey && "pPhHqQ".includes(e.key) && e.key.length === 1) return false;
  if (e.ctrlKey && !e.altKey && (e.key === "l" || e.key === "L") && id !== 0) return false;
  if (id === 0 && (e.key === "PageUp" || e.key === "PageDown") && term.buffer.active.type === "normal") {
    // xterm.js asks on keydown, keypress and keyup: scroll once.
    if (e.type === "keydown") term.scrollPages(e.key === "PageUp" ? -1 : 1);
    return false;
  }
  return true;
}

// An editor pane's terminal, which sits on the grid.
const TERM = {
  fontFamily: v("--mono"), fontSize: 14, lineHeight: 1, cursorBlink: true, allowProposedApi: true,
  scrollback: 10000, // src/pty.rs SCROLLBACK, so the replay keeps the shell's whole history
  theme: THEME,
};

// The shell reads less like a terminal: Laura's light colors, smaller type and more space between
// lines. The contrast keeps an agent's dark theme readable on white.
const SHELL = {
  ...TERM, fontSize: 13, lineHeight: 1.3, theme: LIGHT, minimumContrastRatio: 4.5,
  cursorStyle: "bar", cursorInactiveStyle: "none",
};

// The DOM renderer draws block characters from the font, with gaps; WebGL fills the cell.
// Without WebGL2 (a VM, remote desktop), the addon throws and the DOM renderer stays.
function webgl(term) {
  try {
    const gl = new WebglAddon.WebglAddon();
    gl.onContextLoss(() => gl.dispose());
    term.loadAddon(gl);
  } catch {}
}

// A file path in the shell with a `/` or `\` and an extension with a letter, and an optional `:line`.
// A path starts a word: at the row's start, after a space or an opening mark (`Update(src/a.rs)` links
// `src/a.rs`), or after a word's `**` but not a glob's (`src/**/a.rs`, `**/a.rs`). A path ends its word:
// a letter, digit, `_+-`, `/` or `\` after the extension, also after dots, means a longer name
// (`foo.test.js/index`, `app.log.1`), which is no link, so backtracking can't link a part of a path.
// A folder can be a whole `[slug]` or `(group)`, or start with `@` or `+`. Not a home path (`~/a/b.rs`);
// the link provider skips URLs.
export const PATH = /(?<=(?:^|[\s(\[{<"'`=“‘])(?:\*+(?![\/\\]))?)(?:[A-Za-z]:[\/\\]|\.{0,2}[\/\\])?(?:(?:[@+]?[\p{L}\p{N}_.+-]+|\[[^\]\/\\\s]+\]|\([^)\/\\\s]+\))[\/\\])*\+?[\p{L}\p{N}_.-]*[\p{L}\p{N}_-]\.[A-Za-z0-9]*[A-Za-z][A-Za-z0-9]*(?!\.*[\/\\\p{L}\p{N}_+-])(?::\d+)?/gu;

export function makeTerm(id, el) {
  // The saved screen comes at the PTY's size; `fitShell` fits the shell to its box after the first render.
  const term = new Terminal(id === 0 ? { ...SHELL, ...layout.shell } : TERM);
  // Emoji are two cells wide in Unicode 11+, as the child and vt100 count them.
  term.loadAddon(new Unicode11Addon.Unicode11Addon());
  term.unicode.activeVersion = "11";
  // The child sees the same terminal as in the TUI: no replies to its queries.
  const p = term.parser, done = () => true;
  for (const q of [{ final: "c" }, { prefix: ">", final: "c" }, { final: "n" }, { prefix: "?", final: "n" },
    { intermediates: "$", final: "p" }, { prefix: "?", intermediates: "$", final: "p" }]) p.registerCsiHandler(q, done);
  p.registerDcsHandler({ intermediates: "$", final: "q" }, done);
  for (const n of [4, 10, 11, 12]) p.registerOscHandler(n, done);
  term.onData(d => {
    // Focus reports: the TUI never sends them.
    if (d === "\x1b[I" || d === "\x1b[O") return;
    frame(id, enc.encode(d));
  });
  term.onBinary(d => frame(id, Uint8Array.from(d, c => c.charCodeAt(0))));
  term.attachCustomKeyEventHandler(e => termKeys(id, term, e));
  term.open(el.querySelector(".term"));
  webgl(term);
  terms.set(id, term);
  if (id === 0) {
    const once = term.onRender(() => (once.dispose(), fitShell()));
    new ResizeObserver(() => fitShell()).observe(el.querySelector(".term"));
    // A wheel step scrolls the shell without the focus, so the keystroke-or-click rule doesn't see it.
    el.querySelector(".term").addEventListener("wheel", () => layout.waiting && act({ act: "seen", wait: layout.wait }), { capture: true, passive: true });
    term.registerLinkProvider({ provideLinks(y, cb) {
      // shortcut: Laura links neither part of a path that wraps onto the next row; join wrapped rows if agents wrap paths.
      const line = term.buffer.active.getLine(y - 1);
      // The text, and each character's column: an emoji takes two columns.
      let text = "";
      const col = [];
      for (let x = 0; line && x < line.length; x++) {
        const c = line.getCell(x);
        if (!c?.getWidth()) continue;
        const s = c.getChars() || " ";
        text += s;
        for (let i = 0; i < s.length; i++) col.push(x + 1);
      }
      // A path in a URL (`https://x.com/@a/b.js`) is no link.
      const urls = [...text.matchAll(/[a-z][\w+.-]*:\/\/\S+/gi)].map(u => [u.index, u.index + u[0].length]);
      // The word that crosses a row boundary, a wrapped path or URL, is no link: the URL's second row has no scheme.
      const head = line?.isWrapped ? text.search(/\s|$/) : 0;
      const tail = term.buffer.active.getLine(y)?.isWrapped ? text.search(/\S*$/) : text.length;
      cb([...text.matchAll(PATH)].filter(m => /[\/\\]/.test(m[0]) && m.index >= head && m.index + m[0].length <= tail
        && !urls.some(([a, b]) => m.index >= a && m.index < b)).map(m => ({
        text: m[0],
        range: { start: { x: col[m.index], y }, end: { x: col[m.index + m[0].length - 1], y } },
        decorations: { underline: true, pointerCursor: true },
        activate: () => {
          const [, path, n] = m[0].match(/^(.*?)(?::(\d+))?$/);
          // `pane` only routes an error to the drawer.
          act({ act: "open_path", pane: 0, path, line: n ? +n : undefined });
        },
      })));
    } });
  }
  // Laura brackets a paste, as the TUI does; xterm.js mustn't send it too. A box in an editor
  // pane (a comment, the review body) takes its own paste.
  el.addEventListener("paste", e => {
    if (e.target.closest?.(".draft, .pop")) return;
    e.preventDefault();
    e.stopPropagation();
    act({ act: "paste", pane: id, text: e.clipboardData.getData("text") });
  }, true);
}
