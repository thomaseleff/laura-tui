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
- **If `laura close --all` errors with `unsubmitted inline review`**, send only: "Submit (`Shift+S`) or refresh (`Ctrl+R`) pane #N to continue.", then wait. **If it errors with `unsaved edits`**, send only: "Save (`:w`) or quit Neovim in pane #N to continue.", then wait. These only happen after your partner comments or edits, and they are the only messages you may send outside the script.
- **See the `laura` skill** for the full CLI or run `laura --help`.
- **Read the docs** at https://thomaseleff.github.io/laura-tui/llms.txt

## Instructions

The following workflow guides you through the demo, follow it from start to finish:
- Text under **Say** is the exact copy to send your partner in chat — deliver it word-for-word and do not send any other chat messages / commentary outside the script.
  - IF your partner asks you a question or for a further explanation, answer the question or walk them through the explanation in the same style of narration as the script before returning to the next step in the demo.
- Commands under **Do** are the exact commands to run that accompany that portion of the script.
- Italicized text is instruction for you

### Setup

**Do:**

```bash
# Use your own agent name for --agent (e.g. claude) — it auto-labels the comments you leave below.
laura ready --session demo --agent claude              # enables inline review submission; prints JSON
D=$(mktemp -d)                                          # scratch dir for demo files
```

_Keep `$D` and the `journal` path `laura ready` printed (`$JOURNAL` below) for later beats. Note what it lists under `experimental`: it decides which bonus beats run._

```bash
# Close all open panes before beginning the demo
laura close --all
```

---

### What Laura is

**Say:**

> **Laura** is a TUI workspace that gives your agent an **API over the TUI**, so your agent can tile panes while you pair-program.
>
> Each pane shows a file or the output of a running command, and you can work with your agent in every pane without leaving the terminal.
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

_Then **wait for `Next`.**_

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

> **[1 / 5] The workspace is just panes**
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

_Then **wait for `Next`**, and `laura close --all`._

---

### The pane interactions

**Do:**

```bash
cat > "$D/plan.md" <<'EOF'
# Auth tokens — draft spec

Review before we ship.

- Tokens are issued on login and returned in the JSON body.
- Every token expires 24 hours after it is issued.
- A refresh token extends a session up to 30 days.
- Tokens are stored in local storage on the client.
- Revoked tokens are checked against a denylist on each request.

Open question: should refresh tokens rotate on every use?
EOF
DOC=$(laura open "$D/plan.md" --ratio 55)
# Annotate the spec for your partner to reply to. Line numbers are
# 1-based *file* lines (count them in the heredoc, not the rendered view).
laura comment 6 "suggest 1 hour — 24h is a long window for a bearer token" --pane "$DOC"
```

**Say:**

> **[2 / 5] The pane interactions**
>
> Beyond laying out panes, Laura lets you review with your agent in every pane, without leaving the terminal.
>
> I opened a plan in the pane to the right and left a comment for you. Reply to my comment and add your own:
>
> 1. Laura focuses a new pane when it opens. To focus another pane, press `Ctrl+P` then the pane's number. Press `Esc` in a file pane to return to the chat.
> 2. In the plan pane, press `↑`/`↓` to move the cursor to L6, the **"Every token expires 24 hours…"** line. My comment shows as a card under the line. Press `c`, type `Agreed`, then press `Enter` to reply.
> 3. Move to L11, the **"Open question: should refresh tokens rotate…"** line. Press `c`, type `Sounds good`, then press `Enter`. L11 has no thread yet, so your comment starts a **new** thread.
> 4. Press `r` on a thread to collapse or expand the thread. Press `r` on a line without a thread to collapse or expand every thread in the pane.
> 5. When you're ready, press `Shift+S`, type `Replied below - all looks good` as the review body, then press `Enter` to submit.
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

_Apply the agreed revisions to `$D/plan.md` (`ttl` → 1 hour, httpOnly cookie,
rotating refresh) — the file pane reloads with the revisions:_

```bash
cat > "$D/plan.md" <<'EOF'
# Auth tokens — draft spec

Review before we ship.

- Tokens are issued on login and returned in the JSON body.
- Every token expires 1 hour after it is issued.
- A refresh token extends a session up to 30 days, rotating on every use.
- Tokens are stored in an httpOnly cookie on the client.
- Revoked tokens are checked against a denylist on each request.

Open question: resolved — refresh tokens rotate on every use.
EOF
```

**Say:**

> I applied your inline review: L6 now sets a 1 hour TTL, and L11 now says refresh tokens rotate on every use. Laura reloaded the plan in the pane with the changes.
>
> Your threads add up to one inline review. When you submit with `Shift+S`, Laura writes the whole inline review into the chat and clears the threads from the pane.
>
> If I edit a file while you have an unsubmitted inline review on its pane, Laura freezes the pane. The frozen pane keeps showing the snapshot, so your comments stay on the original lines, and Laura shows ⚠ on the pane's bottom border. To continue, submit with `Shift+S`, or press `Ctrl+R` to refresh: Laura discards the unsubmitted inline review and reloads the file.
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
cat > "$R/tokens.py" <<'EOF'
def issue(user):
    ttl = 24 * 3600
    token = sign(user, ttl)
    store_local(token)
    return token
EOF
git -C "$R" init -q
git -C "$R" -c user.email=demo@laura -c user.name=laura add -A
git -C "$R" -c user.email=demo@laura -c user.name=laura commit -qm base
cat > "$R/tokens.py" <<'EOF'
def issue(user):
    ttl = 3600
    token = sign(user, ttl)
    set_httponly_cookie(token)
    return token
EOF
laura open "$R/tokens.py" --diff --ratio 55   # focused, straight into the inline diff vs HEAD
```

**Say (git available):**

> **[3 / 5] Code review**
>
> Here's the code change that implements the plan, including the changes from your inline review. With `git` installed, Laura marks added, modified and deleted lines in each pane's gutter, and can show the full diff. I opened the full diff in the pane to the right.
>
> 1. Press `d` to turn the diff view **off**. Laura shows the file with the gutter markers. Press `d` again to turn the diff view back on.
>
> The diff view still shows the file, so the same pane keys work: `↑`/`↓` to move, `c` to comment, `Shift+S` to submit, and `Esc` to return to the chat. If you comment, submit (`Shift+S`) or refresh (`Ctrl+R`) before you send `Next`.
>
> **Suggested prompts**
>
> - `/laura:laura Show me the diff of my last commit so I can review it`
> - `/laura:laura Open the staged changes in a pane so I can review them`
>
> Send `Next` to debug with live logs.

**Do (git unavailable):**

```bash
cat > "$D/tokens.diff" <<'EOF'
diff --git a/auth/tokens.py b/auth/tokens.py
--- a/auth/tokens.py
+++ b/auth/tokens.py
@@ -3,8 +3,8 @@ def issue(user):
-    ttl = 24 * 3600
+    ttl = 3600
     token = sign(user, ttl)
-    store_local(token)
+    set_httponly_cookie(token)
     return token
EOF
laura open "$D/tokens.diff" --ratio 55   # git absent — show the static patch instead
```

**Say (git unavailable):**

> **[3 / 5] Code review**
>
> Here's the code change that implements the plan, including the changes from your inline review. `git` isn't installed, so I opened the change as a patch file in the pane to the right. With `git` installed, Laura marks added, modified and deleted lines in each pane's gutter, and can show the full diff.
>
> The patch is still a file, so the same pane keys work: `↑`/`↓` to move, `c` to comment, `Shift+S` to submit, and `Esc` to return to the chat. If you comment, submit (`Shift+S`) or refresh (`Ctrl+R`) before you send `Next`.
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

**Do:**

```bash
cat > "$D/worker.py" <<'EOF'
def process(job):
    tries = 0
    while not job.done:
        tries += 1           # bug: backoff never resets between jobs
        run(job)
    return tries
EOF
CODE=$(laura open "$D/worker.py" --ratio 50 --no-focus)   # shell left, code right
# Split the code pane vertically: code on top (67%), a thin tail pane below (33%) where autoscroll is visible.
( for i in {1..30}; do echo "[$i] retry job=42 backoff=$((i*i))s"; sleep 0.3; done ) \
  | laura tail --title worker.log --follow --split "$CODE" --dir v --ratio 33 &
```

**Say:**

> **[4 / 5] Debug**
>
> I opened a source file in the pane to the right, ran the code, and piped its logs into a tail pane below the source file. Laura scrolls the tail pane to the newest output as the logs arrive.
>
> A tail pane still shows a file, so the same pane keys work: `↑`/`↓` to move, `c` to comment, `Shift+S` to submit, and `Esc` to return to the chat. If you comment, submit (`Shift+S`) or refresh (`Ctrl+R`) before you send `Next`.
>
> **Suggested prompts**
>
> - `/laura:laura Open worker.py and tail the test run beside it`
> - `/laura:laura Run the build and tail its output next to the failing file`
>
> Send `Next` to learn how to record feedback.

_Then **wait for `Next`**, and `laura close --all`._

---

### Edit together (experimental)

_Run this beat only if `laura ready` listed `editor` under `experimental`. Otherwise, or if `laura open --edit` returns an error, skip straight to **Recording feedback** without mentioning it. The beat shows best with Laura's theme on (your partner answered `y` to the startup question), so the flip keeps one palette._

**Do:**

```bash
cat > "$D/retry.py" <<'EOF'
def backoff(tries):
    delay = 2 ** tries
    return delay
EOF
ED=$(laura open "$D/retry.py" --edit --ratio 55)   # never takes focus
```

**Say:**

> **Bonus · Edit together**
>
> The pane to the right is an **editor pane**, where you edit the file in Neovim next to the chat.
>
> 1. Press `Ctrl+P`, then the pane's number, to edit the file. Change `2 ** tries` to `min(2 ** tries, 60)` and save with `:w`.
> 2. Tell me when you've saved.

_Wait for your partner, then annotate the saved line:_

```bash
laura comment 2 "nice cap - should 60 be a setting?" --pane "$ED"
```

**Say:**

> I left a comment on L2. In the editor view, Neovim shows my comment as a `✎` sign on L2, and the pane's border shows `[review: 1]`. To read the whole thread, put the cursor on L2 and press `Ctrl+W` then `d`.
>
> 1. Press `Ctrl+L` to flip the pane to its **file view**. The file view shows the saved file, with my comment as a card, and takes the usual pane keys.
> 2. On L2, press `c`, type a reply, then press `Enter`.
> 3. Press `Shift+S`, then `Enter`, to submit. Laura submits the inline review into the chat and flips the pane back to the editor view.
>
> **Suggested prompts**
>
> - `/laura:laura Open retry.py in an editor pane so I can fix it`
>
> Submit your inline review with `Shift+S` to continue.

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

> **[5 / 5] Recording feedback**
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
> - **Debug dashboard** — source code next to a tail pane, like you just saw.
> - **Diff review** — review changes in place before you commit them.
> - **Data analysis** — a markdown report, with headings and lists, in a pane.
