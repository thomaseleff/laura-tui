# Navigating the TUI

The following reference provides the keystrokes to navigate the Laura TUI workspace. Press `Ctrl+H` anytime from within Laura to pull up the same reference.

Keys Laura doesn't bind pass through to the shell.

## Global keys

| Key | Action |
|-----|--------|
| `Ctrl+P` | Panes popup — type a pane **id** then `Enter` to focus (single-digit ids focus on keypress) |
| `Ctrl+T` | Tab nav — `←/→` browse · `n` new tab · `x` close tab · `r` rename tab |
| `Ctrl+H` | Help |
| `Ctrl+Q` | Quit (then `y` to confirm) |
| `F12` | Lock all input to the shell |

## In a focused pane

| Key | Action |
|-----|--------|
| `↑/↓` | Move the cursor |
| `n/N` | Jump the cursor to the next / previous thread |
| `←/→` | Scroll pre-formatted lines sideways (code, diffs, markdown tables/fences/HTML) |
| `c` | Comment on a line: start a thread, edit or delete your last comment, or reply (needs `laura ready`) |
| `r` | Collapse/expand the cursor's thread — or, off a thread, all of them |
| `Shift+S` | Submit the inline review (needs `laura ready`) |
| `Ctrl+R` | Refresh the pane: discard the unsubmitted inline review and reload the file |
| `d` | Toggle inline diff vs git `HEAD` |
| `x` | Close the pane |
| `h` | Clear the pane's highlight |
| `Esc` | Leave the pane |

An inline review is a **call and response**. Comments and threads remain in the pane until you press `Shift+S` to submit the inline review. A pane with threads shows `[review: N ↑a ↓b]` on its border: `N` threads, `a` at or above the cursor, `b` below it. Use `n`/`Shift+N` to jump to the next / previous thread. If the inline review can't be written into the shell, the notice `⚠ inline review submission failed — Enter to retry · Esc to cancel` appears and the threads and review body are kept. `Enter` retries; `Esc` closes the review body and keeps the threads. The cause is recorded in the journal's `review` event. While you type a comment or review body, the agent's commands wait until you press `Enter` or `Esc`.

A file pane with an unsubmitted inline review **freezes** when its file changes on disk. A focused frozen pane shows the notice `⚠ <file> changed on disk — Shift+S submit · Ctrl+R refresh (discards unsubmitted inline review)` (shortened to `⚠ Shift+S submit · Ctrl+R discards inline review` on a narrow terminal). `Shift+S` submits against the snapshot (the inline review starts with a `⚠ file changed` banner), then the pane reloads. `Ctrl+R` refreshes. Deleting the pane's last comment also ends the freeze, and the pane reloads.

## Typing a comment or review body

| Key | Action |
|-----|--------|
| `←/→` | Move the cursor |
| `Home/End` | Jump to the start / end of the line |
| `Backspace/Delete` | Delete before / after the cursor |
| `\`+`Enter` | Insert a newline |
| `Enter` | Add the comment, save the edit, or submit the review body |
| `Esc` | Cancel |

To delete your comment, press `c` on it, clear the text, and press `Enter`.

## Scrolling

`PageUp/PageDown` and the mouse wheel scroll Laura's history on the main screen. On the alternate screen (claude, vim, less) they forward to the child, which owns its own scrollback (no scrollbar then).

In a file pane the view stays put while the cursor moves, and scrolls only when the cursor reaches its top or bottom edge. A comment opening or collapsing above the view doesn't move it. The mouse wheel scrolls the page and carries the cursor along. `laura highlight` centers its lines once and puts the cursor on the first highlighted line, so `c` right after a highlight comments there.

## Selection

A plain left **drag** selects within a pane and copies to the system clipboard on release (via OSC 52). From a file pane the copy is clean source — no line-number gutter, wrapped lines rejoined — while the shell copies its glyphs verbatim.
