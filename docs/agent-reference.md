# Agent reference

> [!TIP]
> This reference is for coding agents. See [Navigating the TUI](navigation.md) on how to move around the Laura TUI workspace.

## Enable interaction

```bash
laura ready
```

Run once per tab before opening a pane. Inline review interactions within panes are unavailable until `laura ready` is run.

A `laura` command waits while the user is typing a comment or review body, and returns once they press `Enter` or `Esc`.

## Preview layout

```bash
laura layout
laura open <path> --dry-run
```

Both emit a JSON report of the current panes in the workspace:

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

## Open a pane

```bash
laura open <path>                     # auto-tile: split the newest pane, alternating orientation (dwindle); prints the new pane id
laura open <path> --split <id>        # override: split a specific pane instead
laura open <path> --dir v --ratio 30  # override: v stacks / h side-by-side; new pane gets 30%
laura open <path> --side first        # override: new pane lands on the first side of the split
laura open <path> --no-focus          # open without moving focus into the pane
laura open <path> --dry-run           # print the would-be overflow report; open nothing
laura open <path> --panel <id>        # swap a pane's file in place — same id, rect, focus
```

A bare `laura open <path>` dynamically tiles automatically in a dwindle pattern. Any split flag overrides it. A split that would collapse a pane errors (exit 1). `open` prints the new pane id - capture it to target that pane later.

Opening a file that's already open, in a new split or with `--panel`, still opens it and warns `already open in pane #N`. Reuse pane N (`laura highlight --pane N`) unless you intend to show two distinct sections or renders of the same file. If pane N has an unsubmitted inline review, the warning says so instead; work in the new pane.

`--panel` errors (exit 1) while the pane has an unsubmitted inline review. Ask the user to submit (`Shift+S`) or refresh (`Ctrl+R`).

## Highlight a section for the user

```bash
laura open src/x.rs --highlight 40 52   # open a file already scrolled to and highlighting lines 40–52
laura highlight 40 52                    # highlight in an already-open pane (1-based, inclusive)
laura highlight 40 --pane <id>           # single line, in a specific (possibly unfocused) pane
laura highlight --off                     # clear the highlight (the user can also press `h` on the pane)
```

`open --highlight` opens the file and highlights the lines in one call; the pane first paints at the range. `laura highlight` re-highlights an open pane. The highlight stays until you re-set or clear it. On a frozen pane, `highlight` errors (exit 1); `--off` still clears. In markdown, a hand-wrapped paragraph is one row, so any of its source lines highlights the whole block. Fenced code and HTML blocks keep per-line numbers.

## Show what changed

A file pane marks each line changed since git `HEAD` in the gutter: a **green** bar on added lines, **blue** on modified, and a dim-**red** `── N lines removed ──` row where lines were deleted. Markers update on reload.

- Markers need `git` on `PATH`. Without it, `laura open` warns `diff markers unavailable` on stderr and shows a notice; markers stay off.
- In markdown, a marker lights the rendered row whose paragraph or block covers the changed source line.
- An untracked file has no markers until it's committed.

## See the full diff inline

The gutter marks *where* lines changed; the diff view shows *what* changed — the pane body becomes an interleaved `+`/`-` diff vs git `HEAD`, deleted lines visible as red `-` rows with their old text.

```bash
laura open src/x.rs --diff       # open straight into the diff view
laura diff --pane <id>           # toggle the diff view on an open pane
laura diff --pane <id> --off     # back to the normal file view
```

## Close a pane

```bash
laura close            # close the focused pane
laura close <id>       # close a specific pane by id
laura close --all      # close every pane, back to shell-only
```

Closing a pane with an unsubmitted inline review errors (exit 1). With `--all`, nothing closes. Ask the user to submit (`Shift+S`) or refresh (`Ctrl+R`).

## Read an inline review

When a `[laura review · <path>]` block arrives in your chat, treat it as feedback on that file: address each `L<n>` thread (line numbers are 1-based) and the review body, then edit the file.

Each `L<n>` is a thread; its comments are labelled by author (`> [user] …`, `> [agent] …`). An inline review is a **call and response**: the block is the whole conversation on that file. The user's comments reach you only when they submit (`Shift+S`); you can't read them off the pane.

## Annotate a file

```bash
laura comment 42 "handled — see the retry loop"   # reply on line 42's thread (starts one if none)
laura comment 42 "…" --author reviewer            # attribute to a name (default: the session's ready --agent, else agent)
laura comment 42 "…" --pane <id>                  # target a specific (possibly unfocused) pane
```

`<line>` is a 1-based real source line (same mapping as `laura highlight`). `<body>` is required. A `<line>` past the file's end errors (exit 1) and places no comment. A comment starts an unsubmitted inline review that only the user can submit or refresh. Until they do, the pane can't be closed or replaced. Annotate only when you need the user's answer in a thread. If you edit a file while the user has an unsubmitted inline review on it, its pane freezes until they submit or refresh. An inline review from a frozen pane starts with a `⚠ file changed` banner, and each thread header carries its raw source line. Re-map your comments against those lines.
