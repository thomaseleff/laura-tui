# Navigating the workspace

This reference lists the keystrokes and clicks in the workspace. Press `Ctrl+H` or click **?** in the top bar to show the keystrokes in Laura. To start Laura, see [Use the workspace](workspace.md).

> [!NOTE]
> In the terminal (`laura --tui`), Laura uses the same keystrokes. For what is different, see [Navigating the TUI](tui-navigation.md).

Laura passes the keystrokes that it doesn't use to the shell, or to Neovim in an editor pane's editor view.

## Global keystrokes

| Keystroke | What Laura does |
|---|---|
| `Ctrl+P` | Opens the pane selector. Type part of a file name or a pane id, then press `Enter`. A pane id that only one pane matches focuses that pane at once, and `0` focuses the shell. The pane selector also finds a workspace: see [Switch workspaces](#switch-workspaces). |
| `Ctrl+H` | Shows the keystrokes. |
| `Ctrl+Q` | Quits, after you press `y`. While Neovim has unsaved edits, Laura shows a notice that names the pane instead. |
| `F12` | Sends every keystroke to the shell, or to the focused editor pane, until you press `F12` again. |
| `Ctrl+Shift+V` | Pastes. Laura passes `Ctrl+V` itself to the shell, as a terminal does. |

- Click a pane to focus it, and click a line in a file pane to move the cursor to it.
- Drag to select text. Laura copies the selection when you release the button, without the line-number gutter.
- Each row of the pane selector shows the pane's state, such as **New**, **Frozen**, **Unsaved** or `N unsubmitted`.

The browser keeps some keystrokes, so Laura never receives them. On Windows and Linux, the browser keeps `Ctrl+W`, `Ctrl+T`, `Ctrl+N` and `Ctrl+Tab`. On macOS, the browser keeps only `Ctrl+Tab`.

`Ctrl+W` closes the browser tab. The workspace keeps running. To go back to the workspace, open the URL again.

When the shell exits while Neovim has unsaved edits, Laura stays open:

1. Laura focuses the first editor pane with unsaved edits and shows the notice `⚠ To close Laura after the shell stopped, save or quit Neovim.`
2. When you save or quit that Neovim, Laura does the same for the next editor pane with unsaved edits.
3. Laura closes when no Neovim has unsaved edits.

## Switch workspaces

The workspace switcher is at the left of the top bar. The workspace switcher shows the session label and the folder of the workspace. Before the agent sets a session label, the workspace switcher shows the folder name in grey.

To switch to another workspace:

1. Click the workspace switcher. Laura lists the running workspaces, oldest first, and marks the workspace that you are in with ✓. Laura selects the first other workspace.
2. Press `Enter` to switch to the selected workspace, or press `↑` or `↓` to select a different workspace first. You can also click the workspace.

Laura loads the other workspace in the same browser tab. The workspace that you leave keeps running.

- To open a workspace in a new browser tab, click **↗** on its row or press `Ctrl+Enter`. `Ctrl`+click, `Cmd`+click and a middle click also open a new browser tab.
- To close the list, press `Esc` or click outside the list.
- While a comment box is open, the browser asks before it leaves the workspace, so you do not lose the comment.

The pane selector finds a workspace too. Press `Ctrl+P` and type part of a session label or a folder. Laura lists the panes under **Panes** and the other workspaces under **Workspaces**. Press `Enter` to switch to the selected workspace, or `Ctrl+Enter` to open the workspace in a new browser tab.

What Laura shows depends on [the Laura server](workspace.md#the-laura-server):

| The Laura server | The workspace switcher | The pane selector |
|---|---|---|
| Runs | The list ends with **New workspace…**, which opens the Laura server's page. | **Workspaces** ends with **New workspace…**. |
| Does not run | The list ends with the line `Open Laura from your desktop or app menu, or run laura --serve, to start more workspaces.` With only one workspace, the workspace switcher shows no ▾ and no list. | **Workspaces** lists only the other workspaces. |

While the Laura server runs, select the Laura logo at the left of the top bar to open the Laura server's page. `Ctrl`+click, `Cmd`+click and a middle click open the page in a new browser tab.

## In a focused pane

| Keystroke | Button | What Laura does |
|---|---|---|
| `↑` `↓` | — | Moves the cursor. |
| `n` `N` | — | Moves the cursor to the next or previous thread. |
| `←` `→` | — | Scrolls code and diff lines sideways. |
| `c` | **+** | Comments on the cursor's line: starts a thread, replies, or edits your last comment. |
| `r` | **–** | Collapses or expands the cursor's thread, or every thread when the cursor is on no thread. |
| `Shift+S` | **Submit review** | Opens the review body, then submits the inline review into the chat. |
| `Ctrl+R` | Refresh | Discards the unsubmitted inline review and reloads the file. |
| `d` | Diff | Shows or hides the inline diff view against git `HEAD`. |
| `h` | **×** on the highlight chip | Clears the agent's highlight. |
| `p` | Pause \| Resume | Pauses or resumes a tail pane, or a file pane that the agent opened with `--follow`. |
| `x` | Close | Closes the pane. |
| `Esc` | — | Leaves the pane. |

An inline review is a **call and response**. Your comments and the agent's comments stay in the pane until you submit the inline review.

### Threads and inline reviews

- Laura shows each thread below its line. Click **Reply…** to add a comment. On a thread that ends with your comment, click **Edit…** or **Delete**.
- Click the thread count in the pane header to list the pane's threads, then click a thread to move the cursor to it.
- While you type a comment or a review body, Laura holds the agent's commands until you press `Enter` or `Esc`, close the browser tab, or open the workspace in another tab.
- When a submit fails, Laura keeps the threads and the review body. Click **Retry submit** or press `Enter` to try again.

A file pane with an unsubmitted inline review **freezes** when its file changes on disk. Laura keeps showing the snapshot and shows a banner below the pane header:

- Click **Submit review** or press `Shift+S` to submit against the snapshot. The inline review starts with a `⚠ file changed` banner, and then the pane reloads.
- Click **Refresh** or press `Ctrl+R` to discard the inline review and reload the file.
- Delete the pane's last comment, and Laura also ends the freeze.

A tail pane that you **pause** with `p` doesn't reload, so you can read and comment while the program writes more output. Laura loses no output. Press `p` again to resume. When the pane has threads, the pane freezes instead of reloading.

## Typing a comment or review body

| Keystroke | What Laura does |
|---|---|
| `Enter` | Adds the comment, saves the edit, or submits the review body. |
| `Shift+Enter` | Inserts a new line. |
| `Esc` | Cancels. |

To delete your comment, press `c` on its line, clear the text, and press `Enter`. While the box is open, you can click another pane to select text and paste the text into the comment.

## Pane header

The pane header shows the pane's id, the file's name and, when the file has changes, the lines added and removed, such as `+3 −1`. A dot before the name is the unseen marker: the pane changed while you worked in another pane.

| Chip | What it means |
|---|---|
| **● Unsaved** | Neovim has unsaved edits in the editor pane. |
| **Paused** | You paused the tail pane. |
| **Frozen** | The file changed on disk while the pane has an unsubmitted inline review. |
| **Reloaded** | The file changed on disk a few seconds ago. Laura flashes the changed lines in green. |
| **Diff view** | The pane shows the inline diff view. |
| **Highlight L…** | The agent highlighted these lines. Click the chip to scroll to them. |
| **Submission failed** | Laura did not submit the inline review into the chat. |
| **Agent**, **Screenshot**, **● Recording**, `393×852` | In a browser pane, the agent's browser tool clicks or types, takes a screenshot, records the page, or emulates a device. See [Work on a page with the agent](workspace.md#work-on-a-page-with-the-agent). |

Hover over a button to see its keystroke. When the pane header doesn't fit, Laura shortens the labels, replaces the chips with a dot, and moves the buttons into **More**.

When a pane is narrower than 160 pixels or shorter than 100 pixels, Laura shows `This pane is too small to show <name>. Make the window larger or close a pane.` To close the pane, click the pane and press `x`.

## Status bar

The status bar on the right of the hint line lists what waits on you. Click an item to focus its pane.

- `<file> · unsubmitted inline review`: the pane has an unsubmitted inline review. After a failed submit, the item's dot turns red.
- `<file> · unsaved Neovim edits`: the editor pane has unsaved edits.
- `Waiting for laura ready`: the agent has not run `laura ready`, so you can't comment yet.

## Notices

Laura shows a notice in the bottom-right corner of its pane, or of the workspace when the notice is for no pane.

| Notice | What to do |
|---|---|
| `` ⚠ The agent must run `laura ready` before you can comment. `` | Ask the agent to run `laura ready`. |
| `⚠ Add a comment before you submit the inline review.` | Comment on a line first. |
| `⚠ The shell stopped, so Laura cannot submit the inline review.` | Start Laura again. |
| `⚠ Press d to close the diff view before you comment.` | Press `d`, then comment. |
| `⚠ An editor pane does not show the diff view.` | Ask the agent to show the diff in a file pane. |
| `⚠ Laura did not submit the inline review, but kept its threads and review body.` | Click **Retry submit** or press `Enter`. |

## Markdown files

Laura shows a markdown file as rendered markdown.

- Press `↑` or `↓` to move the cursor one block at a time: a heading, a paragraph, a list item, a table, a quote or a code block. In a code block, a table or a list, the cursor moves one code line, table row or nested list item at a time.
- When you comment on one of those lines, Laura puts the comment on that line. Anywhere else in a block, Laura puts the comment on the block's first line.
- When the agent highlights a line in a block, Laura highlights the whole block.
- Scroll a wide table or code block sideways with the mouse or trackpad.
- Press `d` to see the changes as source lines.
- Click a link to a file by relative path, such as `[Setup](setup.md#install)`, and Laura opens the file in a new pane, or focuses the pane that shows it. Laura opens an `http` or `https` link in a new browser tab.
- Laura shows an image that the file references by relative path, such as `![Flow](img/flow.png)`, when the image is below the folder Laura started in, outside hidden folders. For an image by an absolute path or a remote URL, Laura shows only the alt text.

When a link's file or heading doesn't exist, Laura shows `⚠ The link target <link> does not exist.` or `⚠ The file <file> has no heading #<heading>.`

## HTML files

Laura shows an HTML file as the rendered page, with its scripts running in a sandbox that keeps them from the workspace and the shell.

- Laura loads the page's stylesheets, scripts, images and fonts by relative path from the page's folder or below it, outside hidden folders. Laura doesn't load a path that starts with `/`, such as `/style.css`.
- The page can't show `alert()` dialogs or keep data in browser storage or cookies.
- Click an element to move the cursor to the line of its start tag. Laura shows a pin on each element with a thread. Click the pin to move the cursor to the thread.
- Click a text field, or an element with a `tabindex` attribute such as a game's canvas, to give the page the keystrokes. The hint line shows `Keystrokes go to the page`. Press `Esc` to give the keystrokes back to Laura.
- In a page, `↑`, `↓`, `←` and `→` go to the page. Press `n` or `N` to move to a thread.
- To comment with a click, see [Comment on an HTML page](workspace.md#comment-on-an-html-page).

## Browser panes

A [browser pane](workspace.md#show-a-web-app-in-a-browser-pane) shows a page from a URL. The pane header shows the back and forward buttons, the page's title, the URL in an address field, and the live badge.

| Keystroke | Button | What Laura does |
|---|---|---|
| `Alt+←` `Alt+→` | Back, Forward | Goes to the previous or next page. |
| `↑` `↓` `PageUp` `PageDown` `Space` | — | Scrolls the page. |
| `c` | Comment mode | Comments on the element under the pointer. When the pointer is not on the page, turns on comment mode. |
| `n` `N`, `r`, `Shift+S`, `h`, `x` | — | Work as in a file pane. |
| `Ctrl+R` | Refresh | Discards the unsubmitted inline review and the highlight, and reloads the page. Starts Laura's browser again when it stopped. |
| `Esc` | — | Gives the keystrokes back to Laura, or else moves the focus to the shell. |
| `Enter` `Esc` | OK, Cancel | Answers the page's dialog. |

While a field in the page has the keystrokes, every keystroke goes to the page except `Esc`, `Ctrl+P`, `Ctrl+Q`, `Ctrl+H` and `F12`. To use the page, see [Use the page](workspace.md#use-the-page).

## In an editor pane

> [!NOTE]
> Editor panes are [experimental](cli.md#experimental-features).

An editor pane has two views. Click **Editor** | **File** in the pane header or press `Ctrl+L` to flip between them.

- **Editor view** (the default): you edit the file in Neovim. Every keystroke goes to Neovim, except `Ctrl+L`, `Ctrl+P`, `Ctrl+H`, `Ctrl+Q` and `F12`.
  - Laura shows each comment as a `✎` diagnostic. Press `]d` or `[d` to go to the next or previous thread and read it in full.
  - Press `Ctrl+W` then `d` to show the thread under the cursor. On Windows and Linux, the browser keeps `Ctrl+W`, so run `:wincmd d` instead.
- **File view**: you comment on the saved file and submit the inline review, as in a file pane. Save (`:w`) before you flip. Threads stay on their lines and don't follow your edits. Diff (`d`) and highlight (`h`) don't work.

When you submit from the file view, Laura flips the pane back to the editor view. Laura never focuses an editor pane when it opens. To leave the pane, press `Ctrl+P` then a pane id. Laura asks about [its theme](tui-navigation.md#lauras-theme) only in the TUI.

## One workspace per run

Each `laura` run is one workspace, shown in one browser tab at a time.

- When you open the URL in a second tab, Laura moves the workspace there. The first tab shows `The workspace is open in another tab.` and a **Use this tab** button.
- When you close the browser tab, the workspace keeps running. Open the URL again to return.
- When the workspace ends, the tab shows `The workspace stopped. To use Laura again, start Laura in the terminal.`
- While [the Laura server](workspace.md#the-laura-server) runs, the message for a moved workspace has a **Show the workspaces** button, which opens the Laura server's page. When the workspace ends, Laura opens the Laura server's page in the tab.
- When the tab can't connect, Laura shows `This tab cannot connect to the workspace. Make sure that Laura did not stop and that you used the full URL from the terminal.` Check that Laura still runs, then open the full URL that Laura printed.
