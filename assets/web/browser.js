// Laura's script in a browser pane's page (#107), in an isolated world that the page's scripts can't
// see. Laura sends the pane's threads and cursor through `__laura.set`, and this script finds each
// thread's element: the element that its CSS selector finds, else the element that its quote starts.
// `__laura.pick` names the element at a point, and `__laura.find` the element that the agent's
// selector finds. Through the `laura` binding, the script tells Laura the boxes of the elements that
// the web app outlines, and the threads whose element is not on the page.
const reply = m => typeof laura === "function" && laura(JSON.stringify(m));
// A mobile page with no meta viewport lays out wider than the device and shows zoomed out. The
// frame, and so the web app, is in the visual viewport's px. A point in a frame goes through each
// frame's box up to the top page; a cross-origin frame has no `frameElement`, and no point.
const toFrame = (x, y) => {
  for (let w = window; w !== top; w = w.parent) {
    const f = w.frameElement;
    if (!f) return null;
    const r = f.getBoundingClientRect();
    [x, y] = [x + r.left + f.clientLeft, y + r.top + f.clientTop];
  }
  const v = top.visualViewport;
  return [(x - v.offsetLeft) * v.scale, (y - v.offsetTop) * v.scale];
};
if (window === top) {
  const flat = s => (s ?? "").replace(/\s+/g, " ").trim();
  const cut = (s, n) => (s.length > n ? "…" + s.slice(s.length - n + 1) : s);
  const real = el => (el && !el.matches("html, body, head") ? el : null);
  // A unique `#id`, else `tag:nth-of-type(n)` steps up to `body`, with `nth` only among same-tag
  // siblings. `body > ` pins a path that a deeper element also matches.
  const selectorOf = el => {
    const steps = [];
    for (let e = el; e && e !== document.body && e !== document.documentElement; e = e.parentElement) {
      const id = e.id && "#" + CSS.escape(e.id);
      if (id && document.querySelectorAll(id).length === 1) {
        steps.unshift(id);
        break;
      }
      const same = [...(e.parentElement?.children ?? [])].filter(s => s.localName === e.localName);
      steps.unshift(same.length > 1 ? `${e.localName}:nth-of-type(${same.indexOf(e) + 1})` : e.localName);
    }
    const sel = steps.join(" > ");
    return document.querySelector(sel) === el ? sel : "body > " + sel;
  };
  const quoteOf = el => Array.from(flat(el.textContent)).slice(0, 60).join("");
  // The selector's element when its text still starts with the quote, else the deepest element whose
  // text does, else the selector's element (a hot update changed its text).
  // ponytail: the quote search reads every element's text, O(n²) on a large page; index the text if
  // a large page lags.
  const find = t => {
    let el = null;
    try { el = document.querySelector(t.selector); } catch {}
    if (el && quoteOf(el) === t.quote) return el;
    let best = null;
    if (t.quote) {
      for (const e of document.body.querySelectorAll("*")) {
        if ((!best || best.contains(e)) && flat(e.textContent).startsWith(t.quote)) best = e;
      }
    }
    return best ?? el;
  };
  // A hash-routed app's route, such as `#/settings`, is a page of its own.
  const here = () => /^#!?\//.test(location.hash) ? location.href : location.href.split("#")[0];
  const vv = visualViewport;
  let state = null, apply = null, lastCursor, lastHl, lastLost = null, timer = 0, rect;
  // The lines of the threads whose element this document has shown.
  const seen = new Set();
  // Draws the threads on this page; returns the lines of those whose element is not on the page.
  const draw = () => {
    if (!apply || !state) return [];
    const threads = [], lost = [];
    for (const t of state.threads) {
      if (t.url !== here()) continue;
      const el = real(find(t));
      if (el) {
        threads.push([el, t]);
        seen.add(t.line);
      } else lost.push(t.line);
    }
    const c = state.cursor?.url === here() ? real(find(state.cursor)) : null;
    const hl = state.highlight?.url === here() ? real(find(state.highlight)) : null;
    apply({ threads, cur: state.focus ? c : null, anchor: c, hl, comment: state.comment });
    if (state.cursor?.line !== lastCursor) c?.scrollIntoView({ block: "nearest" });
    if (state.highlight?.line !== lastHl) hl?.scrollIntoView({ block: "center" });
    lastCursor = state.cursor?.line;
    lastHl = state.highlight?.line;
    return lost;
  };
  // 500 ms after the page stops changing, once it has loaded: a hot update or a script's render
  // replaces elements, which then show again.
  // ponytail: a page that changes its DOM more often than every 500 ms (a clock) never runs the
  // check; check on a timer too if a page needs it.
  // An element that this document hasn't shown yet gets 5 s from the start of the load, for a page
  // that shows it after it gets its data. An element that the page showed and then lost doesn't wait.
  // ponytail: data that arrives more than 5 s after the page starts to load still gives the notice;
  // wait on the page's requests if users hit it.
  const check = () => {
    clearTimeout(timer);
    timer = setTimeout(() => {
      if (document.readyState !== "complete" || !state) return;
      const lost = draw();
      if (performance.now() < 5000 && lost.some(l => !seen.has(l))) {
        timer = setTimeout(check, 5000 - performance.now());
        return;
      }
      const key = here() + " " + lost;
      if (key !== lastLost) reply({ lost, url: here() });
      lastLost = key;
    }, 500);
  };
  // A thread's element keeps the thread's selector and quote after the page changes its text, so a
  // pick or the agent's selector on it names the thread.
  const name = (el, selector) => {
    const on = state?.threads.find(t => t.url === here() && real(find(t)) === el);
    return on ? { selector: on.selector, quote: on.quote, url: on.url } : { selector, quote: quoteOf(el), url: here() };
  };
  globalThis.__laura = {
    set(s) {
      state = s;
      draw();
      check();
    },
    pick(x, y) {
      [x, y] = [x / vv.scale + vv.offsetLeft, y / vv.scale + vv.offsetTop];
      // ponytail: no selector reaches into a shadow root or a frame, so the element is the web
      // component or the frame; add piercing selectors when users comment inside web components.
      const el = document.elementsFromPoint(x, y).find(e => e.matches("body *"));
      if (!el) return null;
      return name(el, selectorOf(el));
    },
    // The agent's selector, as the agent wrote it.
    find(sel) {
      let el;
      try { el = real(document.querySelector(sel)); } catch { return { error: "invalid" }; }
      return el && name(el, sel);
    },
    // The pane header's highlight chip.
    reveal() {
      const hl = state?.highlight?.url === here() && real(find(state.highlight));
      hl && hl.scrollIntoView({ block: "center" });
    },
  };
  // What Laura shows on the page, as boxes `[top, left, bottom, right]` in the frame's px. The web
  // app draws the outlines, tags and pins over the canvas, so the agent's screenshots and snapshots
  // show none of them. `rect` is the cursor's element's box, which the cursor's thread floats beside.
  // shortcut: the boxes reach the web app beside the frames, not in them, and can trail the page by
  // a frame while it scrolls; place them by the frame's timestamp if users see them lag.
  const reporter = () => {
    let s = null, hov = null, raf = 0, sent = "";
    const box = el => {
      if (!el?.getClientRects().length) return null;
      const r = el.getBoundingClientRect(), [l, t] = toFrame(r.left, r.top), [rt, b] = toFrame(r.right, r.bottom);
      return [t, l, b, rt];
    };
    const sync = () => raf ||= requestAnimationFrame(() => {
      raf = 0;
      if (!s) return;
      const a = box(s.anchor);
      if (String(a) !== rect) reply({ rect: a });
      rect = String(a);
      // The hovered element's tag, and the cursor's element's tag when no hover covers it.
      const tag = el => cut(selectorOf(el), 40);
      const cur = box(s.cur), h = box(hov);
      const m = {
        th: s.threads.flatMap(([el, t]) => (b => (b ? [[t.line, ...b]] : []))(box(el))),
        hl: box(s.hl),
        cur: cur && [cur, s.comment || s.cur === hov ? null : tag(s.cur)],
        hov: h && [h, tag(hov)],
      };
      const k = JSON.stringify(m);
      if (k !== sent) reply({ marks: m });
      sent = k;
    });
    // In comment mode, the web app sends the pointer's moves and no press; a hover outlines the
    // element under the pointer.
    const hover = el => ((hov = el), sync());
    addEventListener("mouseover", e => s?.comment && hover(real(e.target?.closest?.("body *") ?? null)), true);
    addEventListener("mouseout", e => s?.comment && !e.relatedTarget && hover(null), true);
    addEventListener("scroll", sync, true);
    addEventListener("resize", sync);
    // The page moves its elements without a scroll: a script's DOM change, an image that loads, a
    // transition that ends.
    new MutationObserver(sync).observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
    for (const type of ["load", "transitionend", "animationend"]) addEventListener(type, sync, true);
    // Laura drops the marks when the top frame navigates; a page back from the back/forward cache
    // sends its own again.
    addEventListener("pageshow", e => e.persisted && ((sent = rect = ""), sync()));
    return state => {
      s = state;
      if (!s.comment) hov = null;
      sync();
    };
  };
  // The script runs before the page has a `body`. A document with no `body` (an SVG file) gets no
  // threads.
  const start = () => {
    if (!document.body) return;
    apply = reporter();
    new MutationObserver(check)
      .observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
    addEventListener("load", check);
    draw();
    check();
  };
  if (document.readyState === "loading") addEventListener("DOMContentLoaded", start, { once: true });
  else start();
}
// Each click and input in any frame, for the web app's marks of the agent's clicks and typing. Laura
// drops the user's own. A click with no pointer (a script's `click()`, a key) marks the element's
// center.
// ponytail: a page's own script click() or input event, with no user input in the 500 ms before it,
// also shows a mark; it can't be told from the agent's script click.
// ponytail: a cross-origin frame runs in its own process, which Laura's script doesn't reach; no
// mark there.
{
  const box = e => e.target instanceof Element ? e.target.getBoundingClientRect() : null;
  addEventListener("click", e => {
    const r = box(e);
    const at = e.detail ? toFrame(e.clientX, e.clientY) : r && toFrame(r.left + r.width / 2, r.top + r.height / 2);
    at && reply({ click: at });
  }, true);
  addEventListener("input", e => {
    const r = box(e), at = r && toFrame(r.left, r.top);
    at && reply({ type: [...at, r.width * top.visualViewport.scale, r.height * top.visualViewport.scale] });
  }, true);
}
