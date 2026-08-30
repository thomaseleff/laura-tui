---
name: demo
description: Run a guided, live walkthrough of Laura. Use when someone wants to see what Laura does or get a feel for the review loop, panels, tailing, workspaces, and feedback. Requires running inside a Laura tab (LAURA_TAB is set).
when_to_use: You are running inside a Laura tab (LAURA_TAB is set) and the developer wants a hands-on tour of Laura — "show me what this does", "demo laura", "how does this work".
user-invocable: true
---

# laura demo

You are running inside a Laura tab (`$LAURA_TAB` is set). Run a **live walkthrough** of Laura: you drive the panes with the `laura` CLI, the developer interacts inside the panels to guide you.

This file is an exact script. The narration under **Say** is copy to send the developer in chat, near-verbatim. The commands under **Do** are yours to run. After each numbered beat, **stop and wait** — the developer advances by sending `Next` in chat (beat 2 also advances when their review arrives). Don't run ahead.

Run `laura --help` (and `laura <cmd> --help`) to see the current CLI.

Docs: https://thomaseleff.github.io/laura-tui/llms.txt

## Setup (run once, silently)

```bash
JOURNAL=$(laura ready --session demo --agent demo)   # enables review submission; prints the journal path
D=$(mktemp -d)                                        # scratch dir for demo files
```

Keep `$JOURNAL` and `$D` for later beats.

---

## Beat 1 — What Laura is

**Say:**

> **Laura** — *LOW-rah* — is a TUI workspace your agent builds while you work.
>
> Laura started as an experiment, to allow developers to give feedback on files directly in the terminal. No browser, no leaving the shell.
>
> A few skills drive it, and you can run them from your chat any time: `/laura:open <file>` shows a file in a panel, `/laura:close` dismisses one, and `/laura:demo` runs this walkthrough.
>
> In the demo, I'll guide you through the core review workflow and show how you can build up a workspace with your agent as you go. To move between sections, just send me `Next` in the chat.
>
> More at https://thomaseleff.github.io/laura-tui and https://github.com/thomaseleff/laura-tui.
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
```

**Say:**

> **[1 / 5] The review loop**
>
> This is the loop Laura was built for — reviewing a file in place. The spec is open on the right. Try marking it up:
>
> 1. Panels are auto-focused by default. Press `Ctrl+P`, then type an id to focus a different panel. Or, press `Esc` from any panel to return here to the chat.
> 2. Move the line cursor with `↑`/`↓` to the **"expires 24 hours"** line, press `c`, type `make this 1 hour`, and press `Enter`.
> 3. Move to the **"stored in local storage"** line, press `c`, type `use an httpOnly cookie instead`, `Enter`.
> 4. Press `Shift+S`, type an overall note like `tighten token lifetimes before we ship`, and press `Enter` to submit.
>
> Your review lands right back in my chat.
>
> **Suggested prompts**
>
> - `/laura:open README.md`
> - `/laura:open Write up a plan and open it in a panel so I can mark it up`
>
> Send `Next` when you're done to see how a whole workspace comes together.

Then **wait.** Advance when the `[laura review · …]` block arrives **or** the developer sends `Next`.

When the review arrives: read it back in one line, then **edit `$D/tokens.md` to address each comment** — the panel re-renders live so they see it change. Then tell them to send `Next`.

---

## Beat 3 — The workspace is just panels

**Do:**

```bash
laura close --all
prev=""; i=0
for n in 1 1 2 3 5 8; do
  printf '%s\n' "$n" > "$D/fib-$i.txt"
  if [ -z "$prev" ]; then
    prev=$(laura open "$D/fib-$i.txt" --ratio 62)
  else
    [ $((i % 2)) -eq 1 ] && dir=v || dir=h
    prev=$(laura open "$D/fib-$i.txt" --split "$prev" --dir "$dir" --ratio 62)
  fi
  i=$((i+1))
done
```

**Say:**

> **[2 / 5] The workspace is just panels**
>
> Laura's workspace is just panels in a split tree — the agent composes them based on the task. Laura was designed to expose general-purpose tooling, so it can lay out pretty much anything... so here's a Fibonacci sequence.
>
> **Suggested prompts**
>
> - `/laura:open Stack the plan, the code, and the logs in one view`
> - `/laura:open Put the spec on the right and my notes below it`
>
> Send `Next` for your first workspace: reviewing a diff.

Then **wait for `Next`**, and `laura close --all`.

---

## Beat 4 — Workspace: diff review

**Do:**

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
DIFF=$(laura open "$D/tokens.diff" --ratio 55)
```

**Say:**

> **[3 / 5] Workspace: diff review**
>
> Here's the diff that implements the changes from the spec earlier — a shorter token TTL and an httpOnly cookie instead of local storage. Additions and removals are colored just like your editor. It opens focused, so review it just like the doc: `↑`/`↓` to a line, `c` to comment, `Shift+S` to submit (`Esc` returns to chat). A diff is just a file to Laura, so the whole review loop works on changes too.
>
> **Suggested prompts**
>
> - `/laura:open Show me the diff of my last commit so I can review it`
> - `/laura:open Open the staged changes in a pane for review`
>
> Send `Next` for the debug dashboard workspace.

Then **wait for `Next`** (or a review — address it if it comes), and `laura close --all`.

---

## Beat 5 — Workspace: debug dashboard

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
CODE=$(laura open "$D/worker.py" --ratio 40)   # shell left, code right
# Split the code pane vertically: code on top (78%), a thin log panel below where autoscroll is visible.
( for i in {1..30}; do echo "[$i] retry job=42 backoff=$((i*i))s"; sleep 0.3; done ) \
  | laura tail --title worker.log --follow --split "$CODE" --dir v --ratio 78 &
```

Check the fit with `laura layout`; if a panel reports `overflow_rows > 0`, lower a `--ratio` and re-open.

**Say:**

> **[4 / 5] Workspace: debug dashboard**
>
> Code on the right, a thin live log tailing right below it — the newest line pinned at the bottom as it streams. Watch the output while pointing at the suspect code.
>
> **Suggested prompts**
>
> - `/laura:open Open worker.py and tail the test run beside it`
> - `/laura:open Run the build and tail its output next to the failing file`
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
> - You can ask your agent at any time to log positive or negative feedback. All feedback is stored locally so you can audit / review periodically and open as issues in GitHub (https://github.com/thomaseleff/laura-tui/issues) anytime.
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
