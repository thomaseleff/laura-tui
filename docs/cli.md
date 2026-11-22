# CLI reference

The CLI reference lists every `laura` command and its flags. Run `laura` on its own to start Laura. Run `laura <command>` in a shell inside Laura to open files in panes, highlight lines, comment, and more. Each command sends one message to its workspace over the [protocol](protocol.md) and exits. Commands print nothing on success unless noted below.

## Host

```bash
laura                   Show the workspace in the web app, hosting your default shell. Prints
                        the URL, and opens it in a web browser when Laura runs in a terminal.
laura -- <cmd> [args]   Show the web app, hosting <cmd> instead of your shell.
laura --tui [-- <cmd>]  Show the workspace in the terminal (the TUI) instead.
laura --web [-- <cmd>]  The same as `laura`.
laura --port <n>        Serve the web app on port <n> instead of a free port.
laura --serve           Run the Laura server at http://127.0.0.1:1989/, which lists the running
                        workspaces and starts new ones. Prints the URL with the key. Takes no <cmd>.
laura --serve --port <n>  Run the Laura server on port <n>. `--port 0` picks a free port.
laura --serve --open    Open the Laura server in a web browser, and start the Laura server when it
                        does not run. The Laura icon runs this command.
laura --shortcut        Write the Laura icon for this computer. Prints the path of each file.
```

When a Laura server already runs, `laura --serve` prints the URL of that Laura server again, also with a different `--port`. When another program uses the port, Laura returns the error `port <n> is in use: pass --port`. `laura --serve -- <cmd>` returns the error `--serve takes no program: start a workspace with a program on the Laura server's page`. See [The Laura server](workspace.md#the-laura-server).

With `--open`, Laura opens the browser at the Laura server that runs, or starts a Laura server and opens the browser at it. When a Laura server of another version runs, Laura stops that Laura server and starts a new one. When another program uses the port, Laura returns the error `Another program uses port <n>. Stop that program, then open Laura from your desktop or app menu, or run laura --serve again.` When the Laura server can't start, Laura also opens a page with the error, and exits with status 1. See [Start Laura from a desktop icon](workspace.md#start-laura-from-a-desktop-icon).

`laura --shortcut -- <cmd>` returns the error `--shortcut takes no program`.

`lauraw` is the program that the Laura icon runs on Windows. `lauraw` runs `laura --serve --open` with no console window. Laura installs `lauraw` on every system. On Linux and macOS, `lauraw` also runs `laura --serve --open`.

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
laura open <url>        Show an http(s) URL in a browser pane (needs the web app, and Chrome
                        or Edge). Takes the split flags, --no-focus and --panel. Errors
                        (exit 1) with --highlight, --diff or --edit. --follow changes nothing.
                        See Browser panes below.
laura close [<id>]      Close a pane (default: the focused one). Errors (exit 1) while the
                        pane has an unsubmitted inline review or unsaved Neovim edits.
      --all             Close every pane, back to shell-only. Errors (exit 1), closing nothing,
                        while any pane has an unsubmitted inline review or unsaved Neovim edits.
laura focus <id>        Focus a pane by id.
laura highlight [start|selector] [end]
                        Highlight lines start..=end (1-based, inclusive) in a pane and
                        scroll them into view. In a browser pane, pass a CSS selector in place
                        of the lines. See Browser panes below. end defaults to start (single line). Line numbers
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
laura comment <line|selector> <body>
                        Write to a line's thread: start a thread on that line or append a
                        reply to the user's thread there; the comment is attributed to the agent.
                        <line> is a 1-based real source line (same mapping as `laura highlight`).
                        In a browser pane, pass a CSS selector in place of the line. See
                        Browser panes below.
                        `laura comment 42 "handled — see the retry loop"`. A <line> past
                        the file's end errors (exit 1) and places no comment.
      --author <name>   Attribute the comment to <name> (default: the session's
                        `ready --agent` name, else `agent`).
      --pane <id>       pane to comment on (default: the focused pane).
laura layout            Print the layout: per-pane rects + overflow (JSON).
laura ready             Mark the workspace as hosting an agent (enables commenting and inline reviews). Prints the journal path,
                        the experimental features on, and the layout (JSON).
      --session <id>    Name the journal session (default: laura-<pid>-<n>).
      --agent <name>    Attribute journal events to this agent name, and name the shell after it.
      --title <text>    Label the session in a few words: the session label.
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
                        the tail pane closes. The user can pause the tail pane (`p`) to
                        read it.
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

Editor panes report `"kind": "editor"` and `content_rows: null`, because Neovim sizes its own content. Browser panes report `"kind": "browser"`, the URL as `path`, the page's `title`, and `content_rows: null`. A pane that the user [paused](navigation.md#in-a-focused-pane) reports `"paused": true`, because the user doesn't see its new output. While the web app shows the shell in the [drawer](navigation.md#the-drawer), the shell reports `"drawer": true`, and the other panes fill the workspace. See [the `layout` report](protocol.md#layout) for how `--split 0` works then.

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

## Browser panes

`laura open <url>` shows a page from an `http://` or `https://` URL in a **browser pane** in the web app. For what the user sees, see [Show a web app in a browser pane](workspace.md#show-a-web-app-in-a-browser-pane). To use browser panes:

1. Install Chrome or Edge. To use another browser of the same kind, set `LAURA_BROWSER` to its path.
2. Start Laura with `laura`.

`laura open <url>` returns these errors, in this order:

- In the TUI: `browser panes need the web app: start Laura without --tui`.
- With no Chrome or Edge, or a `LAURA_BROWSER` path with no file: `browser panes need Chrome or Edge: install one or set LAURA_BROWSER to its path`.
- With `--highlight`: `a browser pane has no lines: open the page, then run laura highlight '<selector>' --pane <id>`.
- With `--diff`: `a browser pane has no diff: run laura open <file> --diff on the page's source file`.
- With `--edit`: `a browser pane has no editor: run laura open <file> --edit on the page's source file`.
- When Laura's browser can't start: `couldn't start Laura's browser: <reason>`. Laura records Chrome's output in the journal's [`browser` event](#journal).

`laura open` uses any other text, such as `file:///etc/hosts` or `chrome://version`, as a file path.

In a browser pane, the agent passes a CSS selector in place of a line number:

- `laura comment '<selector>' "<body>" --pane <id>` starts a thread on the element that the selector finds. When the element already has a thread, the comment is a reply on that thread.
- `laura highlight '<selector>' --pane <id>` highlights the element and scrolls the page to it. `laura highlight --off --pane <id>` clears the highlight.

Laura looks for the element on the page that the pane shows now. An argument of digits only is a line number, and any other argument is a selector.

`laura comment` and `laura highlight` return these errors, in this order:

- With a selector and an end line: `a selector takes no end line`.
- With a line number on a browser pane: `pane #<id> is a browser pane: pass a CSS selector`.
- With a selector on a file pane: `pane #<id> is a file pane: pass a line number`.
- When Laura's browser stopped: `Laura's browser exited: press Ctrl+R in a browser pane to start it again`.
- When the page shows a dialog: `pane #<id>'s page is showing a dialog: answer it with your browser tool, or ask the user to answer it in the pane`.
- When the page is still loading: `pane #<id>'s page is still loading: run the command again when the page has loaded`.
- With a selector that is not valid CSS: `invalid selector: <selector>`.
- When no element on the page matches: `no element matches <selector> in pane #<id>`.

`laura diff --pane <id>` on a browser pane returns `a browser pane has no diff: run laura open <file> --diff on the page's source file`.

## Settings

Laura saves your settings in `~/.laura/settings.json` and uses them in each workspace. Laura writes the file when you change a setting. To go back to a default, remove the key from the file, then start a new workspace.

If Laura can't read `settings.json`, for example after a typo in a hand edit, Laura uses the defaults and doesn't change the file. A setting that you change then applies to that workspace only. Fix the file, then start a new workspace.

Laura also keeps `~/.laura/settings.lock`, an empty file, so that two workspaces don't save a setting at the same moment.

| Key | What Laura does with the value | You set it with |
|---|---|---|
| `drawer` | Shows the drawer at this height, in % of the workspace, from 20 to 85. Without the key, the drawer is 54%. | [The drawer's grip](navigation.md#the-drawer) |
| `editor_theme` | `laura` applies Laura's theme in editor panes, and `neovim` keeps your theme. Without the key, the TUI asks at start. | [The startup wizard](tui-navigation.md#lauras-theme) |

An earlier Laura saved the theme in `~/.laura/editor-theme`. Laura moves the answer into `settings.json` at the next start. If Laura can't write `settings.json`, Laura uses the answer in `~/.laura/editor-theme` and moves the answer at a later start.

## Journal

When the agent runs `laura ready`, Laura starts a journal for the session at `~/.laura/sessions/<session>.ndjson` and prints its path as `journal`. Laura adds one NDJSON line to the journal for each event in the table below. Each line includes `ts` (unix ms), `session`, `agent` (when set), and `version` (Laura's version, plus `+<commit>` when Laura was built from a git checkout). A `review` event includes `error` when Laura couldn't write the inline review into the chat. Laura adds a `browser` event when Laura's browser doesn't start.

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
| `ready` | `pid` (Laura's process id), `title` |
| `open` | `pane`, `path`, `replaced`, `edit` |
| `close` | `pane`, or `all` |
| `focus` | `pane` |
| `highlight` | `pane`, `start`, `end`, or `selector`, or `cleared` |
| `diff` | `pane`, `on` |
| `comment` | `pane`, `line` or `selector` |
| `review` | `pane`, `path`, `comments` (thread count), `body`, `error` |
| `feedback` | `sentiment` (`+` / `-`), `body`, `pane` |
| `browser` | `error` (why Laura's browser didn't start), `output` (the last 10 lines of Chrome's output) |

Every event also carries `ts`, `version`, `session`, and `agent` when `ready --agent` named one. `edit` and `replaced` appear only when true; `ready`'s `title` is `null` when omitted or blank, `feedback`'s `body` is `null` when omitted, and `feedback`'s `pane` is `null` when the shell has focus.

## Global

```bash
laura --help            Print help (also per-subcommand: laura open --help).
laura --version         Print version.
```

The [protocol reference](protocol.md) documents the messages behind these commands.
