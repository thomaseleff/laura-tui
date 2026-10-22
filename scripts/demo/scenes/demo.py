"""The README demo. The scene follows the beats of plugins/laura/skills/demo/SKILL.md, and plays the
actions of the user.

Change this scene only when the user asks for a new README demo. Refer to the record-demo skill.
"""

FIXTURES = "demo"

PLAN_REVISED = """# Auth tokens — draft spec

Review before we ship.

- Tokens are issued on login and returned in the JSON body.
- Every token expires 1 hour after it is issued.
- A refresh token extends a session up to 30 days, rotating on every use.
- Tokens are stored in an httpOnly cookie on the client.
- Revoked tokens are checked against a denylist on each request.

Open question: resolved — refresh tokens rotate on every use.
"""

TOKENS_CHANGED = """def issue(user):
    ttl = 3600
    token = sign(user, ttl)
    set_httponly_cookie(token)
    return token
"""


async def reply(user, text="Next"):
    await user.focus(0)
    await user.type(text)
    await user.press("Enter")


async def scene(user, agent, camera):
    await user.type("/laura:demo")
    await user.press("Enter")
    await agent.laura("ready", "--session", "demo", "--agent", "claude")
    await agent.laura("close", "--all")

    # What Laura is
    await agent.say(
        "Laura is a TUI workspace that gives your agent an API over the TUI, so your agent can tile panes "
        "while you pair-program.\n\n"
        "Each pane shows a file or the output of a running command, and you can work with your agent in every pane.\n\n"
        "In this demo, I'll walk you through the inline review loop, show how your agent lays out different "
        "workspaces, and show how to record feedback. You have experimental features on, so I'll show those too.\n\n"
        "Send Next to get started."
    )
    await reply(user)

    # [1 / 5] The workspace is just panes
    prev = None
    for i, n in enumerate([1, 1, 2, 3, 5, 8]):
        (agent.repo / f"fib-{i}.txt").write_text(f"{n}\n")
        split = ["--split", prev, "--dir", "v" if i % 2 else "h"] if prev else []
        prev = await agent.laura("open", f"fib-{i}.txt", *split, "--ratio", "62", "--no-focus")
    await agent.say(
        "[1 / 5] The workspace is just panes\n\n"
        "Laura's workspace is just panes, so your agent can lay out almost anything. "
        "Here's a Fibonacci sequence, one number per pane.\n\n"
        "Send Next to learn how to interact with your agent in a pane."
    )
    await reply(user)
    await agent.laura("close", "--all")

    # [2 / 5] The pane interactions
    doc = await agent.laura("open", "plan.md", "--ratio", "55")
    await agent.laura("comment", "6", "suggest 1 hour — 24h is a long window for a bearer token", "--pane", doc)
    await agent.say(
        "[2 / 5] The pane interactions\n\n"
        "I opened a plan in the pane to the right and left a comment for you on L6. "
        "Reply to my comment, add your own on L11, then press Shift+S to submit your inline review."
    )
    await user.goto_line(doc, 6)
    await user.press("c")
    await user.type("Agreed")
    await user.press("Enter")
    await user.goto_line(doc, 11)
    await user.press("c")
    await user.type("Sounds good")
    await user.press("Enter")
    await camera.pause(800)
    await camera.shot("threads")
    await user.press("Shift+S")
    await user.type("Replied below - all looks good")
    await user.press("Enter")
    await agent.wait_for_review()
    await agent.write("plan.md", PLAN_REVISED)
    await user.page.locator(f"#p{doc} .head .reloaded").wait_for(state="visible")
    await agent.say(
        "I applied your inline review: L6 now sets a 1 hour TTL, and L11 now says refresh tokens rotate "
        "on every use. Laura reloaded the plan in the pane with the changes.\n\n"
        "Send Next to review the code that implements your inline review."
    )
    await camera.shot("reloaded")
    await reply(user)
    await agent.laura("close", "--all")

    # [3 / 5] Code review
    await agent.write("tokens.py", TOKENS_CHANGED)
    await agent.laura("open", "tokens.py", "--diff", "--ratio", "55")
    await agent.say(
        "[3 / 5] Code review\n\n"
        "Here's the code change that implements the plan. I opened the full diff in the pane to the right. "
        "Press d to turn the diff view off and on.\n\n"
        "Send Next to debug with live logs."
    )
    await camera.shot("diff")
    await user.press("d")
    await camera.pause(1500)
    await user.press("d")
    await camera.pause(1000)
    await reply(user)
    await agent.laura("close", "--all")

    # [4 / 5] Debug
    code = await agent.laura("open", "worker.py", "--ratio", "50", "--no-focus")
    logs = [f"[{i}] retry job=42 backoff={i * i}s" for i in range(1, 21)]
    await agent.laura(
        "tail",
        "--title",
        "worker.log",
        "--follow",
        "--split",
        code,
        "--dir",
        "v",
        "--ratio",
        "33",
        stdin=logs,
        shown=f"python worker.py 2>&1 | laura tail --title worker.log --follow --split {code} --dir v --ratio 33",
    )
    await camera.shot("tail")
    await agent.say(
        "[4 / 5] Debug\n\n"
        "I opened a source file in the pane to the right, ran the code, and piped its logs into a tail pane "
        "below the source file. Laura scrolls the tail pane to the newest output as the logs arrive.\n\n"
        "Send Next to try the experimental features."
    )
    await reply(user)
    await agent.laura("close", "--all")

    # Bonus · Edit together
    ed = await agent.laura("open", "retry.py", "--edit", "--ratio", "55")
    await agent.say(
        "Bonus · Edit together\n\n"
        "The pane to the right is an editor pane, where you edit the file in Neovim next to the chat. "
        "Press Ctrl+P, then the pane's number, then change 2 ** tries to min(2 ** tries, 60) and save with :w."
    )
    await user.focus(ed)
    await user.type("2G", keys=True)
    await user.type("0f2", keys=True)
    await user.type("C", keys=True)
    await user.type("min(2 ** tries, 60)")
    await user.press("Escape")
    await user.type(":w", keys=True)
    await user.press("Enter")
    await camera.pause(1200)
    if "min(2 ** tries, 60)" not in (agent.repo / "retry.py").read_text(encoding="utf-8"):
        raise AssertionError("Neovim did not save retry.py")
    await camera.shot("edited")
    await reply(user, "Saved")
    await agent.laura("comment", "2", "nice cap - should 60 be a setting?", "--pane", ed)
    await agent.say(
        "I left a comment on L2. Press ]d in Neovim to read the thread, then Ctrl+L to flip the pane "
        "to its file view, reply on L2, and submit."
    )
    await user.focus(ed)
    await user.type("]d", keys=True)
    await camera.pause(1500)
    await camera.shot("float")
    await user.type("kj")
    await user.press("Control+l")
    await user.goto_line(ed, 2)
    await user.press("c")
    await user.type("Yes, a setting. Default 60.")
    await user.press("Enter")
    await user.press("Shift+S")
    await user.press("Enter")
    await agent.wait_for_review()
    await camera.pause(1500)
    await agent.laura("close", "--all")

    # Bonus · See a page
    pg = await agent.laura("open", "page.html", "--ratio", "55")
    frame = user.page.frame_locator(f"#p{pg} iframe")
    await agent.say(
        "Bonus · See a page\n\n"
        "I opened page.html in the pane to the right. Laura shows the page with its stylesheet and runs its "
        "script when you click Save. Use comment mode in the pane header to comment on an element."
    )
    await user.click(frame.locator("button"))
    await camera.pause(800)
    # The click selected the button, so `c` adds a comment to the button.
    await user.press("c")
    await user.type("Make the button wider.")
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

    # [5 / 5] Recording feedback
    await agent.laura("feedback", "--positive", "the debug dashboard layout read clearly")
    await agent.laura("feedback", "--negative", "wanted a side-by-side diff view")
    await agent.say(
        "[5 / 5] Recording feedback\n\n"
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
    await camera.pause(2000)
