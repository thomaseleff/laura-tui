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
laura ready         # run once at session start — enables the user's review submission
laura open <path>   # split a pane, show <path> in the new panel. Prints the new pane id.
```

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

`laura layout` (and `laura open <path> --dry-run`) print JSON with each pane's rect and overflow. If a panel's `overflow_rows > 0` (or `clipped` is true), it's taller than its pane — lower `--ratio`, change `--dir`, or split a different pane until it fits. `--dry-run` reports without opening anything.

## Reading a review

The user marks up the panel and submits a review, which arrives in your input as a block:

```
[laura review · docs/spec.md]

tighten token lifetimes before we ship.

L1  Tokens expire in 24h.
      > make this 1h
```

Treat it as feedback on that file: address the overall note and each `L<n>` comment (line numbers are 1-based; the line's text is echoed after `L<n>`), then edit the file.
