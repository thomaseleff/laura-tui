"""The README demo. The scene follows the beats of plugins/laura/skills/demo/SKILL.md, and plays the
actions of the user.

Change this scene only when the user asks for a new README demo. Refer to the record-demo skill.
"""

import asyncio
import shlex
import socket
import subprocess
import sys
import time
import urllib.request

FIXTURES = "demo"

PLAN_REVISED = """# Plan: greet each visitor by name

Today the greeting API says `Hello, world!` to every visitor. This plan greets each visitor by name, then serves and logs the greeting, so we can debug it.

## Steps

1. `greet Ada` prints `Hello, Ada!`, and `greet` still prints `Hello, world!`.
2. The greeting is in English or Spanish: `--lang es` prints `Hola, Ada!`.
3. The API serves the greeting at `/hello?name=Ada`.
4. The API logs each request, with the name it greeted.
"""

HELLO_CHANGED = """GREETINGS = {"en": "Hello", "es": "Hola"}


def greet(name="world", lang="en"):
    return f"{GREETINGS[lang]}, {name or 'world'}!"
"""

# The debug pipeline of the demo skill: one subshell starts the greeting API, sends it requests and stops it.
API = f"""( {shlex.quote(sys.executable)} -u app.py & APP=$!
  until curl -s -o /dev/null http://127.0.0.1:8766/hello; do sleep 0.5; done
  for i in $(seq 1 6); do
    for q in "name=Ada" "name=Ada&lang=es" "name=Bo" "name=Cy&lang=fr"; do
      curl -s -o /dev/null "http://127.0.0.1:8766/hello?$q"; sleep 0.6
    done
  done
  kill $APP ) 2>&1"""


def serve(root):
    # The demo skill serves on 8765. Another free port keeps the scene running when 8765 is in use.
    with socket.socket() as s:
        try:
            s.bind(("127.0.0.1", 8765))
        except OSError:
            s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
    srv = subprocess.Popen(
        [sys.executable, "-m", "http.server", str(port), "--bind", "127.0.0.1", "--directory", str(root)],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    for _ in range(50):
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{port}/", timeout=1)
            return srv, port
        except OSError:
            if srv.poll() is not None:
                break
            time.sleep(0.1)
    srv.kill()
    raise RuntimeError(f"the web server on port {port} did not start")


async def reply(user, text="Next"):
    await user.focus(0)
    await user.type(text)
    await user.press("Enter")


async def setup(agent):
    # app.py binds 8766, which the GIF shows. With another program on 8766, curl gets that program's answers.
    with socket.socket() as s:
        try:
            s.bind(("127.0.0.1", 8766))
        except OSError:
            raise RuntimeError("port 8766 is in use; stop the program on it and record again") from None
    # Another workspace runs before the demo, so that the Laura server's page lists it.
    return await agent.workspace("hello-web", "greeting page")


async def scene(user, agent, camera, other):
    # The user starts the demo's workspace from the Laura server's page.
    await camera.pause(1500)
    await camera.shot("start")
    await user.click("#folder")
    await user.press("Control+a")
    await user.type("~/hello-api")
    await user.click(user.page.locator("[name=prog]").nth(1))
    await user.click("#go")
    await user.page.wait_for_selector("#p0 .xterm", timeout=15000)
    await agent.ready()
    await camera.pause(800)
    await user.type("/laura:demo")
    await user.press("Enter")
    await agent.laura("ready", "--session", "demo", "--agent", "claude", "--title", "Laura demo")
    await agent.laura("close", "--all")

    # What Laura is
    await agent.say(
        "Laura is a workspace that gives your agent an API over the UI, so your agent can tile panes "
        "while you pair-program.\n\n"
        "Each pane shows a file or the output of a running command, and you can work with your agent in every pane.\n\n"
        "In this demo, I'll walk you through the inline review loop, show how your agent lays out different "
        "workspaces, and show how to record feedback.\n\n"
        "Send Next to get started."
    )
    await camera.shot("canvas")
    await reply(user)

    # [1 / 7] The workspace is just panes
    # As in the demo skill's `$D`, the scratch files stay out of the repo, so the project at a glance doesn't list them.
    scratch = agent.repo.parent / "scratch"
    scratch.mkdir(exist_ok=True)
    prev = None
    for i, n in enumerate([1, 1, 2, 3, 5, 8]):
        (scratch / f"fib-{i}.txt").write_text(f"{n}\n")
        split = ["--split", prev, "--dir", "v" if i % 2 else "h"] if prev else []
        args = [*split, "--ratio", "62", "--no-focus"]
        prev = await agent.laura("open", str(scratch / f"fib-{i}.txt"), *args,
                                 shown=f"laura open $D/fib-{i}.txt {' '.join(args)}")
    await agent.say(
        "[1 / 7] The workspace is just panes\n\n"
        "Laura's workspace is just panes, so your agent can lay out almost anything. "
        "Here's a Fibonacci sequence, one number per pane.\n\n"
        "The panes fill the workspace, and this chat floats in a drawer over them. Press Ctrl+` to close "
        "the drawer and see every pane, and Ctrl+` again to come back here.\n\n"
        "New: Drag the line between two panes to resize them, and drag the drawer's top edge or press "
        "Ctrl+Shift+↑ ↓ to set its height. Laura keeps your sizes.\n\n"
        "Send Next to learn how to interact with your agent in a pane."
    )
    await user.press("Control+Backquote")
    await user.page.locator("#p0").wait_for(state="hidden")
    await camera.pause(1500)
    await camera.shot("panes")
    # The first line splits fib-0, on top, from the panes below. Laura keeps the dragged size.
    await user.drag("#ws > .split", 0, -110)
    await camera.pause(800)
    await camera.shot("resized")
    await user.press("Control+Backquote")
    await user.focused(0)
    await user.press("Control+Shift+ArrowUp")
    await user.press("Control+Shift+ArrowUp")
    await camera.pause(800)
    await camera.shot("drawer")
    await user.press("Control+Shift+ArrowDown")
    await user.press("Control+Shift+ArrowDown")
    await reply(user)
    await agent.laura("close", "--all")

    # [2 / 7] The pane interactions
    doc = await agent.laura("open", "plan.md", "--ratio", "55", "--no-focus")
    await agent.laura("comment", "8", "Should we greet in Spanish too? `--lang es` would print `Hola, Ada!`.", "--pane", doc)
    await agent.say(
        "[2 / 7] The pane interactions\n\n"
        "I opened a plan in a new pane and left you a question on L8. Press c to reply, "
        "r to collapse the thread, then Shift+S to submit your inline review.\n\n"
        "New: A collapsed thread shows as one row, with its line, its authors and its number of comments."
    )
    await user.goto_line(doc, 8)
    await user.press("c")
    await user.type("Yes, add Spanish")
    await user.press("Enter")
    await camera.pause(800)
    await camera.shot("thread")
    await user.press("r")
    await camera.pause(1200)
    await camera.shot("collapsed")
    await user.press("Shift+S")
    await user.press("Enter")
    await agent.wait_for_review()
    await agent.write("plan.md", PLAN_REVISED)
    await user.page.locator(f"#p{doc} .head .reloaded").wait_for(state="visible")
    await agent.say(
        "I applied your inline review: L8 now adds Spanish. Laura reloaded the plan in the pane with the changes.\n\n"
        "Send Next to review the code that implements your inline review."
    )
    # The drawer covers L8, so the user hides the drawer to see the revised step, tinted as changed.
    await user.press("Control+Backquote")
    await user.page.locator("#p0").wait_for(state="hidden")
    await camera.pause(1500)
    await camera.shot("reloaded")
    await user.press("Control+Backquote")
    await user.focused(0)
    await reply(user)
    await agent.laura("close", "--all")

    # [3 / 7] Code review
    await agent.write("hello.py", HELLO_CHANGED)
    diff = await agent.laura("open", "hello.py", "--diff", "--ratio", "55", "--no-focus")
    await agent.say(
        "[3 / 7] Code review\n\n"
        "Here's the code change that implements the plan. I opened the full diff in a new pane. "
        "Press Ctrl+P, then the pane's number, to focus the diff. Press d to turn the diff view off and on.\n\n"
        "New: With the diff view off, Laura tints the changed lines.\n\n"
        "Send Next to debug with live logs."
    )
    await user.focus(diff)
    await camera.shot("diff")
    await user.press("d")
    await camera.pause(1500)
    await camera.shot("tinted")
    await user.press("d")
    await camera.pause(1000)
    await reply(user)
    await agent.laura("close", "--all")

    # [4 / 7] Debug
    code = await agent.laura("open", "app.py", "--ratio", "50", "--no-focus")
    # The API's log arrives while the agent talks and the user pauses the tail pane.
    tail_args = ["--title", "app.log", "--follow", "--split", code, "--dir", "v", "--ratio", "40"]
    tail = asyncio.create_task(
        agent.laura("tail", *tail_args, pipe=API, shown=f"python app.py 2>&1 | laura tail {' '.join(tail_args)}")
    )
    pane = user.page.locator(".pane").filter(has=user.page.locator(".head", has_text="app-log"))
    await pane.wait_for()
    log = (await pane.get_attribute("id"))[1:]
    await agent.say(
        "[4 / 7] Debug\n\n"
        "I opened the greeting API's source in a new pane, started the API, sent it requests, and piped its "
        "log into a tail pane below the source. Laura scrolls the tail pane to the newest output as the "
        "requests arrive. The requests with `lang=fr` fail with 400 and `unknown lang: fr`, because L11 has no "
        "fallback to English.\n\n"
        "Press p in the tail pane to pause the tail pane and read the output. Press p again to follow the "
        "newest output.\n\n"
        "Send Next to see a page."
    )
    await camera.shot("tail")
    await user.focus(log)
    await user.press("p")
    await user.page.locator(f"#p{log} .head").get_by_text("Paused").wait_for()
    await camera.pause(1500)
    await camera.shot("paused")
    await user.press("p")
    await camera.pause(1000)
    await tail
    await reply(user)
    await agent.laura("close", "--all")

    # [5 / 7] See a page
    pg = await agent.laura("open", "page.html", "--ratio", "55", "--no-focus")
    frame = user.page.frame_locator(f"#p{pg} iframe")
    await agent.say(
        "[5 / 7] See a page\n\n"
        "I opened page.html in a new pane. Laura shows the page with its stylesheet and runs its "
        "script when you click Greet. Use comment mode in the pane header to comment on an element."
    )
    await user.click(frame.locator("button"))
    await frame.get_by_role("heading", name="Hello, Ada!").wait_for()
    await camera.pause(800)
    # The click selected the button, so `c` adds a comment to the button.
    await user.press("c")
    await user.type("Make the button blue.")
    await user.press("Enter")
    await camera.shot("page")
    await user.click(frame.get_by_text("Read the notes"))
    await camera.pause(1500)
    await user.focus(pg)
    await user.press("Shift+S")
    await user.press("Enter")
    await agent.wait_for_review()
    await camera.pause(1500)
    await agent.laura("close", "--all")

    # See a page from a URL
    await agent.edit("style.css", "button, input { padding: 0.5em 1em; }\n", "button, input { padding: 0.5em 1em; }\n"
                     "button { background: #4f5bd5; color: white; border: 0; border-radius: 4px; }\n")
    srv, port = serve(agent.repo)
    try:
        url = f"http://localhost:{port}/page.html"
        pb = await agent.laura(
            "open", url, "--ratio", "55", "--no-focus",
            shown=f"python -m http.server {port} & laura open {url} --ratio 55 --no-focus",
        )
        await user.page.locator(f"#p{pb} .head .live").wait_for(state="visible", timeout=20000)
        for _ in range(20):
            try:
                await agent.laura("highlight", "button", "--pane", pb)
                break
            except RuntimeError as e:
                if "is still loading" not in str(e):
                    raise
                await asyncio.sleep(1)
        await agent.say(
            "See a page from a URL\n\n"
            "I made the Greet button blue, as your inline review asked. Then I started a small web server and "
            f"opened {url} in a new browser pane. Laura's browser "
            "shows the page live, as your dev server would. I highlighted the Greet button by its CSS selector.\n\n"
            "Click Greet, press Ctrl+R to refresh the pane, then point at the button, press c and comment."
        )
        await user.click(f"#p{pb} .mk.hl")
        await user.focused(pb)
        await camera.pause(1200)
        # The refresh clears the highlight. The pointer stays on the button, so `c` comments on the button.
        await user.press("Control+r")
        await camera.pause(1500)
        await user.press("c")
        await user.type("Greet on Enter too.")
        await user.press("Enter")
        await camera.shot("browser")
        await user.press("Shift+S")
        await user.press("Enter")
        await agent.wait_for_review()
        await camera.pause(1500)
        await agent.laura("close", "--all")
    finally:
        srv.kill()

    # [6 / 7] Switch workspaces
    # The agent in the other workspace waits for the user while the user works in this workspace.
    await other.ask("The greeting page test failed 3 of 20 runs. Can I wait for the font to load? (y/n)")
    await user.page.locator("#wwait").wait_for(state="visible")
    await agent.say(
        "[6 / 7] Switch workspaces\n\n"
        "Each workspace is one task in one folder. The workspace switcher at the left of the top bar shows "
        "this workspace's session label, Laura demo, and its folder.\n\n"
        "Click the workspace switcher to switch, or press Ctrl+P and type a session label. While the Laura "
        "server runs, select the Laura logo to open the Laura server's page.\n\n"
        "When the agent in another workspace waits for you, the Laura server's page shows Waiting for you "
        "on that workspace's row.\n\n"
        "Send Next to learn how to record feedback."
    )
    await user.click("#wsw")
    await user.page.locator("#menu .mi[data-url]").first.wait_for()
    await camera.pause(1500)
    await camera.shot("switcher")
    await user.press("Escape")
    await reply(user)

    # [7 / 7] Recording feedback
    await agent.laura("feedback", "--positive", "the debug dashboard layout read clearly")
    await agent.laura("feedback", "--negative", "wanted a side-by-side diff view")
    await agent.say(
        "[7 / 7] Recording feedback\n\n"
        "Ask your agent to log positive or negative feedback at any time. Laura stores your feedback locally, "
        "so you can review it with /laura:retro.\n\n"
        "Send Next to wrap up."
    )
    await reply(user)
    await agent.laura("close", "--all")
    await agent.say(
        "The demo is finished! Here are a few workspace ideas for your first task: a plan review, "
        "a debug dashboard, a diff review, and a data analysis report."
    )
    # The demo ends on the Laura server's page, where the other workspace now waits for the user.
    await user.click("#brand")
    await user.page.locator("#rows .sv-wait").wait_for()
    await camera.pause(3000)
    await camera.shot("server")
