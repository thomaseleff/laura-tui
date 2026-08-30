# Tutorial

This walks through one full loop: start Laura, open a file, comment on it, and submit a review back to your agent. It assumes `laura` is installed (see the [README](../README.md)).

> [!TIP]
> **Prefer a guided version?** If you're running a coding agent inside Laura and have the skills installed, ask your agent to run `/laura:demo` for a live, hands-on tour of this same loop — plus panels, tailing, and feedback.

## 1. Start Laura

From your terminal:

```bash
laura
```

Laura opens with a single tab running your shell:

```
+- Laura ----------------------------------------------------+
| [ 1 ]                                                      |
+------------------------------------------------------------+
| $ _                                                        |
+------------------------------------------------------------+
```

Type into the shell as usual — Laura hosts it, it doesn't wrap it. The bottom line shows the current key hints; press `Ctrl+H` any time for the full list, `Ctrl+Q` to quit.

## 2. Run your agent and mark the tab ready

Run your coding agent in the shell. Laura has already set `LAURA_TAB` in this PTY, so the agent's `laura` calls land in this tab. Once, at the start, enable review submission:

```bash
laura ready
```

Until a tab is `ready`, commenting and review submission stay inert — there's no consumer to read a review. An agent that has the [skill](../plugins/laura/skills/open/SKILL.md) runs this itself.

## 3. Open a file in the panel

Show a file beside the shell:

```bash
laura open docs/protocol.md
```

The agent runs `laura open --split`, and the tab splits: your shell stays live on the left, the file renders on the right with a line-number gutter.

```
+- Laura -----------------------------------------------------+
| shell (pty)                | docs/protocol.md               |
| $ ...                      | 1  # Protocol                  |
|                            | 2                              |
|                            | 3  An agent mutates a tab's ...|
+----------------------------+--------------------------------+
```

The panel is live-watched: if the source file changes on disk, it re-renders on its own — no re-invoke.

You're not limited to one. Each `laura open` splits a pane, so you (or the agent) can stack a plan, logs, and a diff in the same tab — see the [how-to](how-to.md) and [CLI reference](cli.md) for `--split`/`--dir`/`--ratio`.

## 4. Comment on a line

Open the panes popup with `Ctrl+P`, then type the panel's **id** (the shell is `0`) to focus it — a single-digit id focuses on keypress; for `10`+ type the digits and press `Enter`. Move the line cursor with `↑`/`↓` (the mouse wheel scrolls too). On the line you want, press `c`, type your comment, and press `Enter` to pin it (`Esc` cancels). The panel title shows `[review: n]` as comments accumulate.

## 5. Submit the review

Press `Shift+S`, type an overall note, and press `Enter`. Laura assembles a PR-style review — each comment bound to its line number with the line's text — and injects it straight into the agent's shell as a `[laura review · …]` block. The agent reads it from its own input and gets to work; as it edits the file, the panel re-renders live.

That's the loop: *show → mark up → submit → revise*, without leaving the terminal. Task recipes are in the [how-to](how-to.md); the exact review format is in the [protocol](protocol.md).
