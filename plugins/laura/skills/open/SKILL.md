---
name: open
description: Show a file to the user in a Laura pane and act on the review they send back. Use when running inside a Laura tab (the LAURA_TAB env var is set), or when a "[laura review · …]" block arrives in your input.
when_to_use: You are running inside a Laura tab (LAURA_TAB is set) and want to show the user a file, or a "[laura review · …]" block arrives in your input.
user-invocable: true
argument-hint: [path]
---

# laura open

You are running inside a Laura tab (`$LAURA_TAB` is set). Prefer **showing** a file in a pane over pasting its contents into the chat. The panel is live — as you edit the file, it re-renders.

```bash
laura ready --session "$YOUR_SESSION_ID" --agent claude   # once at start — pass your own session id so the journal maps to this conversation
laura open <path>                                         # split a pane, show <path> in the new panel. Prints the new pane id.
```

Use your **own** session/conversation id as `--session` (Claude Code: your session id), so a Laura journal lines up 1:1 with this conversation — that's what makes the user's feedback joinable back to what produced it. `--agent` is your agent name. If you truly have no session id, omit `--session` (it defaults to `laura-<pid>-<n>`) — but prefer yours.

Run `laura ready` once so the user can comment and submit, then `laura open <path>` whenever you want them to look at a file.

Docs: https://thomaseleff.github.io/laura-tui/llms.txt

## Arranging panes

A tab is a split tree. The shell is pane `0`; every `laura open` splits a pane and **prints the new pane id** — capture it to address that pane later.

```bash
id=$(laura open plan.md)                       # plan beside the shell (default: split focused, side-by-side)
laura open logs.txt --split "$id" --dir v      # stack logs under the plan
laura open notes.md --ratio 30                 # give the first pane only 30% of the split
laura close "$id"                              # close one pane by id
laura close --all                              # back to shell-only
```

- `--dir h` splits side-by-side, `--dir v` stacks. `--ratio <1..99>` is the first pane's percent (default 50). `--side first|second` picks which side the new panel lands on.
- Assemble the frame for the task — plan right, logs bottom — instead of one panel at a time.

## Fitting before you commit

After `laura open`, read its **stderr** — it warns when the panel doesn't fit, so you don't need a follow-up `laura layout`:

- **`overflows:`** — the doc is taller than the pane. Fine for reading (the user scrolls). Only grow it (`--dir v` for full width, or raise `--ratio`) if they need the whole thing at once.
- **`too small`** — the pane can't render at all; raise `--ratio` or change `--dir`.
- **`cannot read <path>: …`** — the file didn't open; fix the path (the panel shows the error too).
- **no warning** — it fits; move on.

Suspect a long doc up front? `laura open <path> --dry-run` (or `laura layout`) prints JSON with each pane's rect and overflow (`overflow_rows`/`clipped`) without opening anything — pick the geometry, then open once.

Width is a non-issue for **pre-formatted** content: code, diffs, and markdown tables / code fences / HTML blocks no longer wrap — they clip to the panel and scroll sideways (a `›` marks a clipped line). Don't pre-widen the pane or pipe `git diff --stat` just to keep columns aligned; open it narrow and tell the user to press **←/→** in the focused panel to scroll. Prose still wraps normally. (`overflows:` above is about height only.)

## Reading a review

The user can mark up any panel with an in-line review, which arrives in your input as a block:

```
[laura review · docs/spec.md]

tighten token lifetimes before we ship.

L1  Tokens expire in 24h.
      > make this 1h
```

Treat it as feedback on that file: address the overall note and each `L<n>` comment (line numbers are 1-based; the line's text is echoed after `L<n>`), then edit the file.
