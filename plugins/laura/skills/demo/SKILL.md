---
name: demo
description: Run a guided, live walkthrough of Laura.
when_to_use: You are running inside a Laura workspace and your partner wants to see what Laura does or get a feel for the inline review loop, panes, tailing, workspaces, and feedback.
user-invocable: true
---

# laura demo

Run a **live walkthrough** of Laura, where you lay out the panes using the `laura` CLI, coaching your partner on how Laura works.

**Rules**
- **Run Setup's `laura ready` first.** If it fails, tell your partner you're not running in a Laura workspace and stop.
- **After each numbered beat, stop and wait** — your partner advances by sending `Next` (beat 2 advances **only** when their inline review arrives). Don't run ahead.
- **If `laura close --all` errors with `unsubmitted inline review`**, send only: "Submit (`Shift+S`) or refresh (`Ctrl+R`) pane #N to continue.", then wait. **If it errors with `unsaved edits`**, send only: "Save (`:w`) or quit Neovim in pane #N to continue.", then wait. These only happen after your partner comments or edits. **If `laura open --edit` returns an error**, send only: "The editor pane didn't open: `<error>`. Moving on.", then continue to the next beat. These are the only messages you may send outside the script.
- **See the `laura` skill** for the full CLI or run `laura --help`.
- **Read the docs** at https://thomaseleff.github.io/laura-tui/llms.txt

## Instructions

The following workflow guides you through the demo, follow it from start to finish:
- Text under **Say** is the exact copy to send your partner with `laura say` — deliver it word-for-word, and do not send any other messages or commentary outside the script. Pipe a copy of several paragraphs in with a quoted heredoc: `laura say <<'EOF'` … `EOF`.
  - IF your partner asks you a question or for a further explanation, answer the question or walk them through the explanation in the same style of narration as the script before returning to the next step in the demo.
- Commands under **Do** are the exact commands to run that accompany that portion of the script.
- Italicized text is instruction for you
- _Mark a feature with **New:** only if the README's **New:** tip names the feature. When the tip drops the feature, remove the mark here too._

### Setup

**Do:**

```bash
# Use your own agent name for --agent (e.g. claude) — it auto-labels the comments you leave below.
# --title sets the session label that the workspace switcher shows.
laura ready --session demo --agent claude --title "Laura demo"   # enables inline review submission; prints JSON
D=$(mktemp -d)                                          # scratch dir for demo files
```

_Keep `$D` and the `journal` path `laura ready` printed (`$JOURNAL` below) for later beats. Note what it lists under `experimental`, whether `web_app` is `true`, and whether it printed a `browser_url`: they decide which beats run._

_The beat headings count to 7. If `laura ready` did not print `web_app` as `true`, beats 5 and 6 don't run: count to 5 instead (`[1 / 5]` … `[4 / 5]`), and number **Recording feedback** `[5 / 5]`._

```bash
# Close all open panes before beginning the demo
laura close --all
```

_In the web app, your partner now sees the start canvas above the drawer, with this chat in the drawer. Beat 1's first pane takes the start canvas's place._

---

### What Laura is

**Say:**

> **Laura** is a workspace that gives your agent an **API over the UI**, so your agent can tile panes while you pair-program.
>
> Each pane shows a file or the output of a running command, and you can work with your agent in every pane, next to the chat.
>
> Your agent learns Laura from its skills, and you can run the skills anytime in the chat:
>
> - `/laura:laura <file>` shows a file in a pane, and `/laura:laura <prompt>` lays out a whole workspace.
> - `/laura:demo` runs this walkthrough.
> - `/laura:explain <target>` has your agent explain code, a concept, or a PR.
> - `/laura:learn <prompt>` has your agent teach you a concept, or guide you through a coding task.
> - `/laura:retro` has your agent read back the feedback and inline reviews from past sessions.
>
> In this demo, I'll walk you through the inline review loop, show how your agent lays out different workspaces, and show how to record feedback. Each step ends with **Suggested prompts** to try with your agent after the demo.
>
> Read the docs at https://thomaseleff.github.io/laura-tui.
>
> Send `Next` to get started.

_If `laura ready` listed `editor` under `experimental`, add "You have experimental features on, so I'll show those too." before "Read the docs…". Then **wait for `Next`.**_

---

### The workspace is just panes

**Do:**

```bash
laura close --all
prev=""; i=0
for n in 1 1 2 3 5 8; do
  printf '%s\n' "$n" > "$D/fib-$i.txt"
  if [ -z "$prev" ]; then
    prev=$(laura open "$D/fib-$i.txt" --ratio 62 --no-focus)
  else
    [ $((i % 2)) -eq 1 ] && dir=v || dir=h
    prev=$(laura open "$D/fib-$i.txt" --split "$prev" --dir "$dir" --ratio 62 --no-focus)
  fi
  i=$((i+1))
done
```

**Say:**

> **[1 / 7] The workspace is just panes**
>
> Laura's workspace is just panes, so your agent can lay out almost anything. Here's a Fibonacci sequence, one number per pane.
>
> By default, Laura splits the newest pane and alternates the orientation, a spiral called a dwindle. Laura reloads each pane when its file changes.
>
> **Suggested prompts**
>
> - `/laura:laura Stack the plan, the code, and the logs in one view`
> - `/laura:laura Put the spec on the right and my notes below it`
>
> Send `Next` to learn how to interact with your agent in a pane.

_If `laura ready` printed `web_app` as `true`, also say before the suggested prompts: "The panes fill the workspace, and this chat floats in a drawer over them. Press ``Ctrl+` `` to close the drawer and see every pane, and ``Ctrl+` `` again to come back here. **New:** Drag the line between two panes to resize them, and drag the drawer's top edge to set its height. Laura keeps your sizes; double-click a line to give the size back to Laura."_

_Then **wait for `Next`**, and `laura close --all`._

---

### The pane interactions

**Do:**

```bash
cat > "$D/plan.md" <<'EOF'
# Plan: greet each visitor by name

Today the greeting API says `Hello, world!` to every visitor. This plan greets each visitor by name, then serves and logs the greeting, so we can debug it.

## Steps

1. `greet Ada` prints `Hello, Ada!`, and `greet` still prints `Hello, world!`.
2. The greeting is in English only.
3. The API serves the greeting at `/hello?name=Ada`.
4. The API logs each request, with the name it greeted.
EOF
DOC=$(laura open "$D/plan.md" --ratio 55 --no-focus)
# Ask your partner a question on the plan. Line numbers are
# 1-based *file* lines (count them in the heredoc, not the rendered view).
laura comment 8 'Should we greet in Spanish too? `--lang es` would print `Hola, Ada!`.' --pane "$DOC"
```

**Say:**

> **[2 / 7] The pane interactions**
>
> Beyond laying out panes, Laura lets you review with your agent in every pane, next to the chat.
>
> I opened a plan in a new pane and left you a question on L8. Answer my question:
>
> 1. To focus the plan pane, press `Ctrl+P`, then the pane's number. Press `Esc` in a file pane to return to the chat.
> 2. In the plan pane, press `↑`/`↓` to move the cursor to L8, the **"The greeting is in English only."** step. My question shows as a card under the line. Press `c`, type `Yes, add Spanish`, then press `Enter` to reply. To start a new thread, press `c` on a line without a thread.
> 3. Press `r` on the thread to collapse or expand the thread. **New:** Laura shows a collapsed thread as one row, with its line, its authors and its number of comments.
> 4. Press `Shift+S`, then `Enter`, to submit. You can type a review body before `Enter`, for overall direction.
>
> Laura submits your inline review, with its threads and review body, into the chat.
>
> **Suggested prompts**
>
> - `/laura:laura README.md`
> - `/laura:laura Write up a plan and open it in a pane so I can comment on it`
>
> Submit your inline review with `Shift+S` to continue.

_Then **wait** for the `[laura review · …]` block. If your partner sends `Next` instead, reply: "Submit your inline review with `Shift+S` (or refresh with `Ctrl+R` to skip it), then continue."_

**Do:**

_Apply your partner's answer to `$D/plan.md` (Spanish with `--lang es`) — the file pane reloads with the revision:_

```bash
cat > "$D/plan.md" <<'EOF'
# Plan: greet each visitor by name

Today the greeting API says `Hello, world!` to every visitor. This plan greets each visitor by name, then serves and logs the greeting, so we can debug it.

## Steps

1. `greet Ada` prints `Hello, Ada!`, and `greet` still prints `Hello, world!`.
2. The greeting is in English or Spanish: `--lang es` prints `Hola, Ada!`.
3. The API serves the greeting at `/hello?name=Ada`.
4. The API logs each request, with the name it greeted.
EOF
```

**Say:**

> I applied your inline review: L8 now adds Spanish. Laura reloaded the plan in the pane with the changes.
>
> Your threads add up to one inline review. When you submit with `Shift+S`, Laura writes the whole inline review into the chat and clears the threads from the pane.
>
> If I edit a file while you have an unsubmitted inline review on its pane, Laura freezes the pane. The frozen pane keeps showing the snapshot, so your comments stay on the original lines. Laura shows **Frozen** in the pane header and a banner with **Submit review** and **Refresh**. In the TUI, Laura shows ⚠ on the pane's bottom border. To continue, submit with `Shift+S`, or press `Ctrl+R` to refresh: Laura discards the unsubmitted inline review and reloads the file.
>
> Your agent can also have subagents annotate the pane, for extra opinions before you submit.
>
> Send `Next` to review the code that implements your inline review.

_Then **wait for `Next`**, and `laura close --all`._

---

### Review code

_Branch the following step on whether `git` is installed, then run *one* script and deliver its matching **Say** ._

**Do (git available):**

```bash
R="$D/repo"; mkdir -p "$R"
cat > "$R/hello.py" <<'EOF'
GREETINGS = {"en": "Hello"}


def greet(name="world", lang="en"):
    return f"{GREETINGS[lang]}, {name or 'world'}!"
EOF
git -C "$R" init -q
git -C "$R" -c user.email=demo@laura -c user.name=laura add -A
git -C "$R" -c user.email=demo@laura -c user.name=laura commit -qm base
cat > "$R/hello.py" <<'EOF'
GREETINGS = {"en": "Hello", "es": "Hola"}


def greet(name="world", lang="en"):
    return f"{GREETINGS[lang]}, {name or 'world'}!"
EOF
laura open "$R/hello.py" --diff --ratio 55 --no-focus   # the inline diff vs HEAD
```

**Say (git available):**

> **[3 / 7] Code review**
>
> Here's the code change that implements the plan, including the changes from your inline review. **New:** With `git` installed, Laura tints added and modified lines green and blue in each pane, shows a red row where lines were deleted, and can show the full diff. I opened the full diff in a new pane.
>
> 1. Press `Ctrl+P`, then the pane's number, to focus the diff.
> 2. Press `d` to turn the diff view **off**. Laura shows the file with the changed lines tinted. Press `d` again to turn the diff view back on.
>
> In the diff view, press `↑`/`↓` to move and `Esc` to return to the chat. To comment, press `d` to turn the diff view off, then press `c`. Press `Shift+S` to submit. If you comment, submit (`Shift+S`) or refresh (`Ctrl+R`) before you send `Next`.
>
> **Suggested prompts**
>
> - `/laura:laura Show me the diff of my last commit so I can review it`
> - `/laura:laura Open the staged changes in a pane so I can review them`
>
> Send `Next` to debug with live logs.

**Do (git unavailable):**

```bash
cat > "$D/hello.diff" <<'EOF'
diff --git a/hello.py b/hello.py
--- a/hello.py
+++ b/hello.py
@@ -1,4 +1,4 @@
-GREETINGS = {"en": "Hello"}
+GREETINGS = {"en": "Hello", "es": "Hola"}
 
 
 def greet(name="world", lang="en"):
EOF
laura open "$D/hello.diff" --ratio 55 --no-focus   # git absent — show the static patch instead
```

**Say (git unavailable):**

> **[3 / 7] Code review**
>
> Here's the code change that implements the plan, including the changes from your inline review. `git` isn't installed, so I opened the change as a patch file in a new pane. With `git` installed, Laura tints added and modified lines green and blue in each pane, shows a red row where lines were deleted, and can show the full diff.
>
> 1. Press `Ctrl+P`, then the pane's number, to focus the diff.
> 2. The patch is still a file, so the same pane keystrokes work: `↑`/`↓` to move, `c` to comment, `Shift+S` to submit, and `Esc` to return to the chat. If you comment, submit (`Shift+S`) or refresh (`Ctrl+R`) before you send `Next`.
>
> **Suggested prompts**
>
> - `/laura:laura Show me the diff of my last commit so I can review it`
> - `/laura:laura Open the staged changes in a pane so I can review them`
>
> Send `Next` to debug with live logs.

_Then **wait for `Next`** (ignore any inline review if one is submitted), and `laura close --all`._

---

### Debug

_The example runs the API with Python and sends it requests with `curl`. Use any language you have for the API; the steps stay the same._

**Do:**

```bash
cat > "$D/app.py" <<'EOF'
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import parse_qs, urlparse

GREETINGS = {"en": "Hello", "es": "Hola"}


class Hello(BaseHTTPRequestHandler):
    def do_GET(self):
        q = parse_qs(urlparse(self.path).query)
        name, lang = q.get("name", ["world"])[0], q.get("lang", ["en"])[0]
        if lang not in GREETINGS:  # bug: no fallback to English
            self.send_error(400, f"unknown lang: {lang}")
            return
        body = f"{GREETINGS[lang]}, {name}!\n".encode()
        self.send_response(200)
        self.end_headers()
        self.wfile.write(body)


HTTPServer(("127.0.0.1", 8766), Hello).serve_forever()
EOF
CODE=$(laura open "$D/app.py" --ratio 50 --no-focus)   # shell left, code right
# One subshell starts the API, sends it requests and stops it, so the tail pane ends by itself.
# Split the code pane vertically: code on top (60%), the API's log below (40%).
( python -u "$D/app.py" & APP=$!
  until curl -s -o /dev/null http://127.0.0.1:8766/hello; do sleep 0.5; done
  for i in $(seq 1 12); do
    for q in "name=Ada" "name=Ada&lang=es" "name=Bo" "name=Cy&lang=fr"; do
      curl -s -o /dev/null "http://127.0.0.1:8766/hello?$q"; sleep 1
    done
  done
  kill $APP ) 2>&1 | laura tail --title app.log --follow --split "$CODE" --dir v --ratio 40 &
```

**Say:**

> **[4 / 7] Debug**
>
> I opened the greeting API's source in a new pane, started the API, sent it requests, and piped its log into a tail pane below the source. Laura scrolls the tail pane to the newest output as the requests arrive. The requests with `lang=fr` fail with `400` and `unknown lang: fr`, because L11 has no fallback to English.
>
> Press `p` in the tail pane to pause the tail pane and read the output. Press `p` again to follow the newest output.
>
> A tail pane still shows a file, so the same pane keystrokes work: `↑`/`↓` to move, `c` to comment, `Shift+S` to submit, and `Esc` to return to the chat. If you comment, submit (`Shift+S`) or refresh (`Ctrl+R`) before you send `Next`.
>
> **Suggested prompts**
>
> - `/laura:laura Start the API and tail its log next to app.py`
> - `/laura:laura Run the tests and tail their output next to the failing file`
>
> Send `Next` to see a page.

_If `laura ready` did not print `web_app` as `true`, end with "Send `Next` to try the experimental features." instead if `laura ready` listed `editor` under `experimental`, or else with "Send `Next` to learn how to record feedback." Then **wait for `Next`**, and `laura close --all`._

---

### See a page

_Run this beat only if `laura ready` printed `web_app` as `true`: the TUI shows the page as plain text. Otherwise, skip this beat and **Switch workspaces** without mentioning them._

**Do:**

```bash
cat > "$D/page.html" <<'EOF'
<!DOCTYPE html>
<html>
<head><link rel="stylesheet" href="style.css"></head>
<body>
<h1>Hello, world!</h1>
<input id="name" value="Ada">
<button onclick="document.querySelector('h1').textContent = 'Hello, ' + document.getElementById('name').value + '!'">Greet</button>
<p><a href="notes.md">Read the notes</a></p>
</body>
</html>
EOF
cat > "$D/style.css" <<'EOF'
body { font-family: sans-serif; margin: 2em; }
button, input { padding: 0.5em 1em; }
EOF
cat > "$D/notes.md" <<'EOF'
# Notes

The page greets the name in the field.
EOF
laura open "$D/page.html" --ratio 55 --no-focus
```

**Say:**

> **[5 / 7] See a page**
>
> I opened `page.html` in a new pane. Laura shows the page with its stylesheet and runs its script when you click **Greet**.
>
> 1. Click **Greet**. The page changes the heading to **Hello, Ada!**, and Laura moves the cursor to the button's line.
> 2. Click the comment-mode button in the pane's header, then click **Greet**. Laura shows the comment box below the button. Type `Make the button blue`, press `Enter`, then click **Done** to leave comment mode.
> 3. Click **Read the notes**. Laura opens `notes.md` in a new pane.
> 4. Click **Submit review** in the header of page.html's pane, or click the page and press `Shift+S`. Then press `Enter` to submit. Laura quotes the button's line from `page.html` in the inline review.
>
> **Suggested prompts**
>
> - `/laura:laura Build a greeting page and open it so I can comment on it`
>
> Click the page and submit your inline review with `Shift+S` to continue.

_Then **wait** for the `[laura review · …]` block, and `laura close --all`._

_Run the rest of this beat only if `laura ready` printed a `browser_url`: Laura sets `browser_url` when browser panes work. Otherwise, skip straight to **Switch workspaces** without mentioning it._

_Next, show a page from a URL. Serve `$D` with any static file server; the example uses Python's. If `laura open <url>` returns an error, tell your partner and skip the rest of this beat._

**Do:**

```bash
# Apply your partner's comment on the button.
cat > "$D/style.css" <<'EOF'
body { font-family: sans-serif; margin: 2em; }
button, input { padding: 0.5em 1em; }
button { background: #4f5bd5; color: white; border: 0; border-radius: 4px; }
EOF
python -m http.server 8765 --directory "$D" >/dev/null 2>&1 & SRV=$!
# Wait for the server.
until curl -s -o /dev/null http://localhost:8765/; do sleep 0.5; done
P=$(laura open http://localhost:8765/page.html --ratio 55 --no-focus)
# Retry while the page loads.
while laura highlight button --pane $P 2>&1 | grep -q "is still loading"; do sleep 1; done
```

_If `laura highlight` returns another error, tell your partner and skip this beat._

**Say:**

> **See a page from a URL**
>
> I made the **Greet** button blue, as your inline review asked. Then I started a small web server and opened `http://localhost:8765/page.html` in a new browser pane. Laura's browser shows the page live, as your dev server would. The pane header shows the URL, and **● live** while the pane shows the page. I highlighted the **Greet** button by its CSS selector, so Laura outlines it.
>
> 1. Click **Greet** in the pane. The page changes the heading to **Hello, Ada!**, as in your own browser.
> 2. Press `Ctrl+R` in the pane to refresh the pane. Laura reloads the page.
> 3. Point at the button and press `c`. Laura names the button by its CSS selector. Type a comment and press `Enter`.
> 4. Press `Shift+S`, then `Enter`, to submit your inline review.

_Then **wait** for the `[laura review · …]` block._

_If you have chrome-devtools-mcp, add one beat. Select the page whose URL matches pane `$P` in `laura layout`, and click **Greet** with the tool while your partner watches. Then run `laura highlight button --pane $P` and say: "I clicked **Greet** with my browser tool. The ring showed where I clicked, and the heading changed to **Hello, Ada!**." Without the tool, skip this beat._

_Then `laura close --all`, and `kill $SRV`._

---

### Switch workspaces

_Run this beat only if `laura ready` printed `web_app` as `true`: the TUI has no workspace switcher._

**Say:**

> **[6 / 7] Switch workspaces**
>
> Each workspace is one task in one folder. The workspace switcher at the left of the top bar shows this workspace's session label, **Laura demo**, and its folder. I set the session label when the demo started.
>
> To start another workspace, do one of these:
>
> - Run `laura` in a terminal in another folder.
> - Start the Laura server: select the Laura icon (run `laura --shortcut` one time to make it, see [Start Laura from a desktop icon](https://thomaseleff.github.io/laura-tui/workspace/#start-laura-from-a-desktop-icon)), or run `laura --serve`. Then select **New workspace…** in the workspace switcher.
>
> While only this workspace runs and the Laura server does not run, the workspace switcher shows no list. When another workspace runs:
>
> 1. Click the workspace switcher. Laura lists the running workspaces. Click a workspace to switch to it. The workspace that you leave keeps running.
> 2. Or press `Ctrl+P` and type part of a session label or a folder. Laura lists the other workspaces under **Workspaces**. Press `Enter` to switch.
>
> While the Laura server runs, select the Laura logo at the left of the top bar to open the Laura server's page, which lists the running workspaces and starts new ones.
>
> When the agent in another workspace waits for you, for example at a permission prompt, the Laura server's page shows `Waiting for you` on that workspace's row.
>
> Read more in [Switch workspaces](https://thomaseleff.github.io/laura-tui/navigation/#switch-workspaces).
>
> **Suggested prompts**
>
> - `Label this session "greeting page"`
>
> Send `Next` to learn how to record feedback.

_If `laura ready` listed `editor` under `experimental`, end with "Send `Next` to try the experimental features." instead. Then **wait for `Next`.**_

---

### Edit together (experimental)

_Run this beat only if `laura ready` listed `editor` under `experimental`. Otherwise, skip to **Recording feedback** without mentioning it. The beat has no number. If `laura open --edit` returns an error, follow the Rules. The beat shows best with Laura's theme on (your partner answered `y` to the startup question), so the flip keeps one palette._

**Do:**

```bash
cat > "$D/greet.py" <<'EOF'
print("Hello, world!")
EOF
ED=$(laura open "$D/greet.py" --edit --ratio 55)   # never takes focus
```

**Say:**

> **Experimental · Edit together**
>
> The new pane is an **editor pane**, where you edit the file in Neovim.
>
> 1. Press `Ctrl+P`, then the pane's number, to edit the file. Change `world` to `Laura` and save with `:w`.
> 2. Tell me when you've saved.

_Wait for your partner, then annotate the saved line:_

```bash
laura comment 1 "nice - should the name come from the command line?" --pane "$ED"
```

**Say:**

> I left a comment on L1. In the editor view, Neovim shows my comment as a `✎` sign on L1, and the pane's border shows `[review: 1]`. To read the whole thread, press `]d`.
>
> 1. Press `Ctrl+L` to flip the pane to its **file view**. The file view shows the saved file, with my comment as a card, and takes the usual pane keystrokes.
> 2. On L1, press `c`, type a reply, then press `Enter`.
> 3. Press `Shift+S`, then `Enter`, to submit. Laura submits the inline review into the chat and flips the pane back to the editor view.
>
> **Suggested prompts**
>
> - `/laura:laura Open greet.py in an editor pane so I can edit it`
>
> Submit your inline review with `Shift+S` to learn how to record feedback.

_Then **wait** for the `[laura review · …]` block, and `laura close --all`._

---

### Recording feedback

**Do:**

```bash
laura feedback --positive "the debug dashboard layout read clearly"
laura feedback --negative "wanted a side-by-side diff view"
tail -2 "$JOURNAL"
```

**Say:**

> **[7 / 7] Recording feedback**
>
> Laura is new, so your agent may not know every command and flag yet, your agent may lay out content poorly, or you may spot ways to improve Laura.
>
> Ask your agent to log positive or negative feedback at any time. Laura stores your feedback locally, so you can:
>
> - Review your feedback with `/laura:retro`.
> - Have your agent save memories from your feedback.
> - Open issues on GitHub (https://github.com/thomaseleff/laura-tui/issues).
>
> **Suggested prompts**
>
> - `Log positive feedback: the debug dashboard layout read clearly`
> - `That felt clunky — log it as negative feedback for the Laura team`
>
> Send `Next` to wrap up.

_Then **wait for `Next`.**_

---

## Workspace ideas & wrap

**Do:**

```bash
laura close --all
rm -rf "$D"
```

**Say:**

> **The demo is finished!** Here are a few workspace ideas for your first task:
>
> - **Doc or plan review** — a plan on one side, which your agent revises as you comment.
> - **Debug dashboard** — source code next to a tail pane, like the one in step 4.
> - **Diff review** — review changes in place before you commit them.
> - **Data analysis** — a markdown report, with headings and lists, in a pane.
