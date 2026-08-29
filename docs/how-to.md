# How-to

Task recipes for common Laura actions. For a guided first run, see the [tutorial](tutorial.md).

## Open a panel (split a pane)

```bash
laura open <path>                     # split the focused pane, render <path>; prints the new pane id
laura open <path> --split <id>        # split a specific pane instead of the focused one
laura open <path> --dir v --ratio 30  # v stacks / h side-by-side; first pane gets 30%
laura open <path> --side first        # new panel lands on the first side of the split
laura open <path> --no-focus          # open without moving focus into the panel
laura open <path> --dry-run           # print the would-be overflow report; open nothing
```

Every `open` splits a pane, so panels accumulate — a tab holds as many as you arrange. Capture the printed id to target that pane later. Check fit before (or without) committing with `laura layout` or `--dry-run`: both print per-pane rects and overflow as JSON.

## Close a panel

```bash
laura close            # close the focused panel
laura close <id>       # close a specific panel by id
laura close --all      # close every panel, back to shell-only
```

## Enable review (mark the tab ready)

```bash
laura ready
```

Run once per tab before commenting or submitting a review. Fails closed: no `ready`, no review injection.

## Focus a panel and move around

`Ctrl+P` opens the panes popup; press a pane's **digit** to focus it (or run `laura focus <id>` from the shell). `↑`/`↓` move the line cursor; the mouse wheel scrolls too. `Esc` leaves focus and returns keys to the shell.

## Comment on a line

With the panel focused, put the cursor on a line, press `c`, type the comment, `Enter` to save (`Esc` to cancel). Multiple comments per line are fine. The title shows `[review: n]`.

## Submit a review

Press `S`, type an overall note (optional), `Enter`. The assembled `[laura review · <path>]` block is injected into the agent's shell. Submitting clears the panel's comments.

## Read a review (as the agent)

When a `[laura review · <path>]` block arrives in the agent's input, treat it as feedback on that file: address each `L<n>` comment (line numbers are 1-based) and the overall note, then edit the file. See the [protocol](protocol.md#review-payload) for the exact shape.

## Manage tabs

`Ctrl+T` enters tab mode: `←`/`→` browse tabs, `n` opens a new one, `Enter` drops into the focused tab. A tab closes when its shell exits; when the last tab closes, Laura quits. `Ctrl+Q` quits immediately.

## Run a specific program instead of your shell

```bash
laura -- <cmd> [args…]   # tab 1 hosts <cmd>; new tabs still get your default shell
```
