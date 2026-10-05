# Navigating the TUI

This reference describes the keystrokes for navigating the workspace and interacting with Laura. Use `Ctrl+H` within Laura to pull up the full keystroke reference.

Laura passes keys it doesn't use to the shell, or to Neovim in an editor pane's editor view.

## Global keys

| Key | Action |
|-----|--------|
| `Ctrl+P` | Panes popup — type a pane **id** then `Enter` to focus (single-digit ids focus on keypress) |
| `Ctrl+T` | Tab nav — `←/→` browse · `n` new tab · `x` close tab · `r` rename tab. While a Neovim in the tab has unsaved edits, `x` shows a notice instead of closing it |
| `Ctrl+H` | Help |
| `Ctrl+Q` | Quit (then `y` to confirm). While any Neovim has unsaved edits, shows a notice naming its tab and pane instead: save (`:w`) or quit it first (`:qa!` discards) |
| `F12` | Lock all input to the shell, or to the focused editor pane |

## In an editor pane

> [!WARNING]
> Editor panes are [experimental](cli.md#experimental-features).

An editor pane (`laura open --edit`) supports two views, an **Editor view** and a **File view**. Use `Ctrl+L` to flip between the views.

- **Editor view** (the default): edit the file in Neovim. Every key goes to Neovim, including `Esc` and `PageUp/PageDown`, except `Ctrl+L`, `Ctrl+P`, `Ctrl+T`, `Ctrl+H`, `Ctrl+Q` and `F12`. Comments show as an inline `✎` diagnostic; use `Ctrl+W` then `d` to read the whole thread and `]d` to jump to the next one. The editor view does not support comment interactions, so flip to the file view to comment or submit.
- **File view**: comment on the saved file and submit the inline review, with the same keys as a file pane (below). Diff (`d`) and highlight (`h`) are not available.

The border shows the view (`· editor` or `· file`), `● unsaved` while Neovim has unsaved edits, and `[review: N]` when the pane has threads.

When you submit an inline review from the file view, Laura returns focus to the shell and flips the pane back to the editor view. `Ctrl+L` flips views only in a focused editor pane; in the shell it clears the screen. To use Neovim's own `Ctrl+L`, run `:noh` instead.

The file view has these limits:

- It shows the **saved file**, so save before `Ctrl+L`.
- Threads stay on the line they were made on; they don't follow your edits.
- If you save while the pane has an unsubmitted inline review, Laura freezes the pane, as with any file pane (below). Submit or refresh (`Ctrl+R`) to end the freeze.

Laura never focuses an editor pane when it opens, so you can keep typing in the chat. To leave an editor pane, press `Ctrl+P` then a pane id (`0` is the shell).

- If the agent turns the pane you're in into an editor pane, Laura opens it in the file view, so you keep using the file pane keys.
- If the agent opens another file in the editor pane you're in, Laura keeps the pane's current view.
- If the editor pane has unsaved edits, `x` in the file view shows a notice instead of closing the pane. Save (`:w`) or quit Neovim first.

### Laura's theme

The first time you start Laura with editor panes enabled, Laura asks if you want to use its theme in Neovim: `y` yes, `n` keep your Neovim as it is, `Esc` ask again next launch. Laura holds the agent's commands until you answer.

Laura applies its theme to Neovim, including the file view's syntax colors (Nord, over the terminal's background), line numbers and a sign column, so Neovim and the file view look the same when you flip with `Ctrl+L`. Laura applies the theme only inside Laura, after Neovim loads your config, and never changes your config. Laura saves your answer in `~/.laura/editor-theme`. To be asked again, delete the file and restart Laura.

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
| `Ctrl+L` | In an editor pane, flip between the editor view and the file view |
| `Esc` | Leave the pane |

An inline review is a **call and response**. Comments and threads stay in the pane until you press `Shift+S` to submit the inline review. While you type a comment or review body, Laura holds the agent's commands until you press `Enter` or `Esc`.

A file pane's border shows:

- `#<id>`, its pane id, so `Ctrl+P <id>` focuses it. The focused pane's border is bright, the others gray.
- `+N -M` after the file name: lines added (green) and removed (red) since git `HEAD`. Nothing shows when the file is clean, untracked, or `git` isn't installed. In an editor pane, both views count the saved file.
- `[review: N ↑a ↓b]` when the pane has threads: `N` threads, `a` at or above the cursor, `b` below it.

If an inline review cannot be written into the shell, Laura keeps the threads and review body and shows the notice `⚠ inline review submission failed — Enter to retry · Esc to cancel`. `Enter` retries; `Esc` closes the review body and keeps the threads. Laura records the cause in the journal's `review` event.

A file pane with an unsubmitted inline review **freezes** when its file changes on disk. A focused frozen pane shows the notice `⚠ <file> changed on disk — Shift+S submit · Ctrl+R refresh (discards unsubmitted inline review)` (shortened to `⚠ Shift+S submit · Ctrl+R discards inline review` on a narrow terminal). `Shift+S` submits against the snapshot (the inline review starts with a `⚠ file changed` banner), then the pane reloads. `Ctrl+R` refreshes. If you delete the pane's last comment, Laura also ends the freeze and reloads the pane.

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

In the shell, use `PageUp/PageDown` or the mouse wheel to scroll back through the shell's output. When a full-screen program runs in the shell (claude, vim, less), Laura passes those keys to the program, which handles its own scrolling, and Laura shows no scrollbar.

In a file pane, Laura scrolls only when the cursor reaches the top or bottom edge of the pane. Opening or collapsing a comment above the view doesn't scroll it. The mouse wheel scrolls the pane and moves the cursor with it. `laura highlight` scrolls its lines to the center once and moves the cursor to the first highlighted line, so pressing `c` right after a highlight comments there.

## Selection

Drag with the left mouse button to select text within a pane. Laura copies the selection to the system clipboard when you release the button (via OSC 52). From a file pane, Laura copies clean source, without the line-number gutter and with wrapped lines rejoined. From the shell, Laura copies the text exactly as shown.
