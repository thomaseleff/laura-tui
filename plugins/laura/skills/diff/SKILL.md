---
name: diff
description: Show the user what changed in a file by rendering it as an inline +/- diff vs git HEAD in a Laura pane. Use when running inside a Laura tab (LAURA_TAB is set) and the user asks "what changed?" or you want to show your edits rather than describe them.
when_to_use: You are running inside a Laura tab (LAURA_TAB is set) and want to show the changes in a file (an inline +/- diff vs git HEAD) instead of pasting a diff into the chat.
user-invocable: true
argument-hint: [path]
---

# laura diff

You are running inside a Laura tab (`$LAURA_TAB` is set). When the user asks "what changed?" — or you want them to see the edits you just made — **show the diff on screen** instead of pasting `git diff` output into the chat. Laura renders the panel as an interleaved `+`/`-` diff vs git `HEAD`: green `+` for added/modified lines, red `-` rows for deleted lines with their old text.

**Open straight into the diff:**

```bash
laura open src/auth.rs --diff   # open the file already showing its diff vs HEAD
```

**Already open?** Toggle the view on the open panel:

```bash
laura diff --pane "$id"          # toggle the diff view on (no --off = toggle)
laura diff --pane "$id" --off    # back to the normal file view
```

- `--pane <id>` targets a specific panel (default: the focused one). With the panel focused, the user can also press **`d`** to toggle it.
- Toggling on a **clean or untracked** file — or one where `git` isn't installed — is a no-op: `laura diff` exits non-zero with a warning on stderr, and `laura open --diff` warns but opens the normal view. There's nothing to diff.
- The diff is vs the working tree's `HEAD` and refreshes as the file reloads, so it tracks your edits.
- Markdown renders a projection, not raw source, so its diff view shows the file plain — use the gutter markers or `git diff` for `.md`.

Docs: https://thomaseleff.github.io/laura-tui/llms.txt
