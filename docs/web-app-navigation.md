# Navigating the web app

This reference describes the keystrokes and clicks for navigating the web app and interacting with Laura. Press `Ctrl+H` in the web app to show the keystrokes. To start the web app, see [Web app](web-app.md).

> [!WARNING]
> The web app is [experimental](cli.md#experimental-features).

## Keystrokes and clicks

Laura uses the TUI's keystrokes in the web app. See [Navigating the TUI](navigation.md) for the full list.

- Click a pane to focus it.
- Click a line in a file pane to move the cursor to that line.
- Drag to select text. Laura copies the selection when the user releases the button. From a file pane, Laura copies the text without the line-number gutter.
- Press `Ctrl+Shift+V` to paste. Laura sends `Ctrl+V` to the shell, as a terminal does.
- Press `Ctrl+P` or click **Find a pane** in the top bar to open the pane selector. Type part of a file name or a pane id, then press `Enter` or click a row to focus that pane.
  - When the user types a pane id that only one pane matches, Laura focuses that pane at once. With panes `1` and `10`, `1` lists both panes and `10` focuses pane `10`. `0` focuses the shell.
  - Each row shows the pane's state: **New** when the pane changed while the user worked in another pane, **Frozen**, **Unsaved**, **Highlight**, and `N unsubmitted` with the authors of the pane's threads.
- Press `Ctrl+H` or click **?** in the top bar to show the keystrokes in a sheet. To close the sheet, click outside the sheet or press a keystroke.

The browser keeps some keystrokes for itself, so Laura never receives them:

- On Windows and Linux, the browser keeps `Ctrl+W`, `Ctrl+T`, `Ctrl+N` and `Ctrl+Tab`. In Neovim, run `:wincmd` in place of `Ctrl+W`.
- On macOS, the browser keeps only `Ctrl+Tab`.

## Pane header

Laura shows a pane header at the top of each pane. The pane header shows the pane's id and the file's name. Hover over the name to see the file's path. When the file has changes, the pane header shows the number of lines added and removed, such as `+3 −1`.

The pane header shows a chip for each state the pane is in:

- Laura shows **● Unsaved** when Neovim has unsaved edits in the editor pane.
- Laura shows **Frozen** when the pane is [frozen](navigation.md#in-a-focused-pane).
- Laura shows **Reloaded** for a few seconds after the file changes on disk. Laura flashes the changed lines or blocks in green. In a tail pane, Laura flashes the new lines as output arrives. When the agent shows another file in the pane, Laura shows no **Reloaded**.
- Laura shows **Diff view** when the pane shows the inline diff view.
- Laura shows **Highlight L…** with the lines that the agent highlighted. Click the chip to scroll to the highlight. Click **×** on the chip to clear the highlight, as `h` does.
- Laura shows **Submission failed** when Laura couldn't submit the inline review into the chat. See [Threads and inline reviews](#threads-and-inline-reviews).

Each button in the pane header does what its keystroke does:

| Button | Keystroke | What Laura does |
|---|---|---|
| Comment | `c` | Opens a comment on the cursor's line. |
| Comment mode | — | Turns comment mode on or off in an [HTML file](#html-files) or a [browser pane](#browser-panes), where a click on an element opens a comment on the element. Laura shows the button on an HTML page and in a browser pane in place of the comment button. |
| Diff | `d` | Shows or hides the inline diff view. Laura shows the button when the file has changes. |
| Refresh | `Ctrl+R` | Discards the unsubmitted inline review and reloads the file. |
| Close | `x` | Closes the pane. |
| **Submit review** | `Shift+S` | Opens the review body. Laura shows the button, and the number of threads, when the pane has threads. |
| **Editor** \| **File** | `Ctrl+L` | Flips an editor pane between the editor view and the file view. |

Laura shows the comment, refresh and close buttons on a file pane, and on an editor pane in the file view. Laura shows the diff button only on a file pane whose file has changes. Hover over a button to see its keystroke. The keystrokes sheet (`Ctrl+H`) says the same.

When the pane header doesn't fit, Laura shows less in the pane header, in steps, until the pane header fits:

1. Laura hides the file icon and shortens the button labels, such as **Submit** for **Submit review**.
2. Laura replaces the chips and the `+3 −1` count with a dot. Hover over the dot to see the chips' names.
3. Laura moves the comment, diff, refresh and close buttons into **More**. Click **More** to list them.
4. Laura moves **Submit review**, the thread count and the **Editor** | **File** buttons into **More**, and shortens the file's name. The pane header keeps only the pane's id, its name, the dot and **More**.
5. Laura hides the pane's id and the dot.

Laura shortens a long file name before it takes a step, but keeps at least part of the name in view.

When a pane is too narrow to read, Laura hides the pane's file and shows `This pane is too narrow to show <file>. Widen the window or close a pane.` In a very narrow pane, Laura shows only `⚠`. Hover over `⚠` to read the text.

In an editor pane's editor view, a click on **Submit review** or the thread count flips the pane to the file view first.

Laura shows an unseen marker, a dot before the file's name, when the pane reloads or the agent highlights new lines while the user works in another pane. When the user focuses the pane, Laura removes the unseen marker.

## Status bar

Laura shows a status bar on the right of the hint line. The status bar lists what waits on the user:

- Laura shows `<file> · unsubmitted inline review` for each pane with an unsubmitted inline review. Hover the item to see the inline review's state: `in progress`, `review body open`, `submitting` or `submission failed`. After a failed submit, the item's dot turns red.
- Laura shows `<file> · unsaved Neovim edits` for each editor pane with unsaved edits.
- Laura shows `Waiting for laura ready` until the agent runs `laura ready`.

Click an item to focus its pane. When nothing waits on the user, the status bar is empty. When an item doesn't fit, Laura shortens it. Hover the item to read it in full.

## Threads and inline reviews

While Laura submits the inline review, the button reads **Submitting…**. When the submit fails, Laura keeps the threads and the review body, and shows `⚠ inline review submission failed. Laura kept the threads and the review body.` The button reads **Retry submit**, and the pane header shows **Submission failed**. Click **Retry submit** or press `Enter` to try again.

Laura shows each thread right below its line, with the author of each comment. The thread names what it sits on: in a markdown file, the kind of block, such as `paragraph` or `list item`; in an HTML file, the element's start tag, such as `<button#copy>`. Click **–** on a thread or press `r` to collapse the thread, and click **Reply…** to add a comment to the thread. Laura shows the reply box at the end of the thread. On a thread that ends with your own comment, click **Edit…** to change your own last comment, or **Delete** to remove it. Laura opens the comment for editing in its place in the thread. Click the thread count in the pane header to list the pane's threads, then click a thread. Laura moves the cursor to the thread and scrolls the pane to show it. Press `Esc` or click outside the list to close it.

To edit or delete a comment, click **+** or press `c` on its line again. See [Typing a comment or review body](navigation.md#typing-a-comment-or-review-body) for the keystrokes.

When the pane is [frozen](navigation.md#in-a-focused-pane), Laura shows a banner below the pane header in place of the TUI's notice. Click **Submit review** in the banner to submit the inline review, or **Refresh** to discard the inline review and reload the file.

While the user types a comment or review body, Laura keeps the focus in the box when the user clicks a pane, so the user can select text in any pane and paste it into the comment.

While the box is open, Laura [holds the agent's commands](navigation.md#in-a-focused-pane) and the box shows `Laura holds the agent’s commands while you type.` Laura holds the commands until the user presses `Enter` or `Esc`, closes the browser tab, or opens the workspace in another browser tab.

## Markdown files

Laura shows a markdown file in a file pane as rendered markdown: headings, lists, tables, quotes and syntax-colored code.

- Press `↑` or `↓` to move the cursor one block at a time, or click a block to move the cursor to it. A block is a heading, a paragraph, a list item, a table, a quote, a code block, or another top-level part of the file such as a horizontal rule.
- In a code block, a table or a list, press `↑` or `↓` to move the cursor to each code line, table row and nested list item, or click one to move the cursor to it. When the user comments on one of these lines, Laura puts the comment on that line. Laura shows `▾` in the gutter beside each of these lines that has a thread, or `▸` when the user collapses the thread.
- Laura shows the blank lines between blocks as rows, so the user can move the cursor to a blank line and comment on the line.
- When the user comments anywhere else in a block, Laura puts the comment on the block's first line. When the cursor is on a thread below the block, Laura adds the comment to that thread.
- Laura shows a block's threads and the comment box below the block, including the threads on a code line, a table row or a list item. Press `n` or `N` to move the cursor to a thread. Press `r` on a block to collapse or expand the block's threads, or on a thread to collapse or expand only that thread. On a line with a thread, `r` collapses or expands only that line's thread. On a block or a blank line without threads, `r` collapses or expands every thread in the file, as in the TUI.
- When the agent highlights a line inside a block, Laura highlights the whole block and dims the other blocks.
- Laura shows a [`── N lines removed ──` row](agent-reference.md#show-what-changed) above the block where the lines were deleted. When the user deletes the last lines of a code block, Laura shows the row below the code block. When the code block ends the file, Laura shows the row above the code block.
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
- Click an element to move the cursor to the line of the element's start tag. Laura outlines the element at the cursor and each element with a thread, and shows the cursor's line and start tag above the element, such as `L4 <button#copy>`.
- Laura shows a pin at the top-right corner of each element with a thread, with the authors and the number of comments. Click a pin to move the cursor to its thread, and click the pin again to hide or show the thread. The page's own click handlers don't run. Laura hides the pin while the element is hidden, such as inside a closed `<dialog>`.
- In comment mode, the hint line shows `Click an element to comment · Esc leave comment mode`, except in the panes where [`Esc` goes to the program](web-app.md#comment-on-an-html-page).
- Laura shows the thread at the cursor and the comment box below the cursor's element. When the user scrolls the page or the page changes, Laura moves the thread and the pins with their elements.
- Click **–** on the thread or press `r` to hide the thread, and press `r` to show the thread again.
- When the agent highlights lines, Laura outlines the elements that start on those lines and scrolls the page to them.
- When no element starts on the cursor's line, a thread's line or the highlighted lines, Laura outlines the element that starts above them.
- When the user clicks a text field in the page, Laura lets the user type into the field. When the user clicks an element that has a `tabindex` attribute, such as a game's canvas, Laura gives the keystrokes to the page. When the user clicks text inside such an element, such as a line in a code block, Laura keeps the keystrokes. While the page has the keystrokes, the hint line shows `Keystrokes go to the page`. Press `Esc` to give the keystrokes back to Laura. When the user submits a form, the page's script handles the form, and Laura keeps showing the page. A form with `method="dialog"` closes its `<dialog>`, as in a browser.
- Drag to select text in the page, then press `Ctrl+C` to copy the text. Press `Esc` to give the keystrokes back to Laura.
- When the user clicks a link, Laura follows the link and keeps showing the page in the pane:
  - Laura opens an `http` or `https` link in a new browser tab.
  - Laura opens a link to a file by relative path in a new pane, as for a [link in a markdown file](#markdown-files).
  - Laura scrolls the page to the element that a link names by its `id`, such as `#install` or `page.html#install`. Laura opens another HTML file at its top, even when the link names an element in that file.
  - When the page has a `<base>` element, Laura follows each link from the `<base>` address, as a browser does.
- In a page, Laura doesn't move the cursor or scroll on `↑`, `↓`, `←` or `→`. Press `n` or `N` to move the cursor to a thread.
- To see the page's source as an inline diff, press `d`.
- Laura reloads the page when the file changes on disk. While the pane is frozen, Laura keeps showing the snapshot until the user refreshes the pane.

## Browser panes

A [browser pane](web-app.md#show-a-web-app-in-a-browser-pane) shows a page from a URL. The pane header shows the back and forward buttons, a globe icon, the page's title, the page's URL and the live badge. Before the page names a title, the pane header shows the URL as the name. The URL is an address field. To use the page, see [Use the page](web-app.md#use-the-page).

In the focused browser pane:

| Button | Keystroke | What Laura does |
|---|---|---|
| Back | `Alt+←` | Goes back to the previous page. |
| Forward | `Alt+→` | Goes forward to the next page. |
| — | `↑` `↓` `PageUp` `PageDown` `Space` | Scrolls the page. |
| — | `c` | Opens a comment on the element under the pointer. When the pointer is not on the page, turns on comment mode. |
| Comment mode | — | Turns comment mode on or off. In comment mode, a click on an element opens a comment on the element. |
| — | `n` `N` | Moves the cursor to the next or previous thread. |
| — | `r` | Collapses or expands the cursor's thread. |
| **Submit review** | `Shift+S` | Opens the review body. |
| **Highlight &lt;selector&gt;** | — | Scrolls the page to the element that the agent highlighted. |
| **×** on the highlight chip | `h` | Clears the highlight. |
| Refresh | `Ctrl+R` | Discards the unsubmitted inline review and the agent's highlight, and reloads the page. When Laura's browser stopped, Laura starts it again and keeps the threads. |
| Close | `x` | Closes the pane. |
| — | `Esc` | Moves the focus to the shell. |

While a text field in the page has the keystrokes, every keystroke goes to the page except `Esc`, `Ctrl+P`, `Ctrl+Q`, `Ctrl+H` and `F12`. Press `Esc` to give the keystrokes back to Laura. The field in the page loses the focus, and the scroll keys scroll the page again. While the page shows a dialog, `Enter` accepts the dialog and `Esc` cancels it.

The browser pane has no comment or diff button. To comment on a browser pane, see [Comment on a browser pane](web-app.md#comment-on-a-browser-pane).

## One workspace per run

Each `laura --web` run is its own workspace, shown in one browser tab at a time.

- When you open the URL in a second tab, the workspace moves to the second tab. In the first tab, Laura shows `The workspace is open in another tab.` and a **Use this tab** button that moves the workspace back.
- When you close the browser tab, the workspace keeps running. Open the URL again to return to the workspace.
- When the workspace ends, Laura shows `The workspace stopped. To use Laura again, start Laura in the terminal.` in the browser tab.
- When the browser tab can't connect to the workspace, Laura shows `This tab cannot connect to the workspace. Make sure that Laura did not stop and that you used the full URL from the terminal.` Check that Laura still runs in the terminal, then open the full URL that Laura printed.

## Editor panes

Laura asks about [its theme](navigation.md#lauras-theme) only in the TUI. Until you answer in the TUI, editor panes in the web app use your Neovim's own theme.
