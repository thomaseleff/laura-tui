"""The chat stub for the demo. Laura runs this script as the shell program. record.py controls
the chat stub over HTTP.

The chat stub looks like Claude Code in auto mode. Each change draws the full screen again from the
model, so that the text wraps again when the width of the shell changes.

This script works on Linux. The script sets the terminal to raw input, so that a submitted inline
review comes in as one bracketed paste.
"""

import http.server
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import textwrap
import threading
import time
import tty

PORT = int(os.environ["LAURA_DEMO_PORT"])
BIN = os.environ["LAURA_DEMO_BIN"]
HOME = os.path.expanduser("~")
DIM, BOLD, RST = "\x1b[2m", "\x1b[1m", "\x1b[0m"
ORANGE, GREEN, YELLOW = "\x1b[38;5;173m", "\x1b[32m", "\x1b[38;5;214m"
CSI = re.compile(rb"\x1b\[[0-?]*[ -/]*[@-~]")
lock = threading.RLock()
state = {"ready": False, "reviews": []}
# Each entry is [kind, text, out]. The kind is user, agent or tool. Out is the output of a tool, or None until the tool stops.
entries = []
# top is the first chat line on screen while a long reply scrolls, or None to show the end of the chat.
ui = {"inp": "", "paste": None, "pastes": 0, "top": None}

# Raw input keeps the paste markers, the line breaks of a review, and `\r` for Enter. draw() writes `\r\n` itself.
tty.setraw(0)
sys.stdout.reconfigure(encoding="utf-8")


def wrap(text, width, first, rest):
    # Each source line wraps separately and keeps its indent, so that the thread lines of a review stay aligned.
    out = []
    for src in text.split("\n"):
        indent = rest + src[: len(src) - len(src.lstrip())]
        out += textwrap.wrap(src, max(width, 10), initial_indent=first, subsequent_indent=indent) or [first]
        first = rest
    return out


def render(width):
    # Returns the chat lines, the footer, and the first line of the last agent reply.
    reply = None
    body = [f" {ORANGE}✻{RST} Welcome to Claude Code!", "", f"   cwd: ~/{os.path.basename(os.getcwd())}"]
    n = len(" ✻ Welcome to Claude Code!") + 2
    lines = ["╭" + "─" * n + "╮"]
    lines += ["│" + b + " " * (n - len(re.sub(r"\x1b\[[\d;]*m", "", b))) + "│" for b in body]
    lines.append("╰" + "─" * n + "╯")
    for kind, text, out in entries:
        lines.append("")
        if kind == "user":
            lines += [DIM + ln + RST for ln in wrap(text, width, "> ", "  ")]
        elif kind == "agent":
            reply = len(lines)
            lines += wrap(text, width, "● ", "  ")
        else:
            head = wrap(text, width, "● ", "  ")
            name, paren, arg = head[0][2:].partition("(")
            lines += [f"{GREEN}●{RST} {BOLD}{name}{RST}{paren}{arg}", *head[1:]]
            shown = ["Running…"] if out is None else out.split("\n")
            if len(shown) > 3:
                shown = [*shown[:3], f"… +{len(shown) - 3} lines"]
            for i, ln in enumerate(shown):
                lines += [DIM + w + RST for w in wrap(ln, width, "  ⎿  " if i == 0 else "     ", "     ")]
    inp = ui["inp"]
    if ui["paste"] is not None:
        inp = f"[Pasted text #{ui['pastes']} +{ui['paste'].count(chr(10)) + 1} lines]{inp}"
    field = wrap(inp, width - 1, "> ", "  ")
    # textwrap removes the spaces at the end of the input. This line adds the spaces again.
    field[-1] += " " * (len(inp) - len(inp.rstrip(" "))) + "\x1b[7m \x1b[27m"
    rule = DIM + "─" * width + RST
    auto = f"  {YELLOW}⏵⏵ auto mode on{RST}{DIM} (shift+tab to cycle){RST}"
    return lines, ["", rule, *field, rule, auto, ""], reply


def view():
    cols, rows = shutil.get_terminal_size()
    # Use one column less than the width, so that a full line does not wrap.
    lines, footer, reply = render(cols - 1)
    return lines, footer, reply, max(rows - len(footer), 0)


def draw():
    with lock:
        chat, footer, _, room = view()
        # The footer stays at the bottom. Above it, show the chat up to the end, or up to the scroll position of a long reply.
        end = len(chat) if ui["top"] is None else min(ui["top"] + room, len(chat))
        lines = [*chat[max(end - room, 0) : end], *footer]
        sys.stdout.write("\x1b[H" + "\x1b[K\r\n".join(lines) + "\x1b[K\x1b[J")
        sys.stdout.flush()


def add(kind, text, out=None):
    entry = [kind, text, out]
    with lock:
        entries.append(entry)
        ui["top"] = None
        draw()
    return entry


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def reply(self, obj):
        b = json.dumps(obj).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def do_GET(self):
        self.reply(state)

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if self.path == "/say":
            with lock:
                entries.append(["agent", body["text"], None])
                chat, _, reply, room = view()
                # A reply taller than the drawer shows from its first line. record.py shows the end with /scroll.
                below = len(chat) - reply - room
                ui["top"] = reply if below > 0 else None
                draw()
            self.reply({"below": max(below, 0)})
        elif self.path == "/scroll":
            with lock:
                ui["top"] = None
                draw()
            self.reply({})
        elif self.path == "/ask":
            # A question that needs the user: the bell marks the workspace as waiting.
            add("agent", body["text"])
            with lock:
                sys.stdout.write("\a")
                sys.stdout.flush()
            self.reply({})
        elif self.path == "/tool":
            add("tool", f"{body['name']}({body['arg']})", body["out"])
            self.reply({})
        elif self.path == "/run":
            args, pipe = body["args"], body.get("pipe")
            cmd = "laura " + " ".join(args)
            entry = add("tool", f"Bash({body.get('shown') or (f'{pipe} | {cmd}' if pipe else cmd)})")
            # A pipe runs a real command, so that a tail pane shows the command's output as it runs.
            run = ["sh", "-c", f"{pipe} | {shlex.join([BIN, *args])}"] if pipe else [BIN, *args]
            p = subprocess.run(run, stdin=subprocess.DEVNULL, capture_output=True)
            out, err = p.stdout.decode().strip(), p.stderr.decode(errors="replace").strip()
            with lock:
                entry[2] = err if p.returncode else out or "(No content)"
                draw()
            self.reply({"code": p.returncode, "out": out, "err": err})
        else:
            self.send_error(404)


def clean(text):
    # The review header shows the absolute path of the file. Show the scratch home as `~`.
    return text.replace(HOME, "~").rstrip("\n")


def watch():
    # Read the size at an interval instead of on SIGWINCH, so that a redraw never runs inside a draw of the main loop.
    size = None
    while True:
        if (now := shutil.get_terminal_size()) != size:
            size = now
            draw()
        time.sleep(0.2)


threading.Thread(
    target=http.server.ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever,
    daemon=True,
).start()
sys.stdout.write("\x1b[?2004h\x1b[?25l")
threading.Thread(target=watch, daemon=True).start()
state["ready"] = True

buf, paste = b"", None
while chunk := os.read(0, 4096):
    buf += chunk
    while buf:
        if paste is not None:
            end = buf.find(b"\x1b[201~")
            if end < 0:
                paste, buf = paste + buf, b""
                break
            text = (paste + buf[:end]).decode("utf-8", "replace").replace("\r\n", "\n").replace("\r", "\n")
            buf, paste = buf[end + 6 :], None
            with lock:
                ui["paste"], ui["pastes"] = text, ui["pastes"] + 1
                draw()
            continue
        if buf.startswith(b"\x1b[200~"):
            paste, buf = b"", buf[6:]
            continue
        if buf.startswith(b"\x1b["):
            m = CSI.match(buf)
            if not m:
                break
            buf = buf[m.end() :]
            continue
        c, buf = buf[:1], buf[1:]
        with lock:
            # Laura sends `\r` after the paste of a submitted inline review.
            if c == b"\r" and ui["paste"] is not None:
                state["reviews"].append(ui["paste"])
                entries.append(["user", clean(ui["paste"]), None])
                ui["paste"] = None
            elif c == b"\r" and ui["inp"]:
                entries.append(["user", ui["inp"], None])
                ui["inp"] = ""
            elif c == b"\x7f":
                ui["inp"] = ui["inp"][:-1]
            elif c >= b" ":
                ui["inp"] += c.decode("latin-1")
            ui["top"] = None
            draw()
