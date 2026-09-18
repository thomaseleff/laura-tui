---
name: demo
description: Run a guided, live walkthrough of Laura. Use when someone wants to see what Laura does or get a feel for the review loop, panes, tailing, workspaces, and feedback.
when_to_use: You are running inside a Laura tab (LAURA_TAB is set) and the developer wants a hands-on tour of Laura — "show me what this does", "demo laura", "how does this work".
user-invocable: true
---

# laura demo

Run a **live walkthrough** of Laura, where you lay out the panes using the `laura` CLI, coaching the developer on how Laura works.

**Instructions**
- Text under **Say** is the exact copy to send the developer in chat — deliver it word-for-word and do not send any other chat messages / commentary outside the script.
  - IF the developer asks you a question or for a further explanation, answer the question or walk them through the explanation in the same style of narration as the script before returning to the next step in the demo.
- Commands under **Do** are the exact commands to run that accompany the script.

**Rules**
- **Check that `$LAURA_TAB` is set**, otherwise, let the user know you are not running in a Laura workspace.
- **After each numbered beat, stop and wait** — the developer advances by sending `Next` (beat 2 also advances when their review arrives). Don't run ahead.
- **See the `laura` skill** for the full CLI or run `laura --help`.
- **Read the docs** at https://thomaseleff.github.io/laura-tui/llms.txt

## Instructions

The following workflow guides you through the demo, follow it from start to finish:
- Text under **Say** is the exact copy to send the developer in chat — deliver it word-for-word and do not send any other chat messages / commentary outside the script.
  - IF the developer asks you a question or for a further explanation, answer the question or walk them through the explanation in the same style of narration as the script before returning to the next step in the demo.
- Commands under **Do** are the exact commands to run that accompany that portion of the script.
- Italicized text is instruction for you

### Setup

**Do:**

```bash
# Use your own agent name for --agent (e.g. claude) — it auto-labels the comments you leave below.
JOURNAL=$(laura ready --session demo --agent claude)   # enables review submission; prints the journal path
D=$(mktemp -d)                                          # scratch dir for demo files
```

_Keep `$JOURNAL` and `$D` for later beats._

```bash
# Close all open panes before beginning the demo
laura close --all
```

---

### What Laura is

**Say:**

> **Laura** is a TUI workspace that provides your agent with an **API over the TUI**, allowing your agent to lay out panes while you pair-program.
>
> Each pane renders a file or displays logs, allowing you to interact with your agent in every pane without ever leaving the window.
>
> Your agent learns how to use Laura through skills, and you can run them anytime in chat:
> 
> - `/laura:laura <file>` shows a file in a pane (or `/laura:laura <prompt>` to tile a whole workspace)
> - `/laura:demo` runs this walkthrough
> - `/laura:explain <target>` instructs your agent to explain code, a concept, or a PR
> - `/laura:learn <prompt>` instructs your agent to teach you a concept, or guide you through a coding task
> - `/laura:retro` allows you to browse feedback your agent has recorded on working with Laura
>
> In the demo, I'll guide you through the core inline review workflow, show you how your agent lays out different workspaces, and show you how to log feedback. Each step of the demo also includes **Suggested prompts** that you can use after the demo with your agent.
>
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
> Laura's workspace is just panes, so your agent can lay out pretty much anything... Here's a Fibonacci sequence. By default, panes open in a downward spiral / dwindle pattern and auto-update when the source file is modified.
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
# Leave two inline suggestions on the spec for the developer to reply to. Line numbers are
# 1-based *file* lines (count them in the heredoc, not the rendered view).
laura comment 6 "suggest 1 hour — 24h is a long window for a bearer token" --pane "$DOC"
```

**Say:**

> **[2 / 5] The pane interactions**
>
> Aside from laying out panes, Laura allows you to interact with your agent in every pane within your workspace without ever leaving the window.
>
> In the pane to the right I pulled up a plan file and left a suggestion for you to address. Reply to it and add your own:
>
> 1. Panes are auto-focused by default. Press `Ctrl+p`, then type a number to focus the corresponding pane. Press `Esc` from any pane to return here to the chat.
> 2. From within a focused pane, use the `↑`/`↓` arrow keys to move the line cursor to L6, the **"Every token expires 24 hours…"** line — you'll see my note as a card under it. Press `c`, type `Agreed`, and then `Enter` to leave an inline reply.
> 3. Move to L11, the **"Open question: should refresh tokens rotate…"** line, press `c`, and type `Sounds good`, and `Enter`. On a line with no thread, `c` opens a **new** thread instead of replying.
> 4. With active comment threads in a pane, press `r` on an inline thread to collapse/expand the thread. Pressing `r` off an inline thread toggles all threads within the pane.
> 5. When you're ready, press `Shift+S`, type `Replied below - all looks good` and `Enter` to submit.
>
> Your review, along with the inline comment threads, is submitted automatically back into the chat.
>
> **Suggested prompts**
>
> - `/laura:laura README.md`
> - `/laura:laura Write up a plan and open it in a pane so I can mark it up`
>
> Send `Next` or submit your review through `Shift+S` to continue.

_Then **wait.** Advance when the `[laura review · …]` block arrives **or** the developer sends `Next`._

**Do:**

_1. Apply the agreed revisions to `$D/plan.md` (`ttl` → 1 hour, httpOnly cookie,
rotating refresh) — the pane auto-updates by default:_

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

_2. Then reply on each thread — your note now sits under the revised line
(author-labelled):_

```bash
sleep 1   # let the pane finish reloading the rewritten file before commenting —
          # a comment that races the reload sees 0 lines and errors
laura comment 6 "done — changed the spec to a 1 hour TTL" --pane "$DOC"
laura comment 11 "done — noting refresh tokens rotate on every use" --pane "$DOC"
```

**Say:**

> Comment threads accumulate as a part of an inline file review, and `Shift+S` submits the entire review into chat, clearing the comment threads from the pane.
>
> If a file is edited while there is an open inline file review within a pane, the content becomes frozen (no longer auto-updating) to prevent comments from shifting before the review is submitted. When a file is edited during a review, you will be notified by a warning on the lower right of the pane border - you can either complete and submit the review with `Shift+S` or refresh the file and clear the entire review with `Ctrl+R`.
>
> Your agent can also task sub-agents to attach comments to threads as well for extra opinions before submitting a review.
>
> Send `Next` to review the code change from your inline review.

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
> Here's the code change that implements the plan from earlier, including the suggestions we worked out through the inline review. When `git` is available panes render per-line gutter markers, indicating added, modified, or deleted lines, or an optional full diff. I've opened the full diff in the pane to the right.
>
> 1. Press `d` to toggle the inline diff **off**, which will switch the pane back to rendering the raw content along with per-line gutter markers. Press `d` again to bring back the full diff.
>
> A diff is still just a view onto a file, so all the same in-pane interactions work as well (`↑`/`↓` to a line, `c` to comment, `Shift+S` to submit, `Esc` back to chat).
>
> **Suggested prompts**
>
> - `/laura:laura Show me the diff of my last commit so I can review it`
> - `/laura:laura Open the staged changes in a pane for review`
>
> Send `Next` to learn how to tail logs.

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
> Here's the code change that implements the plan from earlier, including the suggestions we worked out through the inline review. When `git` is available panes render per-line gutter markers, indicating added, modified, or deleted lines, or an optional full diff. I've opened the full diff in the pane to the right.
>
> A diff pane is still just a view onto a file, so all the in-pane interactions work as well (`↑`/`↓` to a line, `c` to comment, `Shift+S` to submit, `Esc` back to chat).
>
> **Suggested prompts**
>
> - `/laura:laura Show me the diff of my last commit so I can review it`
> - `/laura:laura Open the staged changes in a pane for review`
>
> Send `Next` to learn how to debug live logs.

_Then **wait for `Next`** (ignore any review if one is submitted), and `laura close --all`._

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
# Split the code pane vertically: code on top (78%), a thin log pane below where autoscroll is visible.
( for i in {1..30}; do echo "[$i] retry job=42 backoff=$((i*i))s"; sleep 0.3; done ) \
  | laura tail --title worker.log --follow --split "$CODE" --dir v --ratio 33 &
```

**Say:**

> **[4 / 5] Debug**
>
> I've opened a source code file to a new pane, then ran the file, tailing the logs into another pane on the right.
>
> Just like a diff, tailed log panes are still just a view onto a file, so all the in-pane interactions work as well (`↑`/`↓` to a line, `c` to comment, `Shift+S` to submit, `Esc` back to chat).
>
> **Suggested prompts**
>
> - `/laura:laura Open worker.py and tail the test run beside it`
> - `/laura:laura Run the build and tail its output next to the failing file`
>
> Send `Next` to learn how to record feedback.

_Then **wait for `Next`**, and `laura close --all`._

---

### Recording feedback

**Do:**

```bash
laura feedback --positive "the debug dashboard layout read clearly"
laura feedback --negative "wanted an inline diff view Laura doesn't have yet"
tail -2 "$JOURNAL"
```

**Say:**

> **[5 / 5] Recording feedback**
>
> Laura is a new tool, so your agent may not have background on the full command / flag interface, or your agent may display content poorly, or you may find improvements to the UX.
>
> You can ask your agent at any time to log positive or negative feedback to note quirks or record ways to best use Laura.
>
> All feedback is stored locally so you can audit / review periodically, task your agent to record memory based on your feedback, or open feedback as issues in GitHub (https://github.com/thomaseleff/laura-tui/issues) anytime.
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

**Do (cleanup):**

```bash
laura close --all
rm -rf "$D"
```

**Say:**

> **The demo is finished!** Here are a few final workspace ideas as you start your first task:
>
> - **Doc / plan review** — a plan on one side, revised live as you comment.
> - **Debug dashboard** — source plus a tailing log, like you just saw.
> - **Diff review** — review changes in place before they land.
> - **Data analysis** — a markdown report (headings, lists) rendered in a pane.
