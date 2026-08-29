# How-to

Task recipes for common Laura actions. For a guided first run, see the [tutorial](tutorial.md).

## Open or replace a panel

```bash
laura open <path>            # renders <path> beside the shell; focuses the panel
laura open <path> --no-focus # open without moving focus into the panel
```

Opening another file replaces the current panel (one panel per tab).

## Close the panel

```bash
laura close
```

## Enable review (mark the tab ready)

```bash
laura ready
```

Run once per tab before commenting or submitting a review. Fails closed: no `ready`, no review injection.

## Focus the panel and move around

`Ctrl+T` then `p` focuses the panel. `↑`/`↓` move the line cursor; the mouse wheel scrolls too. `Esc` leaves focus and returns keys to the shell.

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
