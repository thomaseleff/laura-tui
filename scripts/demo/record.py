# /// script
# requires-python = ">=3.12"
# dependencies = ["playwright==1.63.0", "imageio-ffmpeg==0.6.0"]
# ///
"""Record the README demo from its scene file. This script works on Windows only.

    uv run scripts/demo/record.py scripts/demo/scenes/demo.py

The script writes demo.mp4, demo.gif and the shots of the scene to tmp/demo/demo/. Refer to
.claude/skills/record-demo/SKILL.md.
"""

import argparse
import asyncio
import base64
import difflib
import importlib.util
import json
import os
import shutil
import socket
import subprocess
import sys
import time
import traceback
import urllib.request
from pathlib import Path

import imageio_ffmpeg
from playwright.async_api import async_playwright

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]

# This script draws the pointer and the keystroke box, because headless Chromium does not draw a
# pointer. The script adds elements and styles through the DOM and the CSSOM, because the CSP of the
# web app allows them. The script runs in the top frame only, because the frame of an HTML page also
# gets the init script.
OVERLAY = """addEventListener("DOMContentLoaded", () => {
  if (window !== window.top) return;
  const NS = "http://www.w3.org/2000/svg";
  const ptr = document.createElementNS(NS, "svg"), path = document.createElementNS(NS, "path");
  for (const [k, v] of [["width", 20], ["height", 28], ["viewBox", "0 0 20 28"]]) ptr.setAttribute(k, v);
  for (const [k, v] of [["d", "M1 1V22L6.5 16.8L10.3 26L14 24.4L10.2 15.4H17.5Z"], ["fill", "#111"],
    ["stroke", "white"], ["stroke-width", 1.5], ["stroke-linejoin", "round"]]) path.setAttribute(k, v);
  ptr.append(path);
  Object.assign(ptr.style, {position: "fixed", left: "0", top: "0", zIndex: 2147483647, pointerEvents: "none",
    opacity: "0", transition: "opacity .3s"});
  const keys = document.createElement("div");
  Object.assign(keys.style, {position: "fixed", right: "16px", bottom: "44px", padding: "8px 14px", borderRadius: "8px",
    background: "rgba(20,20,24,.88)", color: "white", font: "600 18px Consolas, ui-monospace, monospace",
    zIndex: 2147483646, pointerEvents: "none", opacity: "0", transition: "opacity .3s"});
  // This bar shows the time until the next action, along the top edge.
  const bar = document.createElement("div");
  Object.assign(bar.style, {position: "fixed", left: "0", top: "0", height: "3px", width: "0", background: "#4f5bd5",
    zIndex: 2147483646, pointerEvents: "none"});
  document.body.append(ptr, keys, bar);
  const show = el => {
    el.style.opacity = "1";
    clearTimeout(el.fade);
    el.fade = setTimeout(() => (el.style.opacity = "0"), 1500);
  };
  window.__ptr = (x, y) => { ptr.style.left = x - 1 + "px"; ptr.style.top = y - 1 + "px"; show(ptr); };
  window.__keys = t => { keys.textContent = t; show(keys); };
  window.__wait = ms => {
    Object.assign(bar.style, {transition: "none", width: "100%"});
    void bar.offsetWidth; // This line starts the transition at full width.
    Object.assign(bar.style, {transition: `width ${ms}ms linear`, width: "0"});
  };
});"""

BOXES = ".draft textarea, .pop.review textarea"
TYPE_DELAY = 75
BEAT = 600
LABELS = {"Control+p": "Ctrl+P", "Control+l": "Ctrl+L", "Escape": "Esc", "ArrowDown": "↓", "ArrowUp": "↑"}


class Agent:
    def __init__(self, port, repo, page):
        self.port, self.repo, self.page, self.seen = port, repo, page, 0

    def _req(self, path, body=None):
        data = json.dumps(body).encode() if body is not None else None
        req = urllib.request.Request(f"http://127.0.0.1:{self.port}{path}", data=data)
        # Use a long timeout, because Laura holds a command while the user types a comment.
        return json.loads(urllib.request.urlopen(req, timeout=120).read())

    async def _call(self, path, body=None):
        return await asyncio.to_thread(self._req, path, body)

    async def say(self, text):
        await self._call("/say", {"text": text})
        # Hold the reply for approximately 480 words a minute, so that the viewer can skim the reply. The bar shows the time that remains.
        ms = max(1000, 125 * len(text.split()))
        await self.page.evaluate("ms => window.__wait(ms)", ms)
        await self.page.wait_for_timeout(ms)

    async def laura(self, *args, stdin=(), shown=None):
        r = await self._call("/run", {"args": [str(a) for a in args], "stdin": list(stdin), "shown": shown})
        if r["code"]:
            raise RuntimeError(f"laura {' '.join(map(str, args))} exited {r['code']}: {r['err']}")
        await asyncio.sleep(BEAT / 1000)
        return r["out"]

    async def wait_for_review(self, timeout=30):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            reviews = (await self._call("/"))["reviews"]
            if len(reviews) > self.seen:
                self.seen = len(reviews)
                return reviews[-1]
            await asyncio.sleep(0.2)
        raise TimeoutError("no review reached the chat")

    async def write(self, path, text):
        f = self.repo / path
        old = f.read_text(encoding="utf-8").splitlines() if f.exists() else None
        f.write_text(text, encoding="utf-8")
        new = text.splitlines()
        if old is None:
            tool, out = "Write", f"Wrote {len(new)} lines to {path}"
        else:
            diff = list(difflib.ndiff(old, new))
            add, rem = (sum(ln.startswith(m) for ln in diff) for m in ("+ ", "- "))
            tool = "Update"
            out = f"Updated {path} with {add} addition{'s' * (add != 1)} and {rem} removal{'s' * (rem != 1)}"
        await self._call("/tool", {"name": tool, "arg": path, "out": out})
        await asyncio.sleep(BEAT / 1000)

    async def edit(self, path, old, new):
        text = (self.repo / path).read_text(encoding="utf-8")
        if old not in text:
            raise ValueError(f"{old!r} is not in {path}")
        await self.write(path, text.replace(old, new, 1))


class User:
    def __init__(self, page):
        self.page, self.ptr = page, (640, 400)

    async def click(self, locator):
        if isinstance(locator, str):
            locator = self.page.locator(locator).first
        await locator.wait_for(state="visible")
        await locator.scroll_into_view_if_needed()
        box = await locator.bounding_box()
        x, y = box["x"] + min(box["width"] / 2, 160), box["y"] + box["height"] / 2
        # The overlay moves the pointer, because mouse events over the frame of a page do not go to the top frame.
        (x0, y0), steps = self.ptr, 25
        for i in range(1, steps + 1):
            xi, yi = x0 + (x - x0) * i / steps, y0 + (y - y0) * i / steps
            await self.page.mouse.move(xi, yi)
            await self.page.evaluate("([x, y]) => window.__ptr(x, y)", [xi, yi])
            await self.page.wait_for_timeout(15)
        self.ptr = (x, y)
        await self.page.mouse.click(x, y)

    async def focused(self, pane):
        # Laura moves the focus. A keystroke before the focus moves goes to the pane that had the focus.
        await self.page.locator(f"#p{pane}.focused").wait_for()

    async def focus(self, pane):
        if await self.page.locator(f"#p{pane}.focused").count():
            return
        if str(pane) == "0" and not await self.page.locator(".pane.focused .xterm:visible").count():
            # On a file pane, `Esc` moves the focus to the shell.
            await self.press("Escape")
        else:
            # The pane selector shows for one beat. When the digits match one pane, the focus moves
            # to that pane immediately.
            await self.press("Control+p")
            await self.type(str(pane), keys=True)
        await self.focused(pane)

    async def goto_line(self, pane, line):
        await self.focus(pane)
        await self.page.locator(f"#p{pane} .rows[data-key]").first.wait_for()
        rows = self.page.locator(f"#p{pane} .rows").first

        def at(key):
            # The key is `<idx>:<line>`. On the thread of a line, the key is `<idx>:L<line>`. The thread is immediately below the line.
            ln = key.split(":", 1)[1]
            return int(ln[1:]) + 0.5 if ln.startswith("L") else int(ln)

        key, last, n = await rows.get_attribute("data-key"), None, 0
        # The open thread of a line counts as the line. The web app puts the cursor on the thread, and `c` adds a reply to the thread.
        while at(key) not in (line, line + 0.5):
            way = "ArrowDown" if at(key) < line else "ArrowUp"
            if n == 60 or last not in (None, way):
                raise RuntimeError(f"the cursor in pane {pane} cannot reach L{line}")
            last, n = way, n + 1
            await self.keys(f"{LABELS[way]} ×{n}")
            await self.page.keyboard.press(way)
            await self.page.locator(f'#p{pane} .rows:not([data-key="{key}"])').first.wait_for()
            key = await rows.get_attribute("data-key")
            await self.page.wait_for_timeout(100)
        await self.page.wait_for_timeout(BEAT)

    async def keys(self, label):
        await self.page.evaluate("t => window.__keys(t)", label)

    async def press(self, key):
        await self.keys(LABELS.get(key, key))
        boxes = self.page.locator(BOXES)
        was_open = await boxes.count() > 0
        await self.page.keyboard.press(key)
        if not was_open and key in ("c", "Shift+S"):
            # The box gets the focus after one beat. If the user types before that, the box does not
            # get the first letter. On a page with no element at the cursor, `c` turns on comment mode.
            await self.page.locator(f"{BOXES}, .cmhint").filter(visible=True).first.wait_for()
        elif was_open and key == "Enter":
            await self.page.wait_for_selector(BOXES, state="detached")
        await self.page.wait_for_timeout(BEAT)

    async def type(self, text, keys=False):
        if keys:
            await self.keys(text)
        await self.page.keyboard.type(text, delay=TYPE_DELAY)
        if keys:
            await self.page.wait_for_timeout(BEAT)


class Camera:
    def __init__(self, page, out):
        self.page, self.out = page, out

    async def shot(self, name):
        await self.page.screenshot(path=str(self.out / f"{name}.png"))

    async def pause(self, ms):
        await self.page.wait_for_timeout(ms)


def rmtree(path):
    # Git object files are read-only on Windows.
    def onexc(func, p, _):
        os.chmod(p, 0o700)
        func(p)

    if path.exists():
        shutil.rmtree(path, onexc=onexc)


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def load_scene(path):
    spec = importlib.util.spec_from_file_location(path.stem, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def make_home(scene, scene_path, home):
    repo = home / "acme-api"
    fixtures = getattr(scene, "FIXTURES", None)
    if fixtures:
        shutil.copytree(scene_path.parent / fixtures, repo)
    else:
        repo.mkdir(parents=True)
    (home / ".laura").mkdir()
    (home / ".laura" / "editor-theme").write_text("laura")
    for a in (["init", "-q"], ["add", "."], ["commit", "-qm", "init", "--allow-empty"]):
        git = ["git", "-c", "user.name=demo", "-c", "user.email=demo@example.com", *a]
        subprocess.run(git, cwd=repo, check=True, capture_output=True)
    return repo


def start_laura(laura, repo, home, port, log):
    env = {
        **os.environ,
        "LAURA_EXPERIMENTAL_WEB": "1",
        "LAURA_EXPERIMENTAL_EDITOR": "1",
        "LAURA_DEMO_PORT": str(port),
        "LAURA_DEMO_BIN": str(laura),
        "USERPROFILE": str(home),
        "HOME": str(home),
        **{k: str(home / ".xdg" / k) for k in ("XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME")},
    }
    proc = subprocess.Popen(
        [str(laura), "--web", "--", sys._base_executable, str(HERE / "chat.py")],
        cwd=repo,
        env=env,
        stdout=log.open("w"),
        stderr=subprocess.STDOUT,
    )
    end = time.monotonic() + 15
    while time.monotonic() < end and proc.poll() is None:
        text = log.read_text(encoding="utf-8", errors="replace")
        url = next((ln.strip() for ln in text.splitlines() if ln.startswith("http")), None)
        if url:
            return proc, url
        time.sleep(0.2)
    proc.kill()
    raise RuntimeError(f"Laura printed no URL:\n{log.read_text(encoding='utf-8', errors='replace')}")


def encode(frames_dir, out):
    ff = imageio_ffmpeg.get_ffmpeg_exe()
    src = ["-f", "concat", "-i", str(frames_dir / "list.txt"), "-fps_mode", "vfr"]
    run = [ff, "-y", "-loglevel", "error", *src]
    subprocess.run([*run, "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "20", str(out / "demo.mp4")], check=True)
    # Do not use dither. The flat colors of the web app fit in the palette, and dither noise adds bytes to each frame.
    gif = "fps=12,mpdecimate,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=none:diff_mode=rectangle"
    subprocess.run([*run, "-vf", gif, str(out / "demo.gif")], check=True)


async def record(scene, page, cdp, url, port, repo, out):
    frames_dir = out / "frames"
    frames_dir.mkdir()
    frames = []

    def on_frame(f):
        frames.append(f["metadata"]["timestamp"])
        (frames_dir / f"{len(frames) - 1:05}.png").write_bytes(base64.b64decode(f["data"]))
        asyncio.ensure_future(cdp.send("Page.screencastFrameAck", {"sessionId": f["sessionId"]}))

    errors = []
    page.on("pageerror", lambda e: errors.append(str(e)))
    page.on(
        "console",
        lambda m: m.type == "error" and not m.location.get("url", "").endswith("/favicon.ico") and errors.append(m.text),
    )
    cdp.on("Page.screencastFrame", on_frame)
    await cdp.send("Page.startScreencast", {"format": "png", "maxWidth": 1280, "maxHeight": 800})
    await page.goto(url)
    await page.wait_for_selector("#p0 .xterm", timeout=15000)
    agent = Agent(port, repo, page)
    end = time.monotonic() + 10
    while not (await agent._call("/"))["ready"]:
        if time.monotonic() > end:
            raise TimeoutError("the chat stub never showed its prompt")
        await asyncio.sleep(0.2)
    page.set_default_timeout(10000)
    await scene.scene(User(page), agent, Camera(page, out))
    await page.wait_for_timeout(1500)
    stop = time.time()
    await cdp.send("Page.stopScreencast")
    if errors:
        raise RuntimeError("page errors:\n" + "\n".join(errors))
    if not frames:
        raise RuntimeError("the screencast sent no frames")
    lines = []
    for i, ts in enumerate(frames):
        nxt = frames[i + 1] if i + 1 < len(frames) else stop
        lines += [f"file '{i:05}.png'", f"duration {max(nxt - ts, 0.001):.3f}"]
    # The concat demuxer ignores the duration of the last entry, unless the list shows the file again.
    lines.append(f"file '{len(frames) - 1:05}.png'")
    (frames_dir / "list.txt").write_text("\n".join(lines) + "\n")


async def main():
    ap = argparse.ArgumentParser(description="Record the README demo from its scene file.")
    ap.add_argument("scene", type=Path)
    ap.add_argument("--laura", type=Path, default=ROOT / "tmp/target/debug/laura.exe")
    args = ap.parse_args()
    scene_path, laura = args.scene.resolve(), args.laura.resolve()
    scene = load_scene(scene_path)
    out = ROOT / "tmp/demo" / scene_path.stem
    rmtree(out)
    home = out / "home"
    repo = make_home(scene, scene_path, home)
    port = free_port()
    proc, url = start_laura(laura, repo, home, port, out / "laura.log")
    try:
        async with async_playwright() as p:
            browser = await p.chromium.launch(channel="chromium")
            ctx = await browser.new_context(viewport={"width": 1280, "height": 800}, device_scale_factor=1)
            await ctx.add_init_script(OVERLAY)
            page = await ctx.new_page()
            cdp = await ctx.new_cdp_session(page)
            try:
                await record(scene, page, cdp, url, port, repo, out)
            except Exception:
                traceback.print_exc()
                await page.screenshot(path=str(out / "failure.png"))
                print(f"FAILED: see {out / 'failure.png'}", file=sys.stderr)
                return 1
            finally:
                await browser.close()
    finally:
        subprocess.run(["taskkill", "/F", "/T", "/PID", str(proc.pid)], capture_output=True)
    encode(out / "frames", out)
    print(f"Wrote {out / 'demo.mp4'} and {out / 'demo.gif'}")
    return 0


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
