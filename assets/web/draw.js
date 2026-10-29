// What Laura draws on a page, shared by an HTML file pane's page (page.js) and a browser pane's page
// (browser.js): the outlines, the tags above the hovered and the cursor's elements, each thread's
// pin, and the cursor's element's box, which the workspace floats the cursor's thread beside.
// `label(el)` is an element's tag text, `pick(target)` the element a hover in comment mode outlines,
// `onRect(box)` takes the cursor's element's box, and `onPin(line)` a click on a thread's pin.
// Returns `apply`, which draws one state.
const lauraDraw = ({ label, pick, onRect, onPin }) => {
  let comment = false, hov = null, anchor = null, cur = null, pins = [], raf = 0;
  // The hovered element's and the cursor's element's name, above their outlines.
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
    t.textContent = label(el);
    t.style.left = h.left - 4 + "px";
    t.style.top = Math.max(0, h.top - 20) + "px";
  };
  // Each thread's pin: its authors and comment count, at its element's top-right corner. A click on
  // a pin opens its thread and reaches none of the page's handlers.
  const pinLayer = document.createElement("div");
  pinLayer.setAttribute("data-laura-pin", "");
  pinLayer.style.cssText = "all: initial; position: fixed; inset: 0; z-index: 2147483646; pointer-events: none;";
  document.documentElement.append(pinLayer);
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
    onRect(box(anchor));
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
  addEventListener("click", e => {
    const pin = e.isTrusted && pinOf(e.target);
    if (!pin) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    onPin(+pin.dataset.line);
  }, true);
  const style = document.createElement("style");
  style.textContent = "[data-laura-th] { outline: 1.5px dashed #9aa2ea; outline-offset: 2px; }"
    + " [data-laura-hl] { outline: 2px solid #ebcb8b; background: #ebcb8b22; outline-offset: 2px; }"
    + " [data-laura-cur] { outline: 1.5px solid #4f5bd5; outline-offset: 2px; }"
    + " [data-laura-cm], [data-laura-cm] * { cursor: crosshair !important; }"
    + " [data-laura-hov] { outline: 2px solid #4f5bd5; background: #4f5bd510; outline-offset: 2px; }";
  (document.head ?? document.documentElement).append(style);
  // `threads`: `[element, {line, n, who, tip}]` pairs; `cur` the cursor's element, outlined;
  // `anchor` the element whose box `onRect` takes; `hl` the highlighted elements.
  return state => {
    comment = state.comment;
    document.documentElement.toggleAttribute("data-laura-cm", comment);
    if (!comment && hov) hover(null);
    for (const el of document.querySelectorAll("[data-laura-cur], [data-laura-th], [data-laura-hl]")) {
      el.removeAttribute("data-laura-cur");
      el.removeAttribute("data-laura-th");
      el.removeAttribute("data-laura-hl");
    }
    pins = [];
    pinLayer.replaceChildren();
    for (const [el, t] of state.threads) {
      el.setAttribute("data-laura-th", "");
      const pin = document.createElement("div");
      pin.dataset.line = t.line;
      if (t.tip) pin.title = t.tip;
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
    for (const el of state.hl) el.setAttribute("data-laura-hl", "");
    cur = state.cur;
    cur?.setAttribute("data-laura-cur", "");
    anchor = state.anchor;
    sync();
  };
};
