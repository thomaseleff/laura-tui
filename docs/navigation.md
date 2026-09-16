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
| `←/→` | Scroll pre-formatted lines sideways (code, diffs, markdown tables/fences/HTML) |
| `c` | Comment on a line |
| `Shift+s` | Submit the review |
| `d` | Toggle inline diff vs git `HEAD` |
| `x` | Close the pane |
| `h` | Clear the pane's highlight |
| `Esc` | Leave the pane |

## Scrolling

`PageUp/PageDown` and the mouse wheel scroll Laura's history on the main screen. On the alternate screen (claude, vim, less) they forward to the child, which owns its own scrollback (no scrollbar then).

## Selection

A plain left **drag** selects within a pane and copies to the system clipboard on release (via OSC 52). From a file pane the copy is clean source — no line-number gutter, wrapped lines rejoined — while the shell copies its glyphs verbatim.
