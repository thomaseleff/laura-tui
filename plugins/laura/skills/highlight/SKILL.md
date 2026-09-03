---
name: highlight
description: Point the user at a span of an already-open file by reverse-highlighting a line range and scrolling it into view. Use when running inside a Laura tab (LAURA_TAB is set) and you want to direct attention to specific lines instead of re-describing them in chat.
when_to_use: You are running inside a Laura tab (LAURA_TAB is set), a file is already open in a pane, and you want to direct the user to a specific line range.
user-invocable: true
argument-hint: [start] [end]
---

# laura highlight

You are running inside a Laura tab (`$LAURA_TAB` is set). When you're referring to a specific span of a file, **point at it on screen** instead of pasting line numbers into the chat — Laura reverse-videos the range and scrolls it into view.

**"Show me where" — the common case.** Nothing's open yet (or a stale file is), and you want to say "here it is." Open *and* point in one call — the panel paints already at the span, no line-1 flash, no pane id to thread back:

```bash
laura open src/auth.rs --highlight 40 52   # open scrolled to and reverse-highlighting lines 40–52
```

**Already open?** Point at it with the standalone verb (this is also what a multi-step walkthrough composes, one call per step):

```bash
laura highlight 40 52 --pane "$id"   # reverse-highlight lines 40–52 in an open panel
```

- Line numbers are **1-based inclusive**, matching the panel gutter and review `L<n>` refs. Both forms take the same `<start> [end]`.
- `end` is optional — `laura open x --highlight 40` / `laura highlight 40` highlights a single line.
- `--pane <id>` (on `highlight`) targets a specific panel (default: the focused one). Highlighting works on an **unfocused** panel too — that's the point, direct attention to a pane the user isn't in.
- The highlight persists until you re-set it (or the file reloads shorter). Out-of-range numbers clamp to the file.

Re-call `laura highlight` to move the user's eye as the conversation moves.

Docs: https://thomaseleff.github.io/laura-tui/llms.txt
