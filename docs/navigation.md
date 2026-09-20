# Navigating the TUI

The following reference provides the keystrokes to navigate the Laura TUI workspace. Press `Ctrl+H` anytime from within Laura to pull up the same reference.

All keystrokes not recognized as Laura commands are always passed directly into the PTY automatically, allowing you to interact with your coding agent CLI.

## Global keys

| Key | Action |
|-----|--------|
| `Ctrl+P` | Panes popup — type a pane **id** then `Enter` to focus (single-digit ids focus on keypress) |
| `Ctrl+T` | Tab nav — `←/→` browse · `n` new tab · `x` close tab |
| `Ctrl+H` | Help |
| `Ctrl+Q` | Quit (then `y` to confirm) |
| `F12` | Lock all input to the shell |

## In a focused pane

| Key | Action |
|-----|--------|
| `↑/↓` | Move |
| `n/N` | Jump the cursor to the next / previous comment thread |
| `←/→` | Scroll pre-formatted lines sideways (code, diffs, markdown tables/fences/HTML) |
| `c` | Comment on a line (start a thread, edit your last note, or reply) |
| `r` | Collapse/expand the cursor's review thread — or, off a thread, all of them |
| `Shift+s` | Submit the review — ships every thread as one block, then clears the pane |
| `Ctrl+R` | Refresh the pane to the on-disk file, discarding any pending comments |
| `d` | Toggle inline diff vs git `HEAD` |
| `x` | Close the pane |
| `h` | Clear the pane's highlight |
| `Esc` | Leave the pane |

A per-line comment is a **call and response**: notes ride the pane while you work, `Shift+S` ships them all as one review block, and the pane clears — nothing outlives the submit. When a pane holds threads, its border reads `[review: N ↑a ↓b]` — `N` threads (what a submit would carry), `a` at or above the cursor and `b` below it, so you can tell a comment sits off-screen before scrolling to it. `n`/`N` jump straight there, wrapping around the ends — `n` past the last thread lands on the first.

If the file changes on disk while you have comments open, the pane **freezes** on its current snapshot rather than reloading out from under your notes; a standing bottom-right warning names both exits. `Shift+S` submits against the snapshot (the review carries a `⚠ file changed` banner) then the pane catches up to disk; `Ctrl+R` discards the pending comments and refreshes to disk.

## Scrolling

`PageUp/PageDown` and the mouse wheel scroll Laura's history on the main screen. On the alternate screen (claude, vim, less) they forward to the child, which owns its own scrollback (no scrollbar then).

## Selection

A plain left **drag** selects within a pane and copies to the system clipboard on release (via OSC 52). From a file pane the copy is clean source — no line-number gutter, wrapped lines rejoined — while the shell copies its glyphs verbatim.
