# Use the workspace

Laura shows the workspace in a browser tab: the shell, file panes, browser panes and editor panes. The agent runs in the shell and uses the `laura` CLI to open and arrange the panes. For each button, chip and keystroke, see [Navigating the workspace](navigation.md).

## Start Laura

Before you start, install Laura and the skills. See the [Quickstart](index.md#quickstart).

1. Run Laura with your agent and the interactive demo:

    ```bash
    laura -- claude "/laura:demo"
    ```

2. Laura opens the workspace in a new tab of your default browser. If no browser opens, open the URL that Laura prints.

To choose the browser, set `LAURA_BROWSER` to its path. When Laura can't open a web browser, Laura shows these two lines, and the workspace keeps running:

```
Laura could not open a web browser: <reason>
To use Laura in this terminal instead, press Ctrl+C and run laura --tui.
```

Laura serves the workspace to this computer only. Over SSH, run `laura --tui`, or forward the port in the URL to your computer and open the URL there.

> [!TIP]
> To show the workspace in the terminal, run `laura --tui -- claude`. See [Navigating the TUI](tui-navigation.md).

## The shell

The agent runs in the shell, pane `0`. Laura shows the shell in white, with Laura's light colors, so the chat reads like the rest of the workspace. When the agent prints a color that is hard to read on white, Laura darkens the color. An editor pane keeps its dark colors.

The pane header names the shell after the agent: the name that the agent gives with `laura ready --agent`, such as `claude`. Before the agent runs `laura ready`, the pane header shows `shell`. The pane selector shows the same name, also in the TUI.

Laura shows the shell in a drawer, with the [chat](navigation.md#the-chat-and-the-shell) of what the agent did through Laura. Before the agent opens a pane, Laura shows a start canvas above the drawer. When the agent opens a pane, the panes fill the workspace, and the drawer floats over the panes. Press ``Ctrl+` `` to open or close the drawer. To keep the shell beside the panes, click **Show the shell as a pane** in the drawer. See [The drawer](navigation.md#the-drawer).

## Know when the agent waits

When the agent waits for you, for example at a permission prompt, Laura marks the workspace as waiting:

- While the shell is in the drawer, the shell button turns amber and shows `Waiting for you` when the agent said nothing since you last wrote to the shell. See [The drawer](navigation.md#the-drawer).
- The browser tab's icon shows an amber dot.
- The other workspaces show an amber dot in the workspace switcher and `Waiting for you` on this workspace's row. See [Switch workspaces](navigation.md#switch-workspaces).
- The Laura server's page shows `Waiting for you` on this workspace's row. See [Open a running workspace](#open-a-running-workspace).

The wait ends when you:

- Move to the shell, for example with `` Ctrl+` `` or a click.
- Press `F12`, so that every keystroke goes to the shell.
- Close the pane selector, the list of keystrokes or the list of workspaces while the shell has the focus.
- Type, paste, scroll or click in the shell.
- Submit an inline review.
- Send a message from the chat's message box.
- Close a pane, and Laura moves the focus to the shell.

The wait does not end when you move the pointer over the shell, when the agent moves the focus to the shell, when you come back to the browser tab, or when you click or type in the chat.

Laura shows a wait when the program in the shell rings the terminal bell or sends a terminal notification.

Claude Code's default signal depends on the terminal that started Laura. Under iTerm2 and Ghostty, Claude Code sends a terminal notification and Laura shows the wait. In other terminals, for example Windows Terminal, Claude Code sends no signal. The terminal bell works in every terminal. To set up Claude Code:

1. Add this setting to `~/.claude/settings.json`:

   ```json
   { "preferredNotifChannel": "terminal_bell" }
   ```

2. Restart Claude Code.

Other agent CLIs work too when they ring the terminal bell or send a terminal notification. Look in the agent CLI's settings for a terminal bell or terminal notifications.

The shell's own bell also marks the workspace, for example when bash rings the bell on a failed `Tab` completion. Your next keystroke ends that wait.

## Review a file

1. Hover a line and click **+**, or move the cursor to the line and press `c`.
2. Type the comment, then press `Enter`. Comment on as many lines as you like.
3. Click **Submit review** or press `Shift+S`, type the review body, then press `Enter`. Laura submits the inline review into the chat.

When the agent edits the file while you review it, Laura freezes the pane, so your comments stay on their lines. Submit the inline review, or click **Refresh** (`Ctrl+R`) to discard it and reload the file. See [Threads and inline reviews](navigation.md#threads-and-inline-reviews).

## Comment on an HTML page

1. Click the comment-mode button in the pane header. Laura outlines the element under the pointer.
2. Click the element, type the comment, then press `Enter`. The page's own click handlers don't run.
3. Click **Done** or press `Esc` to turn off comment mode.

`Esc` turns off comment mode from any pane except the shell and an editor pane's editor view, where `Esc` goes to the program in the pane. Laura quotes the element's start tag in the inline review. See [HTML files](navigation.md#html-files).

## Show a web app in a browser pane

A **browser pane** shows a page from a URL, such as the agent's dev server, a library's documentation or an issue. Laura's browser loads the page, and the pane shows the page live.

> [!NOTE]
> A browser pane is a minimal implementation of a pattern that is still evolving.

1. Install Chrome or Edge. Laura finds Chrome, Edge or Chromium where their installers put them, or the browser that `LAURA_BROWSER` names.
2. Start Laura with `laura`.
3. Ask the agent to show the page, or run `laura open http://localhost:5173` in the shell.

When Laura finds no Chrome or Edge, `laura open <url>` returns `browser panes need Chrome or Edge: install one or set LAURA_BROWSER to its path`.

The **live badge** shows `● live`. When the page's frames arrive more than 100 ms late, the live badge shows the lag, such as `● 140 ms`.

### Use the page

Click, scroll and type in the page as in a browser tab. Press `Esc` to give the keystrokes back to Laura. To go to another URL, click the URL in the pane header, type the URL, and press `Enter`.

- Laura's browser uses a new, empty profile for each workspace, so the page has none of your logins, cookies or extensions. Laura deletes the profile when the workspace ends.
- Laura's browser doesn't download files, and denies each permission request, such as for the camera.
- When the page shows a dialog, such as `confirm()`, press `Enter` or click **OK** to accept, or press `Esc` or click **Cancel**.

> [!IMPORTANT]
> The agent and every other program on this computer can read the pages in Laura's browser, also the cookies of a site that you sign in to. Sign in only to a site that the agent can see.

When the page doesn't load, Laura shows a notice with the error, such as `⚠ The page http://localhost:5173 did not load (net::ERR_CONNECTION_REFUSED).` Start the dev server, then press `Ctrl+R`. When Laura's browser stops or doesn't start, press `Ctrl+R` in a browser pane to start it again. To read why, run `laura journal type=browser -n 1`.

### Comment on a browser pane

1. Point at an element and press `c`, or turn on comment mode and click the element.
2. Type the comment, then press `Enter`.
3. Press `Shift+S` to submit the inline review.

Laura names each element by its CSS selector and its first 60 characters of text, and keeps each thread on its element when the page reloads. When the page loses an element, Laura shows its thread only in the thread list and keeps the thread in the inline review. The agent can comment on and highlight an element by CSS selector. See [Read an inline review](agent-reference.md#read-an-inline-review).

### Work on a page with the agent

The agent can use a browser tool to click, type and run scripts on the page that you see. A browser tool is optional, and you choose and install it. Both tools below need Node. For Claude Code, run one of these commands once per project:

```bash
# chrome-devtools-mcp: debugging, the console and network, performance traces
claude mcp add chrome-devtools -- npx chrome-devtools-mcp@latest --browser-url='${LAURA_BROWSER_URL}'

# Playwright MCP: flows through the app, Playwright tests
claude mcp add playwright -- npx @playwright/mcp@latest --cdp-endpoint='${LAURA_BROWSER_URL}'
```

Keep the single quotes, so the tool reads `LAURA_BROWSER_URL` in each workspace. Until a browser pane opens, the tool returns an error with status 503. For another agent, see the setup of [chrome-devtools-mcp](https://github.com/ChromeDevTools/chrome-devtools-mcp) or [Playwright MCP](https://github.com/microsoft/playwright-mcp).

You see each change that the agent makes. Laura shows a ring where the agent clicks and an outline where the agent types, and the pane header shows **Agent**, **Screenshot** or **● Recording**. The agent's screenshots show the page without Laura's marks.

Laura also shows a ring or an outline when the page's own script clicks or types, for example a page that clicks a button some time after your click.

> [!IMPORTANT]
> Every program on this computer can connect to the port that the browser tools use, and control the pages in Laura's browser.

## The Laura server

The Laura server is one address in the browser that lists the running workspaces and starts new ones. Each workspace still has its own address. Bookmark the Laura server's address to start workspaces without a terminal.

To start the Laura server, select [the Laura icon](#start-laura-from-a-desktop-icon). To start the Laura server from a terminal instead:

1. Run the Laura server:

    ```bash
    laura --serve
    ```

2. Open the URL that Laura prints. The address is `http://127.0.0.1:1989/`, and the URL adds a key. To use a different port, run `laura --serve --port <n>`. One Laura server runs for each user. When the Laura server already runs, `laura --serve` prints the URL of that Laura server again, also with a different `--port`.
3. The page shows **Start a workspace** and the list of the running workspaces.

After the first visit, the browser keeps the key. The plain address `http://127.0.0.1:1989/` and a bookmark of it then work in that browser, also after the browser restarts, until the Laura server stops.

Laura makes a new key each time the Laura server starts. After the Laura server starts again, open the URL that Laura prints again. A browser without the key shows `Open the Laura server` and no workspace.

### Start a workspace

1. Type the folder in **Folder**, or select **Browse…** and pick the folder. `~` is the home folder. The field starts at the last folder that you started a workspace in.
2. Select the program: **Shell**, or **Command** with a command such as `claude`.
3. Select **Start workspace**. The browser opens the new workspace in the same tab.

In the folder browser:

- A click selects a folder.
- A double-click, or `Enter`, opens the selected folder.
- A name in the path at the top opens that folder.
- **Use this folder** takes the selected folder, or the open folder when you selected none.
- The folder browser marks each git repository with `git`, and lists no folder whose name starts with a dot.

When the workspace does not start, the page shows one of these errors below the button:

| Error | Cause |
|---|---|
| `<folder> is not a folder.` | The folder does not exist. |
| `The workspace failed to start.` | Laura could not start the program, for example a command that is not installed, or the command ended at once, for example a command that prints and stops, such as `ls`. Select **Show the error** to read Laura's error, or `The program exited at once.` |
| `The workspace timed out after 10 s.` | The workspace took too long to start. Laura stops that workspace. |

The Laura server splits the command at each space, so an argument can't contain a space.

A workspace that the Laura server started has no terminal. The workspace runs until its shell exits or you [quit](#quit) in the workspace, also after the Laura server stops.

### Open a running workspace

**Running** lists each workspace that runs on this computer, the workspace that you [visited](navigation.md#switch-workspaces) last first, also the workspaces that you started in a terminal with `laura`. Each row shows the session label, or the folder name in grey before the agent sets a session label, then the folder and the program.

- **Open** opens the workspace in this tab.
- **↗** opens the workspace in a new tab.

While the agent in a workspace [waits for you](#know-when-the-agent-waits), the row's dot is amber and the row shows `Waiting for you`.

The page reloads the list every 5 seconds. A workspace that ended leaves the list.

### Stop the Laura server

Select **Stop the Laura server** at the bottom of the page, then **Stop**. The workspaces keep running. Select the Laura icon, or run `laura --serve`, to start the Laura server again. `Ctrl+C` in the Laura server's terminal also stops only the Laura server.

## Start Laura from a desktop icon

The Laura icon is an icon on your desktop or in your app menu that opens the Laura server in your default browser. Use the Laura icon to start workspaces without a terminal.

1. Install Laura. See the [Quickstart](index.md).
2. Write the Laura icon one time:

    ```bash
    laura --shortcut
    ```

    Laura prints the path of each file that Laura writes. The Laura icon shows in these places:

    - Windows and WSL: on the Desktop and in the Start Menu.
    - Linux: in the app menu, as **Laura**.
    - macOS: in Launchpad, in Spotlight, and in **Applications** in your home folder.
3. Select the Laura icon. Laura starts the Laura server and opens the Laura server's page in a new tab of your default browser.

When the Laura server already runs, the Laura icon opens the Laura server's page in a new tab. When you select the Laura icon again while the Laura server starts, Laura opens a second tab of the same Laura server. After you upgrade Laura, the Laura icon stops the Laura server of the old version and starts the new version. The workspaces keep running.

The Laura icon holds the full path of the `laura` program. When that path changes, for example when you install Laura with a different tool, run `laura --shortcut` again.

When the Laura server can't start, Laura opens a page that shows `Laura could not start` and the error. When another program uses the Laura server's port, the error is `Another program uses port <n>. Stop that program, then open Laura from your desktop or app menu, or run laura --serve again.`

The Laura icon runs your shell as a login shell, so the workspaces that the Laura server starts find the programs on your `PATH`, such as `claude`. With csh or tcsh, Laura uses `/bin/sh`.

When your shell startup file starts tmux or zellij with `exec`, the Laura icon starts tmux or zellij instead of Laura. Start tmux or zellij only in a shell that runs no command:

```bash
[ -z "$BASH_EXECUTION_STRING$ZSH_EXECUTION_STRING" ] && exec tmux
```

### The files that Laura writes

To remove the Laura icon, delete these files.

| Computer | Files |
|---|---|
| Linux | `~/.local/share/applications/laura.desktop` and `~/.local/share/icons/laura.png` (in `$XDG_DATA_HOME` when you set it) |
| macOS | `~/Applications/Laura.app` |
| Windows | `Laura.lnk` on the Desktop and in the Start Menu, and `%USERPROFILE%\.laura\laura-<id>.ico` |
| WSL | `Laura.lnk` on the Windows Desktop and in the Windows Start Menu, and `%USERPROFILE%\.laura\laura-<id>.ico` in Windows |

On Windows, the Laura icon runs `lauraw.exe`, which starts the Laura server with no console window. On WSL, the Laura icon starts the Laura server in your Linux distribution, in a minimized terminal window.

On Windows, `uv tool upgrade laura-tui` and `pip install -U laura-tui` can't replace `laura.exe` while the Laura server runs. Before you upgrade, select **Stop the Laura server** at the bottom of the Laura server's page.

## Quit

Press `Ctrl+Q`, then `y`. Don't press `Ctrl+C` in the terminal that started Laura: Laura ends at once, and you lose Neovim's unsaved edits. On Linux, Firefox quits on `Ctrl+Q` before Laura receives it, so use a different browser.
