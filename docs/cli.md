# CLI reference

`laura` with no subcommand runs the TUI host; with a subcommand it acts as a thin client that sends one message to the current tab's socket (`$LAURA_TAB`) and exits. Client verbs are silent on success.

## Host

```
laura                   Run the TUI, hosting your default shell in tab 1.
laura -- <cmd> [args]   Run the TUI, hosting <cmd> in tab 1. New tabs still get your shell.
```

## Commands

A tab's panes form a split tree; the shell is pane `0` and can't be closed. Each `open` splits a pane and prints the **new pane id** on stdout — capture it to address that pane later.

```
laura open <path>       Split a pane and render <path> in the new panel. Prints the new pane id.
      --split <id>      Pane to split (default: the focused pane).
      --dir <h|v>       Split orientation: h side-by-side, v stacked (default h).
      --ratio <1..99>   Percent of the split given to the first pane (default 50).
      --side <first|second>  Which side the new panel lands on (default second).
      --no-focus        Don't move focus into the panel.
      --follow          Autoscroll: pin the cursor to the last line on open and every reload.
      --dry-run         Print the would-be overflow report; open nothing.
laura close [<id>]      Close a panel (default: the focused one).
      --all             Close every panel, back to shell-only.
laura focus <id>        Focus a pane by id.
laura layout            Print the layout: per-pane rects + overflow (JSON).
laura ready             Mark the tab as hosting an agent (enables review submission). Prints the journal path.
      --session <id>    Name the journal session (default: laura-<pid>-<n>).
      --agent <name>    Attribute journal events to this agent name.
laura feedback          Append a feedback signal (layout/render quality, a missing tool) to the journal.
      --positive        Positive signal.        (one of --positive/--negative is required)
      --negative        Negative signal.
      [<body>]          Optional free-text note.
some-cmd | laura tail   Spool piped stdin to an internal file and show it in a live panel.
      --title <t>       Panel title (also names the spool file).
      --follow          Autoscroll to the newest line as output arrives.
```

Commands require `$LAURA_TAB` to be set — i.e. run them from inside a Laura-hosted shell. Outside a tab they error with `not inside a Laura tab (LAURA_TAB unset)`.

`layout` and `open --dry-run` both emit a JSON report — one entry per pane with its rect and overflow, so the agent can size a panel before (or without) committing:

```json
{
  "area": {"x": 0, "y": 1, "width": 120, "height": 39},
  "panes": [
    {"id": 0, "kind": "pty",   "path": null,       "rect": {"x":0,"y":1,"width":48,"height":39},
     "content_rows": null, "visible_rows": 37, "overflow_rows": 0,  "clipped": false},
    {"id": 1, "kind": "panel", "path": "spec.md",  "rect": {"x":48,"y":1,"width":72,"height":39},
     "content_rows": 120, "visible_rows": 37, "overflow_rows": 83, "clipped": true}
  ]
}
```

`overflow_rows > 0` (or `clipped`) means the panel is taller than its pane — widen/reshape the split or lower `--ratio` until it fits.

## Journal

`ready` names a per-session append-only NDJSON journal and prints its path. Every `open`/`close`/`focus`/review/`feedback` event is teed to it, so a session is auditable after it ends. Files live under the OS data dir (`%APPDATA%` / `$XDG_DATA_HOME` / `~/Library/Application Support`) at `laura/sessions/<session>.ndjson`, overridable with `LAURA_DATA_DIR`. It's just files: `cat "$(ls -t <dir>/laura/sessions/*.ndjson | head -1)" | jq .`.

## Global

```
laura --help            Print help (also per-subcommand: laura open --help).
laura --version         Print version.
```

The wire messages behind these verbs are documented in the [protocol](protocol.md).
