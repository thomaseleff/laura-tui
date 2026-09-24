# CLI reference

`laura` with no subcommand runs the TUI host; with a subcommand it acts as a thin client that sends one message to the current tab's socket (`$LAURA_TAB`) and exits. Client verbs are silent on success.

## Host

```bash
laura                   Run the TUI, hosting your default shell in tab 1.
laura -- <cmd> [args]   Run the TUI, hosting <cmd> in tab 1. New tabs still get your shell.
```

## Commands

A tab's panes form a split tree; the shell is pane `0` and never closes. Each `open` splits a pane and prints the **new pane id** on stdout. Capture it to address that pane later.

```bash
laura open <path>       Split a pane and render <path> in the new pane. Prints the new pane id.
                        With no split flags, auto-tiles: splits the newest pane, alternating
                        orientation by depth (a dwindle), so each bare open splits off the newest.
                        A split that would collapse a pane below the minimum errors (exit 1).
                        Relative paths resolve against the *calling* process's cwd, so `cd`
                        in the shell then `laura open ./x` works. Warns on stderr (still exit 0)
                        when the file can't be read (`cannot read <path>: …`) or the pane doesn't
                        fit (`overflows:` / `too small`).
      --split <id>      pane to split (default: the focused pane).
      --dir <h|v>       Split orientation: h side-by-side, v stacked (default: inferred).
      --ratio <1..99>   Percent of the split given to the new pane (default: inferred).
      --side <first|second>  Which side the new pane lands on (default: inferred).
      --no-focus        Don't move focus into the pane.
      --follow          Autoscroll: pin the cursor to the last line on open and every reload.
      --dry-run         Print the would-be overflow report; open nothing.
      --highlight <start> [end]  Highlight a line range once open (1-based, inclusive); end
                        defaults to start. Opens the pane already scrolled to and highlighting
                        the range: `laura open x.rs --highlight 40 52`. (See `laura highlight`
                        to highlight an already-open pane.)
      --diff            Open straight into the inline diff view (vs git HEAD).
      --panel <id>      Replace a pane's content in place (no new split; ignores --split/--dir/--ratio/--side).
                        Errors (exit 1) while the pane has an unsubmitted inline review.
laura close [<id>]      Close a pane (default: the focused one). Errors (exit 1) while the
                        pane has an unsubmitted inline review.
      --all             Close every pane, back to shell-only. Errors (exit 1), closing nothing,
                        while any pane has an unsubmitted inline review.
laura focus <id>        Focus a pane by id.
laura highlight [start] [end]
                        Highlight lines start..=end (1-based, inclusive) in a pane and
                        scroll them into view. end defaults to start (single line). Line numbers
                        are the file's real source lines (an editor / wc -l / git blame), for
                        markdown too — a source line inside a hand-wrapped paragraph maps to
                        that whole block: `laura highlight 40 52`. Errors (exit 1) on a
                        frozen pane.
      --off             Clear the pane's highlight (start is then optional; works on a frozen
                        pane too). The user can also press `h` on the focused pane.
      --pane <id>       pane to highlight (default: the focused pane).
laura diff              Toggle a pane's inline diff view vs git HEAD (interleaved +/- lines).
      --pane <id>       pane to toggle (default: the focused pane).
      --off             Turn the diff view off (default: toggle).
laura comment <line> <body>
                        Write to a line's thread: start a thread on that line or append a
                        reply to the user's thread there; the comment is attributed to the agent.
                        <line> is a 1-based real source line (same mapping as `laura highlight`).
                        `laura comment 42 "handled — see the retry loop"`. A <line> past
                        the file's end errors (exit 1) and places no comment.
      --author <name>   Attribute the comment to <name> (default: the session's
                        `ready --agent` name, else `agent`).
      --pane <id>       pane to comment on (default: the focused pane).
laura layout            Print the layout: per-pane rects + overflow (JSON).
laura ready             Mark the tab as hosting an agent (enables pane interactions). Prints the journal path.
      --session <id>    Name the journal session (default: laura-<pid>-<n>).
      --agent <name>    Attribute journal events to this agent name.
laura feedback          Append a feedback signal (layout/render quality, a missing tool) to the journal.
      --positive        Positive signal.        (one of --positive/--negative is required)
      --negative        Negative signal.
      [<body>]          Optional free-text body.
some-cmd | laura tail   Spool piped stdin to an internal file and show it in a tail pane.
                        The tail pane opens unfocused.
      --title <t>       Pane title (also names the spool file).
      --follow          Autoscroll to the newest line as output arrives.
```

Commands require `$LAURA_TAB` to be set — i.e. run them from inside a Laura-hosted shell. Outside a tab they error with `not inside a Laura tab (LAURA_TAB unset)`.

`layout` and `open --dry-run` both emit a JSON report — one entry per pane with its rect and overflow:

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

`overflow_rows > 0` (or `clipped`) means the content is taller than its pane. A real `open` warns the same condition on stderr as `overflows:` / `too small`.

## Journal

`ready` names a per-session append-only NDJSON journal and prints its path. Every `open`/`close`/`focus`/`comment`/`review`/`feedback` event is teed to it. A `review` event carries `error` when Laura couldn't write the inline review into the shell. Each event is stamped with `ts` (unix ms), `session`, `agent` (when set), and `version` — the build that emitted it: `X.Y.Z+<commit>` off a git checkout, bare `X.Y.Z` off a tarball. Files live under the OS data dir (`%APPDATA%` / `$XDG_DATA_HOME` / `~/Library/Application Support`) at `laura/sessions/<session>.ndjson`, overridable with `LAURA_DATA_DIR`. Read the latest with `cat "$(ls -t <dir>/laura/sessions/*.ndjson | head -1)" | jq .`.

## Global

```bash
laura --help            Print help (also per-subcommand: laura open --help).
laura --version         Print version.
```

The wire messages behind these verbs are documented in the [protocol](protocol.md).
