# Navigating the TUI

The following reference provides the keystrokes to navigate the Laura TUI workspace. Press `Ctrl+H` anytime from within Laura to pull up the same reference.

Keys Laura doesn't bind pass through to the shell, or to Neovim in an editor pane's editor view.

## Global keys

| Key | Action |
|-----|--------|
| `Ctrl+P` | Panes popup — type a pane **id** then `Enter` to focus (single-digit ids focus on keypress) |
| `Ctrl+T` | Tab nav — `←/→` browse · `n` new tab · `x` close tab · `r` rename tab. `x` is refused, with a notice, while a Neovim in the tab has unsaved edits |
| `Ctrl+H` | Help |
| `Ctrl+Q` | Quit (then `y` to confirm). Refused, with a notice naming the tab and pane, while any Neovim has unsaved edits: save (`:w`) or quit it first (`:qa!` discards) |
| `F12` | Lock all input to the shell, or to the focused editor pane |

## In an editor pane

An editor pane (experimental, `laura open --edit`) has two views, and `Ctrl+L` flips between them. Neovim keeps running while the file view shows.

- **editor view** (the default): Neovim takes keys like the shell. Only `Ctrl+L`, `Ctrl+P`, `Ctrl+T`, `Ctrl+H` and `Ctrl+Q` stay Laura's. Everything else goes to Neovim, including `Esc` and `PageUp/PageDown`. Its border shows `#<id> <path> · editor`, plus `● unsaved` while Neovim has unsaved edits and `[review: N]` when it has threads.
- **File view**: the pane works like a file pane, with the keys below: comment, reply, and `Shift+S` to submit the inline review. It has no `d` or `h`: an editor pane shows no diff or highlight. Its border shows `#<id> <path> · file`, with `● unsaved` too while Neovim has unsaved edits.

The editor view shows the pane's threads read-only, as Neovim diagnostics: a `✎` sign on the line, the newest comment at the end of it, and the whole thread with `<C-w>d` (or `]d` to jump between them). They follow your edits in Neovim; the file view shows them on the saved file's lines. To reply or comment, press `Ctrl+L`.

Submitting from the file view returns focus to the shell and flips the pane back to the editor view, so you come back to Neovim to act on the agent's reply. `Ctrl+L` is Laura's only while an editor pane is focused; everywhere else it goes where it always did (the shell's clear screen). For Neovim's own `Ctrl+L`, use `:noh`.

The file view is a trial, with these limits:

- It shows the **saved file**. Unsaved Neovim edits aren't in it, so save before `Ctrl+L`.
- Threads stay on the line they were made on; they don't follow your edits.
- Saving while the pane has an unsubmitted inline review freezes it, as in any file pane (below). Submit or `Ctrl+R` ends the freeze.

An editor pane never takes focus when it opens, so typing into the chat can't land in Neovim. If the agent turns the pane you're in into an editor pane, it opens in the file view, so your next keys still act on the file pane. An editor pane you're in keeps its view when the agent opens another file in it. To leave the pane, press `Ctrl+P` then a pane id (`0` is the shell). When Neovim exits, the pane closes, or stays as a file pane if it has threads. `x` in the file view of an editor pane with unsaved edits shows a notice instead of closing it; save (`:w`) or quit Neovim first.

### Laura's theme

The first time Laura starts with editor panes on, it asks whether Neovim should use Laura's theme inside Laura: `y` yes, `n` keep your Neovim as it is, `Esc` ask again next launch. Until you answer, the agent's commands wait. `y` or `n` shows `✅ Experimental · Editor panes setup complete`. Laura's theme gives Neovim the file view's syntax colors (Nord, over the terminal's own background) and turns on line numbers and an always-on sign column, so a `Ctrl+L` flip neither swaps palettes nor shifts the code. It applies after your Neovim config loads, and only inside Laura: your config is never changed. The answer is kept in `~/.laura/editor-theme`, and the wizard shows the full path; delete that file and restart Laura to be asked again.

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

An inline review is a **call and response**. Comments and threads remain in the pane until you press `Shift+S` to submit the inline review. Every file pane's border starts with its pane id, `#<id>`, so `Ctrl+P <id>` focuses it; the focused pane's border is bright, the others gray. The border shows its file's lines changed since git `HEAD` after the file name, `+N` added in green and `-M` removed in red; nothing when the file is clean, untracked or there's no `git`. In an editor pane, they count the saved file in both views. A pane with threads shows `[review: N ↑a ↓b]` on its border: `N` threads, `a` at or above the cursor, `b` below it. Use `n`/`Shift+N` to jump to the next / previous thread. If the inline review can't be written into the shell, the notice `⚠ inline review submission failed — Enter to retry · Esc to cancel` appears and the threads and review body are kept. `Enter` retries; `Esc` closes the review body and keeps the threads. The cause is recorded in the journal's `review` event. While you type a comment or review body, the agent's commands wait until you press `Enter` or `Esc`.

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
