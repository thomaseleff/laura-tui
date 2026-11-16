// The Laura server's page: the list of the running workspaces, and the form that starts one.
// Every text from Laura (a session label, a folder, an error) goes in as text, never as HTML.
const $ = id => document.getElementById(id);
// A browser can block storage; the page then starts at `~` and Shell.
const store = {
  get: k => { try { return localStorage.getItem(k); } catch { return null; } },
  set: (k, v) => { try { localStorage.setItem(k, v); return true; } catch { return false; } },
};
// The key comes in the printed address once; the page keeps it for this address only.
const given = new URLSearchParams(location.search).get("t");
// Blocked storage: the key stays in the address, so a reload still has the key.
if (given && store.set("laura-key", given)) history.replaceState(null, "", "/");
const key = given ?? store.get("laura-key") ?? "";
const api = (url, opts = {}) => fetch(url, { ...opts, headers: { "X-Laura-Key": key } });
const el = (tag, cls, text) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
};
const label = r => r.title ?? r.root.split(/[\\/]/).filter(Boolean).at(-1) ?? r.root;

let list = [], sent = "", known = null;

// One of the three screens: the list, how to start Laura (no key), or the stopped card.
function show(screen) {
  $("signed").hidden = $("footline").hidden = screen !== "signed";
  $("gate").hidden = screen !== "gate";
  $("stopped").hidden = screen !== "stopped";
}

async function load() {
  let res;
  try {
    res = await api("/workspaces");
  } catch {
    return show("stopped");
  }
  if (!res.ok) return show("gate");
  $("version").textContent = `Laura ${res.headers.get("X-Laura-Version") ?? ""}`;
  const text = await res.text();
  show("signed");
  if (text === sent) return;
  sent = text;
  list = JSON.parse(text);
  paintList();
}

function paintList() {
  const rows = list.map(r => {
    const li = el("li", known && !known.has(r.url) ? "sv-new" : "");
    const names = el("div", "sv-names");
    names.append(el("div", r.title ? "sv-t" : "sv-t sv-fallback", label(r)), el("div", "sv-p", `${r.root} · ${r.program}`));
    const open = el("span", "sv-open");
    const here = el("button", "btn", "Open"), tab = el("button", "btn", "↗");
    tab.ariaLabel = tab.dataset.tip = "Open in a new tab";
    here.onclick = () => location.assign(r.url);
    tab.onclick = () => window.open(r.url, "_blank", "noopener");
    open.append(here, tab);
    li.append(el("span", "sv-dot"), names, open);
    return li;
  });
  known = new Set(list.map(r => r.url));
  $("rows").replaceChildren(...(rows.length ? rows : [el("li", "sv-empty", "No workspaces are running. Start one above.")]));
  $("count").textContent = list.length ? `(${list.length})` : "";
}

// The folder browser: `cwd` is the folder it lists, and `sel` is the selected row's folder.
// `loading` is the last `nav`: Use this folder waits for it and uses the folder that shows.
let cwd = null, sel = null, loading = null;
const FOLDER = '<svg class="ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"><path d="M3 6.5A1.5 1.5 0 0 1 4.5 5H9l2 2.5h8.5A1.5 1.5 0 0 1 21 9v9.5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18.5z"/></svg>';
const nav = path => (loading = listDirs(path));
async function listDirs(path) {
  let res = await api(`/dirs?path=${encodeURIComponent(path)}`);
  if (!res.ok) res = await api("/dirs?path=");
  if (!res.ok) return;
  cwd = await res.json();
  sel = null;
  $("crumbs").replaceChildren(...cwd.crumbs.flatMap((c, i) => {
    const a = el("a", "", c.name);
    a.href = "#";
    a.onclick = e => (e.preventDefault(), nav(c.path));
    return i ? [" / ", a] : [a];
  }));
  $("dirs").replaceChildren(...(cwd.dirs.length ? cwd.dirs.map(d => {
    // An SVG, not 📁: Laura's browser and some systems have no emoji font.
    const li = el("li");
    li.innerHTML = FOLDER;
    li.append(d.name);
    li.setAttribute("role", "option");
    li.dataset.path = d.path;
    if (d.git) li.append(el("span", "sv-git", "git"));
    return li;
  }) : [el("li", "sv-none mute", "No folders here.")]));
}
function select(li) {
  if (!li) return;
  const old = $("dirs").querySelector(".sel");
  old?.classList.remove("sel");
  old?.removeAttribute("aria-selected");
  li.classList.add("sel");
  li.setAttribute("aria-selected", "true");
  li.scrollIntoView({ block: "nearest" });
  sel = li.dataset.path;
}
function browse(open) {
  $("fb").hidden = !open;
  $("browse").setAttribute("aria-expanded", String(open));
  if (open) nav($("folder").value.trim());
}
$("browse").onclick = () => browse($("fb").hidden);
$("fbCancel").onclick = () => browse(false);
$("fbUse").onclick = async () => {
  // The path as Laura shows it (`~/…`).
  await loading;
  if (sel) await nav(sel);
  if (cwd) $("folder").value = cwd.shown;
  browse(false);
};
$("dirs").onclick = e => select(e.target.closest("li[data-path]"));
$("dirs").ondblclick = e => {
  const li = e.target.closest("li[data-path]");
  if (li) nav(li.dataset.path);
};
$("dirs").onkeydown = e => {
  const rows = [...$("dirs").querySelectorAll("li[data-path]")], i = rows.findIndex(li => li.classList.contains("sel"));
  if (e.key === "Enter" && sel) nav(sel);
  else if (e.key === "ArrowDown") select(rows[Math.min(i + 1, rows.length - 1)]);
  else if (e.key === "ArrowUp") select(rows[Math.max(i - 1, 0)]);
  else return;
  e.preventDefault();
};

// Shell or Command: the page keeps the last choice.
const radios = [...document.querySelectorAll("[name=prog]")];
const program = () => (radios[0].checked ? "" : $("cmd").value.trim());
radios.forEach(r => r.onchange = () => { $("cmd").disabled = radios[0].checked; });

$("go").onclick = async () => {
  const folder = $("folder").value.trim();
  $("err").hidden = true;
  $("go").disabled = true;
  $("go").replaceChildren(el("span", "sv-spin"), "Starting…");
  let res;
  try {
    res = await api("/new", { method: "POST", body: JSON.stringify({ folder, program: program() }) });
  } catch {
    res = null;
  }
  if (res?.ok) {
    store.set("laura-folder", folder);
    store.set("laura-program", program() || "shell");
    return location.assign((await res.json()).url);
  }
  $("go").disabled = false;
  $("go").textContent = "Start workspace";
  if (!res || res.status === 401) return load();
  // The headline, then Laura's error (paths, program output) behind Show the error.
  const [head, ...rest] = (await res.text()).split("\n");
  $("err").replaceChildren(head);
  if (rest.length) {
    const more = el("details");
    more.append(el("summary", "", "Show the error"), el("pre", "", rest.join("\n")));
    $("err").append(more);
  }
  $("err").hidden = false;
};

const ask = on => { $("stopSrv").hidden = on; $("stopAsk").hidden = !on; };
$("stopSrv").onclick = () => ask(true);
$("stopNo").onclick = () => ask(false);
$("stopYes").onclick = async () => {
  ask(false);
  try { await api("/quit", { method: "POST" }); } catch {}
  show("stopped");
};

$("folder").value = store.get("laura-folder") || "~";
const last = store.get("laura-program");
if (last && last !== "shell") {
  radios[1].checked = true;
  $("cmd").value = last;
  $("cmd").disabled = false;
}
load();
addEventListener("focus", load);
setInterval(load, 5000);
