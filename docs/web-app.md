# Web app

Laura shows a workspace in a browser tab instead of the terminal: the shell, file panes and editor panes. The user drives them with the same keystrokes as the TUI. The agent runs in the shell and uses the `laura` CLI exactly as it does in the TUI.

> [!NOTE]
> The web app is [experimental](cli.md#experimental-features): a minimal first take on a browser view of the workspace. The pattern is still evolving.

## Start the web app

1. Set `LAURA_EXPERIMENTAL_WEB=1`.
2. Run `laura --web`, or `laura --web -- <cmd>` to host `<cmd>` instead of your shell.
3. Open the URL that Laura prints in a web browser.

Without the variable, Laura returns the error `the web app is experimental: set LAURA_EXPERIMENTAL_WEB=1 before you start Laura`.

Laura serves the web app to this computer only. While the workspace runs, Laura shows only the line `Open this URL in a web browser:` and the URL in the terminal that started it.

## Keystrokes and clicks

Laura uses the TUI's keystrokes in the web app. See [Navigating the TUI](navigation.md) for the full list.

- Click a pane to focus it.
- Click a line in a file pane to move the cursor to that line.
- Drag to select text. Laura copies the selection when the user releases the button. From a file pane, Laura copies the text without the line-number gutter.
- Press `Ctrl+Shift+V` to paste. Laura sends `Ctrl+V` to the shell, as a terminal does.

The browser keeps some keystrokes for itself, so Laura never receives them:

- On Windows and Linux, the browser keeps `Ctrl+W`, `Ctrl+T`, `Ctrl+N` and `Ctrl+Tab`. In Neovim, run `:wincmd` in place of `Ctrl+W`.
- On macOS, the browser keeps only `Ctrl+Tab`.

## Comment and submit an inline review

The user reviews a file pane in the web app as in the TUI, with buttons to comment, submit, and refresh. Laura shows the buttons on the focused file pane.

1. Move the cursor to a line, then click **+** at the end of the line or press `c`.
2. Type the comment in the box that Laura shows over the bottom of the shell, then press `Enter`.
3. When the pane has comments, click **Submit** on the pane's bottom border or press `Shift+S`.
4. Type the review body, then press `Enter` to submit the inline review into the chat.

To edit or delete a comment, click **+** or press `c` on its line again. See [Typing a comment or review body](navigation.md#typing-a-comment-or-review-body) for the keystrokes.

To discard the pane's unsubmitted inline review and reload the file, click **Refresh** or press `Ctrl+R`.

While the user types a comment or review body, Laura keeps the focus in the box when the user clicks a pane, so the user can select text in any pane and paste it into the comment.

While the box is open, Laura [holds the agent's commands](navigation.md#in-a-focused-pane) until the user presses `Enter` or `Esc`, closes the browser tab, or opens the workspace in another browser tab.

## Markdown files

Laura shows a markdown file in a file pane as rendered markdown: headings, lists, tables, quotes and syntax-colored code.

- Press `↑` or `↓` to move the cursor one block at a time, or click a block to move the cursor to it. A block is a heading, a paragraph, a list item, a table, a quote, a code block, or another top-level part of the file such as a horizontal rule.
- In a code block, a table or a list, press `↑` or `↓` to move the cursor to each code line, table row and nested list item, or click one to move the cursor to it. When the user comments on one of these lines, Laura puts the comment on that line. Laura shows `▾` in the gutter beside each of these lines that has a thread, or `▸` when the user collapses the thread.
- Laura shows the blank lines between blocks as rows, so the user can move the cursor to a blank line and comment on the line.
- When the user comments anywhere else in a block, Laura puts the comment on the block's first line. When the cursor is on a thread below the block, Laura adds the comment to that thread.
- Laura shows a block's threads below the block. Press `n` or `N` to move the cursor to a thread. Press `r` on a block to collapse or expand the block's threads, or on a thread to collapse or expand only that thread. On a line with a thread, `r` collapses or expands only that line's thread. On a block or a blank line without threads, `r` collapses or expands every thread in the file, as in the TUI.
- When the agent highlights a line inside a block, Laura highlights the whole block and dims the other blocks.
- Laura shows a [`── N lines removed ──` row](agent-reference.md#show-what-changed) above the block where the lines were deleted.
- When the user clicks a link, Laura follows the link:
  - Laura opens an `http` or `https` link in a new browser tab.
  - Laura opens a link to a file by relative path, such as `[Setup](setup.md)`, in a new pane, as `laura open` does. When a pane already shows the file, Laura focuses that pane instead. When the file doesn't exist, Laura shows the notice `⚠ <link> doesn't exist`. When the link names a folder, Laura shows the notice `⚠ <link> is a folder`. Laura ignores anything after a `?` in the link, such as `?v=2`.
  - Laura moves the cursor to the heading that a link names after `#`, such as `[Install](#install)` or `[Install](setup.md#install)`. The name after `#` can be in any case. When the markdown file has no such heading, Laura shows the notice `⚠ no heading #<heading> in <file>`. In an editor pane, Laura moves the cursor in the file view.
  - Laura shows any other link, such as a `mailto:` link, an email address or a link by absolute path, as plain text.
- Laura shows an image that the markdown file references by a relative path, such as `![Flow](img/flow.png)`:
  - Laura shows PNG, JPEG, GIF, SVG, WebP, AVIF and ICO images.
  - When the markdown file sits below the folder Laura started in, outside hidden folders, the image must sit below the folder Laura started in. Otherwise, the image must sit in the markdown file's folder or below it.
  - Below that folder, Laura doesn't show a hidden image or an image in a hidden folder. A hidden file or folder has a name that starts with `.`.
- Laura shows the alt text of an image that the markdown file references by an absolute path or a remote URL.
- To read a wide table or code block, scroll the table or code block sideways with the mouse or trackpad. In a markdown file, Laura doesn't scroll on `←` or `→`.
- Laura shows an HTML block and front matter as a code block, and other HTML as text.
- To see the changes as source lines, press `d` to show the inline diff view.
- When the user copies from a markdown file, Laura copies the rendered text without list markers or link addresses, and separates table cells with tabs.

## HTML files

Laura shows an HTML file (`.html` or `.htm`) in a file pane as the rendered page, in a sandboxed frame. Laura runs the page's scripts but keeps them from reading or controlling the workspace or the shell.

- Laura loads the page's stylesheets, scripts, images and fonts from the page's folder or below it, by relative path, outside hidden folders:
  - Laura loads CSS stylesheets and JavaScript files (`.js` and `.mjs`).
  - Laura loads the image types that [markdown files](#markdown-files) show.
  - Laura loads WOFF, WOFF2, TTF and OTF fonts.
- Laura doesn't load a file by a path that starts with `/`, such as `/style.css`.
- Laura doesn't let the page show browser dialogs, such as `alert()`, or keep data in browser storage or cookies.
- Click an element to move the cursor to the line of the element's start tag. Laura outlines the element at the cursor and each element with a thread.
- Press `c` to comment on the element; Laura shows no **+** in a page. Press `Shift+S` to submit the inline review. Laura quotes the line of the element's start tag in the inline review, as in the TUI.
- Laura shows the thread at the cursor below the page.
- When the agent highlights lines, Laura outlines the elements that start on those lines and scrolls the page to them.
- When no element starts on the cursor's line, a thread's line or the highlighted lines, Laura outlines the element that starts above them.
- When the user clicks a text field in the page, Laura lets the user type into the field. When the user clicks an element that has a `tabindex` attribute, such as a game's canvas, Laura gives the keystrokes to the page. When the user clicks text inside such an element, such as a line in a code block, Laura keeps the keystrokes. Press `Esc` to give the keystrokes back to Laura. When the user submits a form, the page's script handles the form, and Laura keeps showing the page. A form with `method="dialog"` closes its `<dialog>`, as in a browser.
- Drag to select text in the page, then press `Ctrl+C` to copy the text. Press `Esc` to give the keystrokes back to Laura.
- When the user clicks a link, Laura follows the link and keeps showing the page in the pane:
  - Laura opens an `http` or `https` link in a new browser tab.
  - Laura opens a link to a file by relative path in a new pane, as for a [link in a markdown file](#markdown-files).
  - Laura scrolls the page to the element that a link names by its `id`, such as `#install` or `page.html#install`. Laura opens another HTML file at its top, even when the link names an element in that file.
  - When the page has a `<base>` element, Laura follows each link from the `<base>` address, as a browser does.
- In a page, Laura doesn't move the cursor or scroll on `↑`, `↓`, `←` or `→`. Press `n` or `N` to move the cursor to a thread.
- To see the page's source as an inline diff, press `d`.
- Laura reloads the page when the file changes on disk. While the pane is frozen, Laura keeps showing the snapshot until the user refreshes the pane.

## Quit

Press `Ctrl+Q`, then `y`, to end the workspace, as in the [TUI](navigation.md#global-keystrokes).

Don't press `Ctrl+C` in the terminal that started Laura. Laura ends the workspace at once, and you lose Neovim's unsaved edits.

On Linux, Firefox quits on `Ctrl+Q` before Laura receives the keystroke. Use a different browser instead.

## One workspace per run

Each `laura --web` run is its own workspace, shown in one browser tab at a time.

- When you open the URL in a second tab, the workspace moves to the second tab. In the first tab, Laura shows `The workspace is open in another tab.` and a **Use this tab** button that moves the workspace back.
- When you close the browser tab, the workspace keeps running. Open the URL again to return to the workspace.
- When the workspace ends, Laura shows `The workspace stopped. To use Laura again, start Laura in the terminal.` in the browser tab.
- When the browser tab can't connect to the workspace, Laura shows `This tab cannot connect to the workspace. Make sure that Laura did not stop and that you used the full URL from the terminal.` Check that Laura still runs in the terminal, then open the full URL that Laura printed.

## Editor panes

Laura asks about [its theme](navigation.md#lauras-theme) only in the TUI. Until you answer in the TUI, editor panes in the web app use your Neovim's own theme.
