# CLI reference

The CLI reference lists every `laura` command and its flags. Run `laura` on its own to start Laura. Run `laura <command>` in a shell inside Laura to open files in panes, highlight lines, comment, and more. Each command sends one message to its workspace over the [protocol](protocol.md) and exits. Commands print nothing on success unless noted below.

## Host

```bash
laura                   Run the TUI, hosting your default shell.
laura -- <cmd> [args]   Run the TUI, hosting <cmd> instead of your shell.
```

## Commands

Laura arranges the workspace's panes in a split tree. The shell is pane `0`, and Laura never closes the shell. `laura open` splits a pane and prints the **new pane id** on stdout. Capture the id to target that pane later.

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
                        Errors (exit 1) while the pane has an unsubmitted inline review or
                        unsaved Neovim edits.
      --edit            Edit in Neovim (experimental: needs LAURA_EXPERIMENTAL_EDITOR=1 and nvim
                        on PATH). Opens an editor pane and never takes focus. With --panel on
                        the same file, attaches Neovim in place and keeps the pane's threads.
                        Errors (exit 1) with the reason when editor panes are off, and with
                        --highlight or --diff: editor panes show neither, so annotate the
                        lines with `laura comment` instead.
laura close [<id>]      Close a pane (default: the focused one). Errors (exit 1) while the
                        pane has an unsubmitted inline review or unsaved Neovim edits.
      --all             Close every pane, back to shell-only. Errors (exit 1), closing nothing,
                        while any pane has an unsubmitted inline review or unsaved Neovim edits.
laura focus <id>        Focus a pane by id.
laura highlight [start] [end]
                        Highlight lines start..=end (1-based, inclusive) in a pane and
                        scroll them into view. end defaults to start (single line). Line numbers
                        are the file's real source lines (an editor / wc -l / git blame), for
                        markdown too — a source line inside a hand-wrapped paragraph maps to
                        that whole block: `laura highlight 40 52`. Errors (exit 1) on a
                        frozen pane, and on an editor pane (annotate the lines with
                        `laura comment` instead).
      --off             Clear the pane's highlight (start is then optional; works on a frozen
                        pane too). The user can also press `h` on the focused pane.
      --pane <id>       pane to highlight (default: the focused pane).
laura diff              Toggle a pane's inline diff view vs git HEAD (interleaved +/- lines).
                        Errors (exit 1) on an editor pane (annotate the lines with
                        `laura comment` instead).
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
laura ready             Mark the workspace as hosting an agent (enables commenting and inline reviews). Prints the journal path,
                        the experimental features on, and the layout (JSON).
      --session <id>    Name the journal session (default: laura-<pid>-<n>).
      --agent <name>    Attribute journal events to this agent name.
laura feedback          Append a feedback signal (layout/render quality, a missing tool) to the journal.
      --positive        Positive signal.        (one of --positive/--negative is required)
      --negative        Negative signal.
      [<body>]          Optional free-text body.
laura journal [FIELD=VALUE…]
                        Print journaled events across every session (NDJSON, oldest first).
                        Works outside a workspace. Matching rules under Journal below.
      --since <dur>     Only events newer than <dur>: 30m, 24h, 7d.
      -n <N>            Print the newest N matches (default 50); -n 0 prints only the count.
some-cmd | laura tail   Spool piped stdin to an internal file and show it in a tail pane.
                        The tail pane opens unfocused. The spool lives in a `laura` dir in the
                        per-user temp dir (`~/.laura/runtime/laura/` on Linux when `XDG_RUNTIME_DIR` is
                        unset or Laura can't create a dir in it) and is deleted when the tail
                        pane closes or Laura quits; a crash leaves it in the temp dir. When
                        the agent's shell can't write to Laura's spool dir, `laura tail` spools
                        into the shell's own temp dir, and Laura doesn't delete that spool when
                        the tail pane closes.
      --title <t>       Pane title (also names the spool file).
      --follow          Autoscroll to the newest line as output arrives.
```

Commands work only inside Laura. Laura sets `$LAURA_TAB` in each shell, and each command uses `$LAURA_TAB` to find its workspace. Outside Laura, commands exit 1 with `not inside a Laura workspace (LAURA_TAB unset)`. The exception is `laura journal`, which reads local files and works anywhere.

`laura layout` and `laura open --dry-run` both print a JSON report with one entry per pane, its rect and overflow:

```json
{
  "area": {"x": 0, "y": 0, "width": 120, "height": 40},
  "panes": [
    {"id": 0, "kind": "pty",   "path": null,       "rect": {"x":0,"y":0,"width":48,"height":40},
     "content_rows": null, "visible_rows": 38, "overflow_rows": 0,  "clipped": false},
    {"id": 1, "kind": "panel", "path": "spec.md",  "rect": {"x":48,"y":0,"width":72,"height":40},
     "content_rows": 120, "visible_rows": 38, "overflow_rows": 82, "clipped": true}
  ]
}
```

`overflow_rows` counts the rows that don't fit in the pane. When it's above 0, or `clipped` is `true`, the content **overflows** the pane and the user has to scroll to read the full content. `laura open` also warns on stderr when a file's content overflows (`pane #N overflows: …`), and when the pane is too small to show any of it (`pane #N too small to render …`).

Editor panes report `"kind": "editor"` and `content_rows: null`, because Neovim sizes its own content.

## Experimental features

Experimental features demonstrate a component or interaction before the full pattern for usage or implementation of the feature is known.

Experimental features are off by default and enabled by setting an environment variable before starting Laura. `laura --help` lists the experimental features, and `laura ready` lists them under the `experimental` field so agents know which are active in the session:

```
"experimental": ["editor"]
```

| Variable | Feature |
|---|---|
| `LAURA_EXPERIMENTAL_EDITOR=1` | [Editor panes](#editor-panes) |

## Editor panes

> [!WARNING]
> Editor panes are [experimental](#experimental-features).

`laura open --edit <path>` opens a file through Neovim in an **editor pane**. To enable editor panes:

1. Set `LAURA_EXPERIMENTAL_EDITOR=1`.
2. Install Neovim with `nvim` on `PATH`. Laura inherits `PATH` from the terminal, so restart the terminal after installing.
3. Start Laura.

When disabled, `laura open --edit` returns `editor panes are experimental: …`. When the environment variable is set but Neovim isn't installed, `laura open --edit` returns `editor panes need Neovim: …`, and Laura shows a notice at startup.

When Neovim exits, Laura closes the editor pane. If the editor pane has threads, Laura keeps the pane open as a file pane instead, so the unsubmitted inline review isn't lost.

Editor panes show no highlight or diff. `--edit` with `--highlight` or `--diff`, and `laura highlight` or `laura diff` on an editor pane, return an error (exit 1). When a file pane becomes an editor pane, Laura drops the pane's highlight and diff.

Editor panes support comments. `laura comment --pane <id>` annotates a line, and Neovim shows the thread as an inline `✎` diagnostic. The user presses `Ctrl+L` to flip the pane between its editor view (Neovim) and its file view. The editor view shows comments, but the user can't comment or submit there, so the user flips to the file view to comment (`c`) or submit an inline review (`Shift+S`).

## Journal

When the agent runs `laura ready`, Laura starts a journal for the session at `~/.laura/sessions/<session>.ndjson` and prints its path as `journal`. Laura adds one NDJSON line to the journal for each event in the table below. Each line includes `ts` (unix ms), `session`, `agent` (when set), and `version` (Laura's version, plus `+<commit>` when Laura was built from a git checkout). A `review` event includes `error` when Laura couldn't write the inline review into the chat.

Read the journals back with `laura journal`, which merges every session and prints one event per line, unchanged:

```bash
laura journal                                   # newest 50 events, every session, oldest first
laura journal type=feedback sentiment=+         # positive feedback only
laura journal type=review --since 24h           # inline reviews from the last day
laura journal type=feedback type=review -n 20   # the 20 newest of either
laura journal agent=claude session=plan-84-16
```

- `FIELD=VALUE` matches a top-level field. A string field compares as is (`sentiment=+`); any other value compares by its JSON text (`pane=2`, `body=null`; a missing field reads as `null`).
- Repeat a field to match any of its values (`type=feedback type=review`). Different fields must all match.
- `--since <duration>` takes `30m`, `24h` or `7d` and keeps events with `ts` inside the window.
- `-n <N>` keeps the newest N matches (default 50), printed oldest first. When `laura journal` leaves out matches, it prints one line on stderr: `showing 50 of 812 events; pass -n for more`. `-n 0` prints only that line, so you can count matches without printing them (no line means 0).

Each event's fields:

| `type` | Fields |
|---|---|
| `ready` | — |
| `open` | `pane`, `path`, `replaced`, `edit` |
| `close` | `pane`, or `all` |
| `focus` | `pane` |
| `highlight` | `pane`, `start`, `end`, or `cleared` |
| `diff` | `pane`, `on` |
| `comment` | `pane`, `line` |
| `review` | `path`, `comments` (thread count), `body`, `error` |
| `feedback` | `sentiment` (`+` / `-`), `body`, `pane` |

Every event also carries `ts`, `version`, `session`, and `agent` when `ready --agent` named one. `edit` and `replaced` appear only when true; `feedback`'s `body` is `null` when omitted and its `pane` is `null` when the shell has focus.

## Global

```bash
laura --help            Print help (also per-subcommand: laura open --help).
laura --version         Print version.
```

The [protocol reference](protocol.md) documents the messages behind these commands.
