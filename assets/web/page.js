// Laura's script in an HTML file pane's page (#89). A click reports its element's start-tag line
// (`data-laura-line`) to the workspace, which moves the cursor there; the workspace sends back the
// cursor's, the threads' and the highlight's lines, which this script outlines. In comment mode, a
// click picks an element to comment on instead, and the page's own handlers don't run.
(() => {
  const post = m => parent.postMessage(m, "*");
  const TEXT = "textarea, select, input:not([type=checkbox], [type=radio], [type=button],"
    + " [type=submit], [type=reset], [type=image], [type=file], [type=color])";
  // A malformed escape keeps its raw text, as the browser's own fragment lookup does.
  const decode = h => { try { return decodeURIComponent(h); } catch { return h; } };
  // Comment mode (#108): the workspace's pane header turns it on; a click picks an element.
  let comment = false, hov = null, anchor = null, raf = 0;
  // An element the user can pick: not the document's own `html`, `head` or `body`.
  const real = el => (el && !el.matches("html, body, head") ? el : null);
  const pick = t => real(t?.closest?.("[data-laura-line]"));
  // An element's name, as the workspace's thread labels read it.
  const label = el => `<${el.tagName.toLowerCase()}${el.id ? "#" + el.id : el.classList[0] ? "." + el.classList[0] : ""}>`;
  // The hovered element's and the cursor's element's line and name, above their outlines.
  const mkTag = () => {
    const t = document.createElement("div");
    t.style.cssText = "all: initial; position: fixed; z-index: 2147483647; pointer-events: none; display: none;"
      + " padding: 0 6px; border-radius: 4px 4px 4px 0; font: 500 11px/16px monospace; color: #fff;"
      + " background: #4f5bd5; white-space: nowrap;";
    document.documentElement.append(t);
    return t;
  };
  const tag = mkTag(), curTag = mkTag();
  // An element's box, or null when it shows nowhere, such as inside a closed `<dialog>`.
  const box = el => (el?.getClientRects().length ? el.getBoundingClientRect() : null);
  const place = (t, el) => {
    const h = box(el);
    t.style.display = h ? "block" : "none";
    if (!h) return;
    t.textContent = `L${el.dataset.lauraLine} ${label(el)}`;
    t.style.left = h.left - 4 + "px";
    t.style.top = Math.max(0, h.top - 20) + "px";
  };
  // Each thread's pin: its authors and comment count, at its element's top-right corner. A click on
  // a pin opens its thread and reaches none of the page's handlers.
  const pinLayer = document.createElement("div");
  pinLayer.setAttribute("data-laura-pin", "");
  pinLayer.style.cssText = "all: initial; position: fixed; inset: 0; z-index: 2147483646; pointer-events: none;";
  document.documentElement.append(pinLayer);
  let pins = [], cur = null;
  const pinOf = t => t instanceof Element ? t.closest("[data-laura-pin] > [data-line]") : null;
  // Once a frame: the tags and pins follow their elements, and the workspace floats the cursor's
  // thread beside the cursor's element.
  const sync = () => raf ||= requestAnimationFrame(() => {
    raf = 0;
    place(tag, hov);
    place(curTag, comment || cur === hov ? null : cur);
    for (const [el, pin] of pins) {
      const h = box(el);
      pin.style.display = h ? "flex" : "none";
      if (!h) continue;
      pin.style.left = Math.max(0, Math.min(h.right - 8, innerWidth - 60)) + "px";
      pin.style.top = Math.max(0, h.top - 12) + "px";
    }
    const r = box(anchor);
    post(r ? { laura: "rect", top: r.top, left: r.left, bottom: r.bottom, right: r.right } : { laura: "rect" });
  });
  const hover = el => {
    hov?.removeAttribute("data-laura-hov");
    hov = el;
    el?.setAttribute("data-laura-hov", "");
    sync();
  };
  addEventListener("mouseover", e => comment && hover(pick(e.target)), true);
  addEventListener("mouseout", e => comment && !e.relatedTarget && hover(null), true);
  addEventListener("scroll", sync, true);
  addEventListener("resize", sync);
  // The page moves its elements without a scroll: a script's DOM change, an image that loads, a
  // transition that ends. Laura's tags and pins sit outside `body`, so they don't trigger this.
  // ponytail: a pin lags during a CSS transition and moves at its end; sync each frame while one runs if that shows.
  if (document.body) new MutationObserver(sync).observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
  for (const type of ["load", "transitionend", "animationend"]) addEventListener(type, sync, true);
  // In comment mode, a press moves no focus, selects nothing, and reaches no handler of the page's.
  // ponytail: a page's own capture listener on `window`, added before this script, still runs;
  // inject this script at the start of `<head>` if a page needs it.
  for (const type of ["pointerdown", "mousedown", "pointerup", "mouseup", "dblclick"]) {
    addEventListener(type, e => (comment || pinOf(e.target)) && (e.preventDefault(), e.stopImmediatePropagation()), true);
  }
  let down;
  addEventListener("mousedown", e => (down = [e.screenX, e.screenY]), true);
  addEventListener("click", e => {
    if (!e.isTrusted) return;
    const pin = pinOf(e.target);
    if (pin) {
      e.preventDefault();
      e.stopImmediatePropagation();
      return post({ laura: "line", line: +pin.dataset.line, pin: true });
    }
    const t = e.target instanceof Element ? e.target : e.target.parentElement;
    if (comment) {
      e.preventDefault();
      e.stopImmediatePropagation();
      const el = pick(t);
      if (el) post({ laura: "line", line: (last = +el.dataset.lauraLine), pick: true });
      return;
    }
    const el = t?.closest("[data-laura-line]");
    const href = t?.closest("a[href], area[href]")?.getAttribute("href")?.trim();
    // A page's `<base href>` changes where its links point, as in a browser.
    const based = !!document.querySelector("base[href]");
    const u = href == null || (!based && href.startsWith("#"))
      ? null : URL.parse(href, document.baseURI);
    const mine = u?.origin === location.origin;
    // The decoded paths, not the URLs: the frame's URL carries `?v=…` and escapes more than a link does.
    const here = mine && !!u.hash && decode(u.pathname) === decode(location.pathname);
    const dir = location.pathname.slice(0, location.pathname.lastIndexOf("/") + 1);
    // A web link, or a file's path from the page's folder.
    // ponytail: with a `<base>`, a link that resolves above the page's folder does nothing, as the
    // `/f/<nonce>/` prefix is lost; resolve against a synthetic base if a page needs it.
    const to = !u || here ? null
      : !mine ? (/^https?:$/.test(u.protocol) ? u.href : null)
      : !based ? href
      : u.pathname.startsWith(dir) ? u.pathname.slice(dir.length) + u.hash : null;
    if (to != null || here) e.preventDefault();
    // The clicked element is in view, so the cursor that comes back for it doesn't scroll.
    if (el) last = +el.dataset.lauraLine;
    // After the click's default action, which moves focus to a label's field.
    setTimeout(() => {
      const a = document.activeElement;
      // A drag or a double-click that selects text keeps the focus in the frame, so `Ctrl+C` copies
      // it. A selection left from before, such as when the user clicks a button, doesn't.
      const picked = e.detail > 1 || Math.hypot(e.screenX - down?.[0], e.screenY - down?.[1]) > 3;
      // The clicked element itself with a `tabindex`, such as a game's canvas, takes the keystrokes;
      // a click inside a `<pre tabindex>` or `<main tabindex>` doesn't.
      const editable = picked && !getSelection().isCollapsed
        || !!a && (a.isContentEditable || a.matches(TEXT) || a === t && a.hasAttribute("tabindex"));
      // Every click, so a click on blank space gives the keystrokes back too.
      post({ laura: "line", line: el ? +el.dataset.lauraLine : null, editable });
      if (to != null) post({ laura: "link", href: to });
      if (here) document.getElementById(decode(u.hash.slice(1)))?.scrollIntoView();
    });
  }, true);
  // A page has no server: its script handles a form, and the frame keeps the page. A `dialog`
  // form closes its dialog, as in a browser. `getAttribute`: an `<input name="method">` shadows
  // `form.method`.
  const dialog = (form, by) =>
    (by?.getAttribute("formmethod") ?? form.getAttribute("method"))?.toLowerCase() === "dialog";
  // Capture, so a page's handler that stops the event can't let the frame navigate.
  addEventListener("submit", e => dialog(e.target, e.submitter) || e.preventDefault(), true);
  // `form.submit()` fires no `submit` event.
  const submit = HTMLFormElement.prototype.submit;
  HTMLFormElement.prototype.submit = function () { if (dialog(this)) submit.call(this); };
  // Gives the keystrokes back to the workspace after a click in a text field.
  addEventListener("keydown", e => e.isTrusted && e.key === "Escape" && post({ laura: "esc" }), true);
  const style = document.createElement("style");
  style.textContent = "[data-laura-th] { outline: 1.5px dashed #9aa2ea; outline-offset: 2px; }"
    + " [data-laura-hl] { outline: 2px solid #ebcb8b; background: #ebcb8b22; outline-offset: 2px; }"
    + " [data-laura-cur] { outline: 1.5px solid #4f5bd5; outline-offset: 2px; }"
    + " [data-laura-cm], [data-laura-cm] * { cursor: crosshair !important; }"
    + " [data-laura-hov] { outline: 2px solid #4f5bd5; background: #4f5bd510; outline-offset: 2px; }";
  (document.head ?? document.documentElement).append(style);
  let last, lastHl;
  addEventListener("message", e => {
    if (e.source !== parent || typeof e.data !== "object" || !e.data) return;
    const { cursor, threads, highlight } = e.data;
    comment = !!e.data.comment;
    document.documentElement.toggleAttribute("data-laura-cm", comment);
    if (!comment && hov) hover(null);
    for (const el of document.querySelectorAll("[data-laura-cur], [data-laura-th], [data-laura-hl]")) {
      el.removeAttribute("data-laura-cur");
      el.removeAttribute("data-laura-th");
      el.removeAttribute("data-laura-hl");
    }
    const tagged = [...document.querySelectorAll("[data-laura-line]")];
    const line = el => +el.dataset.lauraLine;
    // The first element that starts on line `n`, else on the nearest line above it.
    const at = n => {
      const top = tagged.reduce((m, el) => (line(el) <= n ? Math.max(m, line(el)) : m), 0);
      return tagged.find(el => line(el) === top);
    };
    pins = [];
    pinLayer.replaceChildren();
    for (const t of threads ?? []) {
      const el = at(t.line);
      if (!el) continue;
      el.setAttribute("data-laura-th", "");
      const pin = document.createElement("div");
      pin.dataset.line = t.line;
      pin.title = "L" + t.line;
      pin.style.cssText = "all: initial; position: fixed; pointer-events: auto; cursor: pointer; display: flex;"
        + " align-items: center; gap: 3px; padding: 2px 6px 2px 2px; border-radius: 12px 12px 12px 2px;"
        + " background: #fff; box-shadow: 0 1px 2px rgb(16 24 40 / .08), 0 4px 14px rgb(16 24 40 / .14);"
        + " font: 600 11px system-ui, sans-serif; color: #686d78;";
      for (const [i, [initial, agent]] of (t.who ?? []).entries()) {
        const av = document.createElement("span");
        av.textContent = initial;
        av.style.cssText = "all: initial; width: 20px; height: 20px; display: grid; place-items: center;"
          + ` border-radius: 50%; font: 600 10px system-ui, sans-serif; color: #fff; background: ${agent ? "#4f5bd5" : "#3d4250"};`
          + (i ? " margin-left: -6px; box-shadow: 0 0 0 2px #fff;" : "");
        pin.append(av);
      }
      pin.append(String(t.n));
      pinLayer.append(pin);
      pins.push([el, pin]);
    }
    // ponytail: a start-tag approximation of the highlighted lines; an element that starts above
    // the range isn't marked when another element starts inside it.
    let hl = [];
    if (highlight) {
      const [lo, hi] = highlight;
      const inside = tagged.filter(el => line(el) >= lo && line(el) <= hi);
      const set = new Set(inside);
      const nested = el => !!el.parentElement && (set.has(el.parentElement) || nested(el.parentElement));
      hl = inside.filter(el => !nested(el));
      if (!hl.length && at(lo)) hl = [at(lo)];
      for (const el of hl) el.setAttribute("data-laura-hl", "");
    }
    cur = cursor == null ? null : real(at(cursor));
    cur?.setAttribute("data-laura-cur", "");
    if (cursor !== last) cur?.scrollIntoView({ block: "nearest" });
    last = cursor;
    // `scroll`: a click on the pane header's highlight chip.
    if (e.data.scroll || String(highlight) !== lastHl) hl[0]?.scrollIntoView({ block: "center" });
    lastHl = String(highlight);
    // The panel's cursor line, even when the pane doesn't have the focus.
    anchor = Number.isInteger(e.data.line) ? real(at(e.data.line)) : null;
    sync();
  });
})();
