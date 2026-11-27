# Navigating the workspace

This reference lists the keystrokes and clicks in the workspace. Press `Ctrl+H` or click **?** in the top bar to show the keystrokes in Laura. To start Laura, see [Use the workspace](workspace.md).

> [!NOTE]
> In the terminal (`laura --tui`), Laura uses the same keystrokes. For what is different, see [Navigating the TUI](tui-navigation.md).

Laura passes the keystrokes that it doesn't use to the shell, or to Neovim in an editor pane's editor view.

## Global keystrokes

| Keystroke | What Laura does |
|---|---|
| `Ctrl+P` | Opens the pane selector. Type part of a file name or a pane id, then press `Enter`. A pane id that only one pane matches focuses that pane at once, and `0` focuses the shell. The pane selector also finds a workspace: see [Switch workspaces](#switch-workspaces). |
| ``Ctrl+` `` | Opens or closes the [drawer](#the-drawer). |
| `Ctrl+.` | Flips the [drawer](#the-drawer) between the History and the shell. From another pane, Laura opens the drawer as it was. |
| `Ctrl+Shift+↑` / `Ctrl+Shift+↓` | Makes the [drawer](#the-drawer) taller or shorter by 5% of the workspace while the drawer is open. |
| `Ctrl+H` | Shows the keystrokes. |
| `Ctrl+Q` | Quits, after you press `y`. While Neovim has unsaved edits, Laura shows a notice that names the pane instead. |
| `F12` | Sends every keystroke to the shell, or to the focused editor pane, until you press `F12` again. While the drawer is in use, Laura shows the drawer, with the shell, until you press `F12` again. In an editor pane, Laura keeps the drawer closed, and `F12` sends the keystrokes to Neovim. From a file pane or a browser pane, Laura moves the focus to the shell, and back to that pane when you press `F12` again, unless you or the agent moved the focus to another pane while the lock was on. Under the pane selector, the help, the quit question or a draft, Laura keeps the focus on the pane. Laura also closes the workspace switcher's list. |
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

## The drawer

Laura shows the History and the shell in the drawer. The drawer floats above the bottom line, over the panes, while the shell has focus. File panes, browser panes and editor panes fill the workspace.

When the web app starts, no pane is open yet. Laura shows a start canvas above the drawer:

- Until you submit your first inline review, Laura shows **Welcome to Laura**, with three steps: ask to see something, comment on a line, and submit your review. The help (`Ctrl+H`) shows the same steps.
- After your first inline review, in any folder, Laura shows the project at a glance: up to three cards, side by side, about the folder that Laura started in. Laura leaves out a card that has nothing to show. While Laura reads the folder, Laura shows three gray placeholder cards.
  - **README.md** shows the folder's `README.md`, without its HTML. Click **Open as a pane** to open the README in a file pane. Click a link in the README to open its file at the heading.
  - **Changes on `<branch>`** lists the files that changed since the last commit, with the lines added and removed, and `?` for a new file that git doesn't track yet. Laura counts only the changes in the folder that Laura started in. When nothing changed in that folder, the card is **Latest on `<branch>`**, marked **No changes in this folder**: the subject, hash, author and age of the last commit that changed that folder, and its files in that folder. Laura lists up to 200 files, then the count of the others.
  - **Recently in this folder** lists your last three sessions in the folder, each with its session label, its age, its inline reviews and the files the agent opened.

  Scroll a card to read the rest of it. Click a file to open it in a file pane. Laura opens a changed file in the diff view. When the folder has no README, no git and no sessions, Laura shows one sentence, `Ask your agent to show you something`, and a `?` button that opens the help.

While the start canvas shows, the drawer stays open, and ``Ctrl+` `` and the shell button do nothing. The agent's first pane takes the place of the start canvas, and the drawer closes when that pane takes the focus. When you close the last pane, Laura shows the start canvas again.

Over the start canvas, the drawer shows at least 18 rows of the shell, so the agent's start screen fits, and at most 54% of the workspace. When you set a height, Laura uses your height. After the first pane opens, the drawer goes back to your height, or to 54%. When you change the drawer's height over the start canvas, the start canvas stays where it is, and the drawer covers more or less of it. Laura makes the project at a glance as tall as the room above the drawer at its least height. The drawer covers the lower part of the cards until you drag the drawer's grip down. On a narrow window, Laura stacks the cards, and you scroll the start canvas to see all of them. When you show the shell as a pane, Laura stacks the cards beside the shell, as tall as the shell. The README gets half of that height, and the other two cards share the rest.

The shell button in the middle of the bottom line shows the shell's name. While the drawer is closed, the shell button also shows the agent's latest `laura say`, until you write to the shell. After you write to the shell, the shell button shows only the shell's name until the agent's next say. The dot on the shell button pulses while the shell prints.

A long say opens as a card that grows upward from the bottom line, over the panes:

- When you click or type in a pane, Laura folds the say to one line, cut with `…`.
- To read the say in full again, point at the shell button for a moment, or move the keyboard focus to the shell button. Laura folds the say again when the pointer leaves the shell button, or when you press `Esc` on the shell button.
- Each new say opens in full again. A screen reader reads each new say. While the agent [waits for you](workspace.md#know-when-the-agent-waits) and said nothing since you last wrote to the shell, the shell button is amber and shows `Waiting for you`. While the agent waits, the browser tab's icon shows an amber dot.

To open the drawer, do one of these:

- Press ``Ctrl+` `` or click the shell button.
- Press `Esc` in a pane.
- Submit an inline review. Laura opens the drawer, so you see the agent read the inline review.

To close the drawer, press ``Ctrl+` `` or click the shell button again. Laura focuses the pane that you used last. A click on a pane also closes the drawer and focuses that pane.

To change the drawer's height, drag the grip on the drawer's top edge, or press `Ctrl+Shift+↑` or `Ctrl+Shift+↓`. While you drag, Laura shows the height, such as `Drawer · 48% of the height`. The height can be from 20% to 85% of the workspace, and Laura always shows at least 5 rows of the shell. Laura saves the height in your [settings](cli.md#settings) and uses the height for each workspace that you start later. To go back to the default height, double-click the grip.

A screen reader names the grip **Drawer height**. When the grip has the focus, `↑` and `↓` change the height, and `Home` goes back to the default height.

To show the shell as a pane, click **Show the shell as a pane** in the drawer's pane header. Laura puts the shell on the left, at 40% of the width. The shell stays a pane when you open and close panes. Over the start canvas, the start canvas takes the right of the workspace, and the agent's next pane takes the place of the start canvas. To put the shell back in the drawer, click **Show the shell in a drawer** in the shell's pane header.

When a pane can't fit beside the shell, Laura shows the notice `⚠ The shell doesn't fit as a pane. Close a pane or make the window larger.` and keeps the shell in the drawer.

### The History and the shell

The drawer shows the History or the shell. The drawer shows the shell until you flip it, and keeps the view that you left it on. A say or a wait doesn't flip the drawer. To flip the drawer, press `Ctrl+.`, or click **History** or **Shell** in the drawer's pane header. When you show the shell as a pane, the pane flips the same way.

The History lists what the agent did through Laura in its session, one row for each event, with its time:

- `ready`, with the session label, when the agent starts the session. The History keeps the rows of the commands that the agent ran before its first `ready`. When the agent then starts another session with `laura ready --session`, Laura starts an empty History.
- One row for each `laura` command that the agent ran: `open`, `close`, `focus`, `highlight`, `diff` and `comment`, with the file and its lines or its element. `laura open --highlight` gives an `open` row and a `highlight` row. A command that returned an error, `laura layout` and `laura feedback` give no row.
- `say`, with each text that the agent sends with [`laura say`](cli.md#commands). Laura shows the text as markdown. Click a file path in the text, such as `src/profile.ts:41`, to open the file, as in the shell. Click a link to a file, such as `[the CLI](docs/cli.md#commands)`, to open the file at the link's heading. Laura reads the link's path from the folder Laura started in.
- `review`, with the file and its count of comments, for each inline review that you submit.

A blue row with a chevron shows its command again when you click the row. Laura never runs the command again, and doesn't add a row for the click. Laura finds the pane by the row's file, not by its pane number, except for an `open` row of a page (below), and that pane takes the focus, which closes the drawer. The drawer keeps the History, so ``Ctrl+` `` opens the History again.

To show a command again from the keyboard, press `Tab` in the History until a blue row has the focus, then press `Enter` or `Space`. Laura does the same as for a click.

- An `open` row focuses the pane that shows the file or the page, or opens the file or the page again. Laura opens an editor pane's file in a file pane. When the browser pane of an `open` row moved to another page of the same site, Laura focuses that browser pane.
- A `highlight` row highlights the lines or the element again.
- A `diff` row that turned the diff view on turns the diff view on again.
- A `comment` row moves the cursor to the comment's line, or highlights the comment's element in a browser pane. Laura adds no comment.
- A `focus` row focuses the pane again.

A `ready`, `close`, `say` or `review` row, the `open` row of `laura tail`, a `highlight` row that cleared the highlight, a `diff` row that turned the diff view off and a `focus` row on the shell show nothing again.

When Laura can't show a command again, Laura changes nothing and shows one of these notices in the drawer:

- `⚠ No pane shows <name>. Click its open row to open it again.` when no pane shows the row's file.
- `⚠ <path> no longer exists.` when an `open` row's file is gone.
- `⚠ No element matches <selector> in pane #<n>.` when the element is gone from the page.
- `⚠ The page in pane #<n> isn't ready. Try again once it has loaded.` while the page loads or shows a dialog.
- `⚠ Pane #<n> is frozen. Submit or refresh its inline review first.` when the pane is frozen.
- `⚠ Pane #<n> is an editor pane, which shows no highlight or diff.` when an editor pane shows the file of a `highlight` row of lines or a `diff` row.
- `⚠ To start Laura's browser again, press Ctrl+R in a browser pane.` when Laura's browser exited.

To open a file that the agent names in the chat, click the file's path in the shell, such as `src/profile.ts:41`. Laura opens the file in a pane, or focuses the pane that shows the file, and puts the cursor on line 41, in the middle of the pane. When line 41 is already on screen, Laura moves only the cursor. In an editor pane, Laura puts the cursor of the file view on line 41. The pane takes the focus, so Laura closes the drawer.

- Laura underlines a path when you point at it. A path needs a `/` or a `\` and an extension, such as `src/profile.ts`, `src\profile.ts`, `./notes.md` or `app/[slug]/page.tsx`.
- Laura reads a relative path from the folder Laura started in.
- Laura doesn't underline a URL, a path that starts with `~/`, or a path that continues on the next row.

When the file doesn't exist, or the path names a folder, Laura shows `⚠ The link target <path> does not exist.` or `⚠ The link target <path> is a folder.` in the drawer.

When the workspace has no room for another pane, Laura shows `⚠ No room for another pane. Close a pane, then click the link again.` in the drawer.

## Resize panes

To make a pane larger, drag the line between that pane and the pane beside it. While you drag, Laura shows each side's share of the space, such as `Pane 1 · 60%   Panes 2, 3 · 40%`. Laura stops the line where a pane would get too small. While you drag, the shell and editor panes keep their size. Laura fits them when you stop.

When the window gets smaller, a pane can become too small. You can still drag a line to make that pane larger, but Laura doesn't make that pane smaller.

Laura keeps the size that you set. When the agent opens another pane, the line stays where you put it.

To give the size back to Laura, double-click the line. Laura puts the line back where Laura had it. When you point at a line between side-by-side panes, Laura shows `Drag or press ← → to resize · double-click or Home to give the size back to Laura`, and `↑ ↓` for a line between stacked panes.

A screen reader names the line **Size of Pane 1 and Pane 2**, with the names of the panes on each side. When the line has the focus, `←` and `→` move a line between side-by-side panes, and `↑` and `↓` move a line between stacked panes, 5% at a time. `Home` gives the size back to Laura.

Laura shows no line between the shell and the start canvas. When you show the shell as a pane again, Laura keeps the size that you set for the shell's line, if the other panes still fit beside the shell. If they don't fit, Laura uses its own size.

## Switch workspaces

The workspace switcher is at the left of the top bar. The workspace switcher shows the session label and the folder of the workspace. Before the agent sets a session label, the workspace switcher shows the folder name in grey.

The browser tab shows `Laura · <session label>`, or `Laura · <folder name>` before the agent sets a session label.

To switch to another workspace:

1. Click the workspace switcher. Laura lists the workspace that you are in first, marked with ✓, then the other running workspaces, the workspace that you visited last first. Laura selects the first other workspace.
2. Press `Enter` to switch to the selected workspace, or press `↑` or `↓` to select a different workspace first. You can also click the workspace.

Laura loads the other workspace in the same browser tab. The workspace that you leave keeps running.

You visit a workspace when you show its browser tab or switch to the workspace. Laura sorts a workspace that you have not visited yet by its start, the newest first.

The list shows each workspace, and the pane selector shows the other workspaces. The dot before each workspace shows its state:

- Amber: the agent in that workspace [waits for you](workspace.md#know-when-the-agent-waits). The row also shows `Waiting for you`.
- Blue: the workspace that you are in. The dot stays blue while the agent in this workspace waits for you. To see where this workspace shows its wait, see [Know when the agent waits](workspace.md#know-when-the-agent-waits).
- Grey: any other workspace.

While another workspace waits for you, the workspace switcher shows an amber dot before ▾.

- To open a workspace in a new browser tab, click **↗** on its row or press `Ctrl+Enter`. `Ctrl`+click, `Cmd`+click and a middle click also open a new browser tab.
- To close the list, press `Esc` or click outside the list.
- While a comment box is open, the browser asks before it leaves the workspace, so you do not lose the comment.

The pane selector finds a workspace too. Press `Ctrl+P` and type part of a session label or a folder. Laura lists the panes under **Panes** and the other workspaces under **Workspaces**, in the order of the workspace switcher. Press `Enter` to switch to the selected workspace, or `Ctrl+Enter` to open the workspace in a new browser tab.

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
| `Esc` | — | Focuses the shell. While the drawer is in use, Laura opens the drawer. |

Before the agent runs `laura ready`, the hint line shows `c` and `Shift+S` dimmed. Hover over a dimmed keystroke to see ``Available after the agent runs `laura ready` ``.

An inline review is a **call and response**. Your comments and the agent's comments stay in the pane until you submit the inline review.

### Changed lines

Laura tints each line changed since git `HEAD`:

- A **green** row is an added line.
- A **blue** row is a modified line.
- A **red** `── N lines removed ──` row shows where lines were deleted.

Laura draws the cursor and the agent's highlight over the tint. In the web app, Laura tints a markdown block blue when the block has a modified line.

In the diff view (`d`), Laura uses the same tints: a `+` row is green, also for a modified line, and a `-` row is red.

### Threads and inline reviews

- Laura shows each thread below its line. Click **Reply…** to add a comment. On a thread that ends with your comment, click **Edit…** or **Delete**.
- Laura shows a collapsed thread as one row below its line, with the line number, the authors of the comments and the number of comments. Click the row to expand the thread.
- On an HTML page or in a browser pane, Laura hides a collapsed thread.
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
| `⚠ A pane can't get any smaller.` | Make the window larger, or close a pane, then [resize the panes](#resize-panes) again. |
| `⚠ Laura's size doesn't fit in this window. Make the window larger, then double-click the line again.` | Make the window larger, then double-click the line again. |
| `⚠ A pane closed while you moved the line.` | [Resize the panes](#resize-panes) again. |

## Markdown files

Laura shows a markdown file as rendered markdown.

- Press `↑` or `↓` to move the cursor one block at a time: a heading, a paragraph, a list item, a table, a quote or a code block. In a code block, a table or a list, the cursor moves one code line, table row or nested list item at a time.
- When you comment on one of those lines, Laura puts the comment on that line. Anywhere else in a block, Laura puts the comment on the block's first line.
- When the agent highlights a line in a block, Laura highlights the whole block.
- Scroll a wide table or code block sideways with the mouse or trackpad.
- Press `d` to see the changes as source lines.
- Click a link to a file by relative path, such as `[Setup](setup.md#install)`, and Laura opens the file in a new pane, or focuses the pane that shows it. When the link names a heading, such as `#install`, Laura puts the heading in the middle of the pane. Laura opens an `http` or `https` link in a new browser tab.
- Laura shows an image that the file references by relative path, such as `![Flow](img/flow.png)`, when the image is below the folder Laura started in, outside hidden folders. For an image by an absolute path or a remote URL, Laura shows only the alt text.

When a link's file or heading doesn't exist, or the link names a folder, Laura shows `⚠ The link target <link> does not exist.`, `⚠ The file <file> has no heading #<heading>.` or `⚠ The link target <link> is a folder.`

When the workspace has no room for another pane, Laura shows `⚠ No room for another pane. Close a pane, then click the link again.` in the link's pane.

## HTML files

Laura shows an HTML file as the rendered page, with its scripts running in a sandbox that keeps them from the workspace and the shell.

- Laura loads the page's stylesheets, scripts, images and fonts by relative path from the page's folder or below it, outside hidden folders. Laura doesn't load a path that starts with `/`, such as `/style.css`.
- The page can't show `alert()` dialogs or keep data in browser storage or cookies.
- Click an element to move the cursor to the line of its start tag. Laura shows a pin on each element with a thread. Click the pin to move the cursor to the thread.
- Click a text field, or an element with a `tabindex` attribute such as a game's canvas, to give the page the keystrokes. The hint line shows `Keystrokes go to the page`. Press `Esc` to give the keystrokes back to Laura.
- In a page, `↑`, `↓`, `←` and `→` go to the page. Press `n` or `N` to move to a thread.
- Click a link to a file by relative path, such as `<a href="setup.html">`, and Laura opens the file as for a [link in a markdown file](#markdown-files), with the same notices. Laura opens an `http` or `https` link in a new browser tab.
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

When you submit from the file view, Laura flips the pane back to the editor view. Laura never focuses an editor pane when it opens. To leave the pane, press `Ctrl+P` then a pane id. Laura asks about [its theme](tui-navigation.md#lauras-theme) only in the TUI. After the user answers `y` or `n`, Laura shows the notice "✅ Experimental · Editor panes setup complete".

## One workspace per run

Each `laura` run is one workspace, shown in one browser tab at a time.

- When you open the URL in a second tab, Laura moves the workspace there. The first tab shows `The workspace is open in another tab.` and a **Use this tab** button.
- When you close the browser tab, the workspace keeps running. Open the URL again to return.
- When the workspace ends, the tab shows `The workspace stopped. To use Laura again, start Laura in the terminal.`
- While [the Laura server](workspace.md#the-laura-server) runs, the message for a moved workspace has a **Show the workspaces** button, which opens the Laura server's page. When the workspace ends, Laura opens the Laura server's page in the tab.
- When the tab can't connect, Laura shows `This tab cannot connect to the workspace. Make sure that Laura did not stop and that you used the full URL from the terminal.` Check that Laura still runs, then open the full URL that Laura printed.
