---
name: demo
description: Run a guided, live walkthrough of Laura. Use when someone wants to see what Laura does or get a feel for the review loop, panes, tailing, workspaces, and feedback.
when_to_use: You are running inside a Laura tab (LAURA_TAB is set) and the developer wants a hands-on tour of Laura — "show me what this does", "demo laura", "how does this work".
user-invocable: true
---

# laura demo

Run a **live walkthrough** of Laura: you drive the panes with the `laura` CLI, the developer interacts inside the panes to guide you.

The narration under **Say** is the exact copy to send the developer in chat — deliver it word-for-word, don't rephrase it. The commands under **Do** are yours to run.

**Rules**
- **Check that `$LAURA_TAB` is set**, otherwise, let the user know you are not running in a Laura workspace.
- **After each numbered beat, stop and wait** — the developer advances by sending `Next` (beat 2 also advances when their review arrives). Don't run ahead.
- **See the `laura` skill** for the full CLI or run `laura --help`.
- **Read the docs** at https://thomaseleff.github.io/laura-tui/llms.txt

## Setup (run once, silently)

```bash
# Use your own agent name for --agent (e.g. claude) — it auto-labels the comments you leave below.
JOURNAL=$(laura ready --session demo --agent claude)   # enables review submission; prints the journal path
D=$(mktemp -d)                                          # scratch dir for demo files
```

Keep `$JOURNAL` and `$D` for later beats.

```bash
# Close all open panes before beginning the demo
laura close --all
```

---

## Beat 1 — What Laura is

**Say:**

> **Laura** is a TUI workspace that provides your agent with an **API over the TUI**, allowing your agent to dynamically tile panes while you pair-program.
>
> Laura started as an experiment, to allow developers to give feedback on files directly in the terminal without leaving the shell.
>
> Your agent learns how to use Laura through skills, and you can run them anytime in chat:
> 
> - `/laura:laura <file>` shows a file in a pane (or `/laura:laura <prompt>` to tile a whole workspace)
> - `/laura:demo` runs this walkthrough
> - `/laura:explain <target>` instructs your agent to explain code, a concept, or a PR
> - `/laura:learn <prompt>` instructs your agent to teach you a concept, or guide you through a coding task
> - `/laura:retro` allows you to browse feedback your agent has recorded on working with Laura
>
> In the demo, I'll guide you through the core review workflow and show how you can build up a workspace with your agent as you go. To move between sections, just send me `Next` in the chat.
>
> Read the docs at https://thomaseleff.github.io/laura-tui.
>
> Send `Next` to try the review loop.

Then **wait for `Next`.**

---

## Beat 2 — The markup loop

**Do:**

```bash
cat > "$D/tokens.md" <<'EOF'
# Auth tokens — draft spec

Review before we ship.

- Tokens are issued on login and returned in the JSON body.
- Every token expires 24 hours after it is issued.
- A refresh token extends a session up to 30 days.
- Tokens are stored in local storage on the client.
- Revoked tokens are checked against a denylist on each request.

Open question: should refresh tokens rotate on every use?
EOF
DOC=$(laura open "$D/tokens.md" --ratio 55)
# Leave two inline suggestions on the spec for the developer to reply to. Line numbers are
# 1-based *file* lines (count them in the heredoc, not the rendered view).
laura comment 6 "suggest 1 hour — 24h is a long window for a bearer token" --pane "$DOC"
laura comment 8 "suggest an httpOnly cookie — local storage is XSS-readable" --pane "$DOC"
```

**Say:**

> **[1 / 5] The review loop**
>
> Aside from composing panes, Laura allows you to interact with your agent in every pane within your workspace. In the right pane I pulled up a markdown spec and left two suggestions for you to address. Reply to them and add your own:
>
> 1. Panes are auto-focused by default. Press `Ctrl+P`, then type an id to focus a different pane. Or, press `Esc` from any pane to return here to the chat.
> 2. Move the line cursor with `↑`/`↓` to the **"expires 24 hours"** line — you'll see my note as a card under it. Press `c`, type `Agreed`, and then `Enter`.
> 3. Move to the **"Open question: should refresh tokens rotate…"** line, press `c`, and type `Sounds good`, and `Enter`. On a line with no thread, `c` opens a **new** thread instead of replying.
> 4. From within a focused pane, press `r` on an inline thread to collapse/expand the thread. Pressing `r` off an inline thread toggles all threads within the pane.
> 5. When you're ready, press `Shift+S`, type `Tightening looks good` and `Enter` to submit.
>
> Your review, along with the inline comment threads, is submitted automatically back into the chat.
>
> **Suggested prompts**
>
> - `/laura:laura README.md`
> - `/laura:laura Write up a plan and open it in a pane so I can mark it up`
>
> Send `Next` when you're done to see how a whole workspace comes together.

Then **wait.** Advance when the `[laura review · …]` block arrives **or** the developer sends `Next`.

The submit cleared their pane — a comment is a call and response, so their review block is the whole conversation and nothing is left on the file. When it arrives: read it back in one line, then reply and edit as **two separate steps** (not one bash block) so the pane's live reload settles in between — otherwise a comment can land while the file is mid-write and error as out-of-range:

1. As you work each thread, **reply on it** with `laura comment <line> "done — <what you did>" --pane "$DOC"` — your note appears inline under the conversation (author-labelled).
2. Then **edit `$D/tokens.md` to apply the agreed revisions** (`ttl` → 1 hour, httpOnly cookie) — the pane re-renders live so they see it change.

Then deliver the **Say** below word-for-word, and wait for `Next`.

**Say:**

> A comment is a call and response: notes ride the pane while we work, and `Shift+S` ships them all as one review block and clears the pane. If I edit the file while you have comments open, the pane freezes on your snapshot — submit with `Shift+S` or refresh with `Ctrl+R` to catch up.
>
> Your agent can also task sub-agents to attach comments to threads as well for extra opinions.
>
> Send `Next` to see how panes are composed.

---

## Beat 3 — The workspace is just panes

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

> **[2 / 5] The workspace is just panes**
>
> Laura's workspace is just panes in a split tree. Laura was designed to expose general-purpose tooling, so it can lay out pretty much anything... so here's a Fibonacci sequence. By default, panes open in a downward spiral / dwindle pattern.
>
> **Suggested prompts**
>
> - `/laura:laura Stack the plan, the code, and the logs in one view`
> - `/laura:laura Put the spec on the right and my notes below it`
>
> Send `Next` for code review.

Then **wait for `Next`**, and `laura close --all`.

---

## Beat 4 — Workspace: code review

Branch **once** on whether `git` is installed, then run *one* script and deliver its matching **Say** — not both.

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
> Here's the change that implements the spec from earlier — a shorter token TTL and an `httpOnly` cookie instead of local storage — opened as an inline diff against the last commit.
>
> Press `d` to toggle the inline diff **off**: the interleaved `+`/`-` view collapses back to the plain, rendered file, and the changed lines stay flagged by **markers in the line-number gutter**. Press `d` again to bring the full diff back.
>
> A diff is still just a view onto a file, so all the in-panel interactions work as well (`↑`/`↓` to a line, `c` to comment, `Shift+S` to submit, `Esc` back to chat).
>
> **Suggested prompts**
>
> - `/laura:laura Show me the diff of my last commit so I can review it`
> - `/laura:laura Open the staged changes in a pane for review`
>
> Send `Next` to debug a service live.

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
> Here's the change that implements the spec from earlier — a shorter token TTL and an `httpOnly` cookie instead of local storage — shown as a `.diff` file.
>
> A diff is still just a view onto a file, so all the in-panel interactions work as well (`↑`/`↓` to a line, `c` to comment, `Shift+S` to submit, `Esc` back to chat).
>
> **Suggested prompts**
>
> - `/laura:laura Show me the diff of my last commit so I can review it`
> - `/laura:laura Open the staged changes in a pane for review`
>
> Send `Next` to debug a service live.

Then **wait for `Next`** (or a review — address it if it comes), and `laura close --all`.

---

## Beat 5 — Workspace: debug

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

Check the fit with `laura layout`; if a pane reports `overflow_rows > 0`, lower a `--ratio` and re-open.

**Say:**

> **[4 / 5] Debug**
>
> Here is a workspace showing a code file with logs tailed underneath. Panes all update automatically, allowing your agent to bring up logs for you and your agent to debug together.
>
> **Suggested prompts**
>
> - `/laura:laura Open worker.py and tail the test run beside it`
> - `/laura:laura Run the build and tail its output next to the failing file`
>
> Send `Next` to learn how to record feedback.

Then **wait for `Next`**, and `laura close --all`.

---

## Beat 6 — Recording feedback

**Do:**

```bash
laura feedback --positive "the debug dashboard layout read clearly"
laura feedback --negative "wanted an inline diff view Laura doesn't have yet"
tail -2 "$JOURNAL"
```

**Say:**

> **[5 / 5] Recording feedback**
>
> - Laura is new so your agent may not have all the tools, or display content poorly, or you may find improvements to the UX.
> - Laura was built to be as general-purpose as possible to allow developers and agents to figure out what works, or what tools are missing.
> - You can ask your agent at any time to log positive or negative feedback.
>
> All feedback is stored locally so you can audit / review periodically, task your agent to record memory based on your feedback, or open feedback as issues in GitHub (https://github.com/thomaseleff/laura-tui/issues) anytime.
>
> **Suggested prompts**
>
> - `Log positive feedback: the debug dashboard layout read clearly`
> - `That felt clunky — log it as negative feedback for the Laura team`
>
> Send `Next` to wrap up.

Then **wait for `Next`.**

---

## Beat 7 — Workspace ideas & wrap

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
