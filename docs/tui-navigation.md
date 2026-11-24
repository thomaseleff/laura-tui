# Navigating the TUI

Laura started as a TUI workspace, and the TUI is still supported. To show the workspace in the terminal, run `laura --tui`. The TUI uses the keystrokes in [Navigating the workspace](navigation.md). This page lists what is different.

In the TUI, `laura open <url>` returns `browser panes need the web app: start Laura without --tui`. See [Browser panes](cli.md#browser-panes).

In the TUI, a click on a file path in the shell doesn't open the file. To open the file, run `laura open <path>`.

The TUI has no drawer and no start canvas. The TUI starts with the shell as the only pane, and shows the shell as a pane beside the other panes.

The TUI does not show when the agent [waits for you](workspace.md#know-when-the-agent-waits). The workspace switcher, the pane selector and the Laura server's page do not list a TUI workspace.

## Pane borders

A pane's border shows what the pane header shows in the browser:

- `#<id>`: the pane id. The focused pane's border is bright, and the others are gray.
- `+N -M`: the lines added and removed since git `HEAD`.
- `[review: N ↑a ↓b]`: `N` threads, `a` at or above the cursor and `b` below it.
- `· editor` or `· file`: the view of an editor pane, and `● unsaved` while Neovim has unsaved edits.
- `· paused`: you paused the tail pane.

To [resize two panes](navigation.md#resize-panes), drag the border between them. To give the size back to Laura, double-click the border. The TUI shows no share while you drag.

## Global keystrokes

`Ctrl+P` opens the panes popup, which finds a pane by id only. Type a pane id, then press `Enter`. A single-digit id focuses the pane at once.

In the TUI, `F12` keeps the focus on the focused pane. From a file pane, Laura sends the keystrokes to the shell.

## Typing a comment or review body

| Keystroke | What Laura does |
|---|---|
| `\`+`Enter` | Inserts a new line. |
| `←` `→` | Moves the cursor. |
| `Home` `End` | Moves the cursor to the start or the end of the line. |
| `Backspace` `Delete` | Deletes before or after the cursor. |

## Notices

The TUI shows these notices in place of the browser's banner and buttons:

- A frozen pane shows `⚠ <file> changed on disk — Shift+S submit · Ctrl+R refresh (discards unsubmitted inline review)`, shortened to `⚠ Shift+S submit · Ctrl+R discards inline review` in a narrow pane.
- A failed submit shows `⚠ inline review submission failed — Enter to retry · Esc to cancel`. `Enter` retries, and `Esc` closes the review body and keeps the threads. Laura records the cause in the journal's `review` event.

## In an editor pane

The border shows the view and the unsaved edits (see [Pane borders](#pane-borders)). `Ctrl+L` flips the view only in a focused editor pane. In the shell, `Ctrl+L` clears the screen.

### Laura's theme

The first time you start Laura with editor panes enabled, Laura asks if you want to use its theme in Neovim: `y` yes, `n` keep your Neovim as it is, `Esc` ask again next launch. Laura holds the agent's commands until you answer.

Laura's theme gives Neovim the file view's colors (Nord, over the terminal's background), line numbers and a sign column, so both views look the same. Laura applies the theme only inside Laura and never changes your config. Laura saves your answer in your [settings](cli.md#settings). To be asked again, remove `editor_theme` from the settings and restart Laura. Until you answer, editor panes in the browser use your Neovim's own theme.

## Small terminals

The TUI needs a terminal of at least 40 columns and 10 rows. In a smaller terminal, Laura shows `Make the window larger to show the workspace.` and ignores keystrokes, pastes and clicks until the terminal is large enough.

## Scrolling

In the shell, press `PageUp` or `PageDown` or use the mouse wheel to scroll back through the output. When a full-screen program runs in the shell, such as claude, vim or less, Laura passes those keystrokes to the program.

In a file pane, Laura scrolls only when the cursor reaches the top or bottom edge of the pane. The mouse wheel scrolls the pane and moves the cursor with it. `laura highlight` moves the cursor to the first highlighted line, so `c` right after a highlight comments there.

## Selection

Drag with the left mouse button to select text in a pane. Laura copies the selection to the clipboard when you release the button (OSC 52, and also `pbcopy` on macOS, because Terminal.app ignores OSC 52). From a file pane, Laura copies the source without the line-number gutter and rejoins wrapped lines.
