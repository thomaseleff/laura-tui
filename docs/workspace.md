# Use the workspace

Laura shows the workspace in a browser tab: the shell, file panes, browser panes and editor panes. The agent runs in the shell and uses the `laura` CLI to open and arrange the panes. For each button, chip and keystroke, see [Navigating the workspace](navigation.md).

## Start Laura

Before you start, install Laura and the skills. See the [Quickstart](index.md#quickstart).

1. Run Laura with your agent and the interactive demo:

    ```bash
    laura -- claude "/laura:demo"
    ```

2. Laura opens the workspace in a Chrome or Edge window, or in your default browser. If no browser opens, open the URL that Laura prints.

To choose the browser, set `LAURA_BROWSER` to its path. When Laura can't open a web browser, Laura shows `Laura could not open a web browser: <reason>`, and the workspace keeps running. Laura serves the workspace to this computer only.

> [!TIP]
> To show the workspace in the terminal, run `laura --tui -- claude`. See [Navigating the TUI](tui-navigation.md).

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

> [!IMPORTANT]
> Every program on this computer can connect to the port that the browser tools use, and control the pages in Laura's browser.

## Quit

Press `Ctrl+Q`, then `y`. Don't press `Ctrl+C` in the terminal that started Laura: Laura ends at once, and you lose Neovim's unsaved edits. On Linux, Firefox quits on `Ctrl+Q` before Laura receives it, so use a different browser.
