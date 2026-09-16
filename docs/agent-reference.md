# Agent reference

> [!TIP]
> This reference is for coding agents. See [Navigating the TUI](navigation.md) on how to move around the Laura TUI workspace.

## Enable interaction

```bash
laura ready
```

Run once per tab before opening a pane. Fails closed: no `ready`, no review injection.

## Open a pane

```bash
laura open <path>                     # split the focused pane, render <path>; prints the new pane id
laura open <path> --split <id>        # split a specific pane instead of the focused one
laura open <path> --dir v --ratio 30  # v stacks / h side-by-side; new pane gets 30%
laura open <path> --side first        # new pane lands on the first side of the split
laura open <path> --no-focus          # open without moving focus into the pane
laura open <path> --dry-run           # print the would-be overflow report; open nothing
```

Every `open` splits a pane, so panes accumulate — a tab holds as many as you arrange. Capture the printed id to target that pane later. Check fit before (or without) committing with `laura layout` or `--dry-run`: both print per-pane rects and overflow as JSON.

## Highlight a section for the reader

```bash
laura open src/x.rs --highlight 40 52   # open a file already scrolled to and highlighting lines 40–52
laura highlight 40 52                    # highlight in an already-open pane (1-based, inclusive)
laura highlight 40 --pane <id>           # single line, in a specific (possibly unfocused) pane
```

The common gesture is "show me where": nothing's on screen (or a stale doc is), so `open --highlight` opens the file *and* highlights the lines in one call — the pane paints already at the span, no line-1 flash. Once a file's open, `laura highlight` re-highlights it. Either way the highlight stays until you re-set it. In markdown, a hand-wrapped paragraph collapses onto one row so a number inside it snaps to the block, but fenced code and HTML blocks keep per-line numbers.

## Show what changed

An open pane marks each line changed since git `HEAD` in the gutter, VSCode-style: a **green** bar on added lines, **blue** on modified, and a dim-**red** `── N lines removed ──` row where lines were deleted. Markers refresh on reload, so they track your edits as you save.

Nothing to run — it's automatic for tracked code/text files.

**Notes:**

- Needs `git` on `PATH`. Without it, `laura open` warns on stderr and shows a brief
  toast; markers stay off.
- Markdown gets markers too: they key off the source line, so a bar lights on the
  rendered row whose paragraph/block covers a changed line. An untracked file has
  none until it's committed.

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

## Read a review

When a `[laura review · <path>]` block arrives in your conversation, treat it as feedback on that file: address each `L<n>` comment (line numbers are 1-based) and the overall note, then edit the file.
