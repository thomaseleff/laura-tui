# Agent reference

> [!TIP]
> The agent reference is for coding agents. For the keys the user presses in the workspace, see [Navigating the TUI](navigation.md).

## Enable interaction

```bash
laura ready
```

Run `laura ready` once per tab before opening a pane. Until you run `laura ready`, the user can't comment (`c`) or submit an inline review (`Shift+S`).

`laura ready` prints one JSON object: `journal` (the journal path), `experimental` (the experimental features enabled in the tab; use only those), and `layout` (what's already open, the same report as `laura layout`). Run `laura ready` at the start of every new chat, because panes from earlier chats may still be open. `layout` is `null` when the running Laura is older than the `laura` CLI; run `laura layout` instead.

Outside a Laura workspace, `laura ready` exits 1 with `not inside a Laura tab`.

Laura holds `laura` CLI commands while the user is typing a comment or review body. Do not kill or retry a command that is waiting.

## Preview layout

```bash
laura layout
laura open <path> --dry-run
```

`laura layout` and `laura open --dry-run` both print a JSON report with one entry per pane, its rect and overflow:

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

When `overflow_rows` is above 0, or `clipped` is `true`, the content overflows the pane and the user has to scroll to read the full content. `laura open` also warns on stderr when a file's content overflows (`pane #N overflows: …`), and when the pane is too small to show any of it (`pane #N too small to render …`).

## Open a pane

```bash
laura open <path>                     # auto-tile: split the newest pane, alternating orientation (dwindle); prints the new pane id
laura open <path> --split <id>        # override: split a specific pane instead
laura open <path> --dir v --ratio 30  # override: v stacks / h side-by-side; new pane gets 30%
laura open <path> --side first        # override: new pane lands on the first side of the split
laura open <path> --no-focus          # open without moving focus into the pane
laura open <path> --dry-run           # print the would-be overflow report; open nothing
laura open <path> --panel <id>        # swap a pane's file in place — same id, rect, focus
laura open <path> --edit              # run Neovim on the file in an editor pane; never takes focus
```

With no split flags, Laura tiles `laura open <path>` automatically: Laura splits the newest pane and alternates the orientation (a dwindle). Any split flag overrides the automatic tiling. When a split would shrink a pane below the minimum size, `laura open` returns an error (exit 1). `laura open` prints the new pane id; capture the id to target that pane later.

When you open a file that's already open, in a new split or with `--panel`, Laura still opens the file and warns `already open in pane #N`. Reuse pane N (`laura highlight --pane N`) unless you want to show two distinct sections of the same file. If pane N has an unsubmitted inline review, the warning ends with `, which has an unsubmitted inline review`. In that case, use the new pane and leave pane N to the user's inline review.

`laura open --panel` returns an error (exit 1) in two cases:

- The pane has an unsubmitted inline review. Ask the user to submit (`Shift+S`) or refresh (`Ctrl+R`).
- The pane is an editor pane with unsaved edits. Ask the user to save (`:w`) or quit Neovim.

Use `--edit` to open an **editor pane** when the user wants to edit the file:

- Laura never focuses an editor pane, so tell the user the editor pane is open and to press `Ctrl+P` then the pane id shown on its border.
- `--panel <id> --edit` on the file the pane already shows attaches Neovim to that pane and keeps its threads. On another file, `--panel <id> --edit` replaces the pane like `--panel`.
- Editor panes show no highlight or diff. `--edit` with `--highlight` or `--diff`, and `laura highlight` or `laura diff` on an editor pane, return an error (exit 1). Annotate the lines with `laura comment` instead; the user sees the thread count on the pane's border while editing.
- When `--edit` returns `editor panes are experimental: …` or `editor panes need Neovim: …` (exit 1), open the file with plain `laura open` and don't retry.

## Highlight a section for the user

```bash
laura open src/x.rs --highlight 40 52   # open a file already scrolled to and highlighting lines 40–52
laura highlight 40 52                    # highlight in an already-open pane (1-based, inclusive)
laura highlight 40 --pane <id>           # single line, in a specific (possibly unfocused) pane
laura highlight --off                     # clear the highlight (the user can also press `h` on the pane)
```

`laura open --highlight` opens the file and highlights the lines in one call, with the pane already scrolled to the range. `laura highlight` moves the highlight in an open pane. The highlight stays until you set a new one or clear it. On a frozen pane, `laura highlight` returns an error (exit 1), but `--off` still clears the highlight.

In markdown:

- Fenced code and HTML blocks, list items, blockquote lines, and callout lines keep one number per line, including blank lines.
- Laura shows a hand-wrapped paragraph as one block, and highlights the whole block for any of the paragraph's lines.
- Laura does the same for a fenced block or table inside a list item or blockquote.

## Show what changed

Laura marks each line changed since git `HEAD` in a file pane's gutter: a **green** bar on added lines, **blue** on modified lines, and a dim-**red** `── N lines removed ──` row where lines were deleted. Laura updates the markers when the file reloads.

- Laura needs `git` on `PATH` for the markers. Without `git`, `laura open` warns `diff markers unavailable` on stderr, and Laura shows a notice and no markers.
- In markdown, Laura marks the rendered row whose paragraph or block holds the changed source line.
- Laura shows no markers on an untracked file until the file is committed.

## See the full diff inline

Laura marks *where* lines changed in the gutter. In the diff view, Laura shows *what* changed: Laura replaces the pane's content with an interleaved `+`/`-` diff against git `HEAD`, with deleted lines as red `-` rows holding their old text.

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

`laura close` returns an error (exit 1) in two cases, and with `--all` closes nothing:

- The pane has an unsubmitted inline review. Ask the user to submit (`Shift+S`) or refresh (`Ctrl+R`).
- The pane is an editor pane with unsaved edits. Ask the user to save (`:w`) or quit Neovim.

## Read an inline review

When the user submits an inline review, you receive a `[laura review · <path>]` block in the chat. Treat the block as feedback on that file: address each `L<n>` thread (line numbers are 1-based) and the review body, then edit the file.

Each `L<n>` is a thread, and each comment starts with its author (`> [user] …`, `> [agent] …`). An inline review is a **call and response**: the block holds the whole conversation on that file. You only see the user's comments when the user submits (`Shift+S`); you can't read them from the pane.

## Annotate a file

```bash
laura comment 42 "handled — see the retry loop"   # reply on line 42's thread (starts one if none)
laura comment 42 "…" --author reviewer            # attribute to a name (default: the session's ready --agent, else agent)
laura comment 42 "…" --pane <id>                  # target a specific (possibly unfocused) pane
```

`<line>` is a 1-based source line (same mapping as `laura highlight`), and `<body>` is required. When `<line>` is past the file's end, `laura comment` returns an error (exit 1) and places no comment.

Annotate only when you need the user's answer in a thread. Your comment starts an unsubmitted inline review that only the user can submit or refresh. Until the user does, you can't close or replace the pane.

If you edit a file while the user has an unsubmitted inline review on it, Laura freezes the pane until the user submits or refreshes. An inline review from a frozen pane starts with a `⚠ file changed` banner, and its line numbers refer to the snapshot, not your edited file. Each thread header quotes its source line; use the quoted text to find the line in the edited file.
