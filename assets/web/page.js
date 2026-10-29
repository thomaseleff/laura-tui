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
  let comment = false;
  // An element the user can pick: not the document's own `html`, `head` or `body`.
  const real = el => (el && !el.matches("html, body, head") ? el : null);
  const pick = t => real(t?.closest?.("[data-laura-line]"));
  // An element's name, as the workspace's thread labels read it.
  const label = el => `<${el.tagName.toLowerCase()}${el.id ? "#" + el.id : el.classList[0] ? "." + el.classList[0] : ""}>`;
  const apply = lauraDraw({
    label: el => `L${el.dataset.lauraLine} ${label(el)}`,
    pick,
    onRect: r => post(r ? { laura: "rect", top: r.top, left: r.left, bottom: r.bottom, right: r.right } : { laura: "rect" }),
    onPin: line => post({ laura: "line", line, pin: true }),
  });
  let down;
  addEventListener("mousedown", e => (down = [e.screenX, e.screenY]), true);
  addEventListener("click", e => {
    if (!e.isTrusted) return;
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
  let last, lastHl;
  addEventListener("message", e => {
    if (e.source !== parent || typeof e.data !== "object" || !e.data) return;
    const { cursor, threads, highlight } = e.data;
    comment = !!e.data.comment;
    const tagged = [...document.querySelectorAll("[data-laura-line]")];
    const line = el => +el.dataset.lauraLine;
    // The first element that starts on line `n`, else on the nearest line above it.
    const at = n => {
      const top = tagged.reduce((m, el) => (line(el) <= n ? Math.max(m, line(el)) : m), 0);
      return tagged.find(el => line(el) === top);
    };
    const drawn = [];
    for (const t of threads ?? []) {
      const el = at(t.line);
      if (el) drawn.push([el, { ...t, tip: "L" + t.line }]);
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
    }
    const cur = cursor == null ? null : real(at(cursor));
    if (cursor !== last) cur?.scrollIntoView({ block: "nearest" });
    last = cursor;
    // `scroll`: a click on the pane header's highlight chip.
    if (e.data.scroll || String(highlight) !== lastHl) hl[0]?.scrollIntoView({ block: "center" });
    lastHl = String(highlight);
    // The panel's cursor line, even when the pane doesn't have the focus.
    const anchor = Number.isInteger(e.data.line) ? real(at(e.data.line)) : null;
    apply({ threads: drawn, cur, anchor, hl, comment });
  });
})();
