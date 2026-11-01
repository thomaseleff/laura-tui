# Web app

Laura shows a workspace in a browser tab instead of the terminal: the shell, file panes and editor panes. The user drives them with the same keystrokes as the TUI. The agent runs in the shell and uses the `laura` CLI exactly as it does in the TUI.

Laura shows the workspace in light colors, and shows the shell and the editor view in dark colors.

> [!WARNING]
> The web app is [experimental](cli.md#experimental-features).

For each button, chip and keystroke, see [Navigating the web app](web-app-navigation.md).

## Start the web app

Before you start, install Laura and the skills. See the [Quickstart](index.md#quickstart).

1. Run this command to start the web app and the interactive demo:

    ```bash
    LAURA_EXPERIMENTAL_WEB=1 laura --web -- claude "/laura:demo"
    ```

    In PowerShell, run `$env:LAURA_EXPERIMENTAL_WEB=1; laura --web -- claude "/laura:demo"`.

2. Open the URL that Laura prints in a web browser.

To host your shell instead of an agent CLI, run `laura --web`.

Without the variable, Laura returns the error `the web app is experimental: set LAURA_EXPERIMENTAL_WEB=1 before you start Laura`.

Laura serves the web app to this computer only. While the workspace runs, Laura shows only the line `Open this URL in a web browser:` and the URL in the terminal that started it. When the shell exits while Neovim has unsaved edits, Laura also shows the notice in that terminal. See [Global keystrokes](navigation.md#global-keystrokes).

The top bar shows the folder that Laura started in, such as `~/projects/laura`.

## Review a file

The user reviews a file pane in the web app as in the TUI, with buttons to comment, submit, and refresh.

1. Hover a line and click **+** at its right edge, or move the cursor to the line and press `c`.
2. Type the comment in the box that Laura shows right below the line, then press `Enter`.
3. When the pane has comments, click **Submit review** in the pane's header or press `Shift+S`. Laura shows the review body box under the button.
4. Type the review body, then press `Enter` to submit the inline review into the chat.

To discard the pane's unsubmitted inline review and reload the file, click the pane header's refresh button or press `Ctrl+R`.

For threads, failed submits, frozen panes and held commands, see [Threads and inline reviews](web-app-navigation.md#threads-and-inline-reviews).

## Comment on an HTML page

To comment on an element with a click, turn on comment mode:

1. Click the comment-mode button in the pane header. Laura outlines the element under the mouse and shows the line of the element's start tag above the element.
2. Click the element. Laura moves the cursor to the element and opens the comment box below the element. The page's own click handlers don't run.
3. Click **Done** or press `Esc` to turn off comment mode. Laura hides the thread until the user moves the cursor. `Esc` turns off comment mode from any pane except the shell and an editor pane in the editor view: in those panes, `Esc` goes to the program in the pane, such as the agent. When a text field or a dialog in a page has the keystrokes, the first `Esc` goes to the field or the dialog, and the next `Esc` turns off comment mode.

Press `c` to comment on the element at the cursor; Laura shows no **+** in a page. When no element sits at the cursor, `c` turns on comment mode. Press `Shift+S` to submit the inline review. Laura quotes the line of the element's start tag in the inline review, as in the TUI.

For how Laura shows an HTML page, see [HTML files](web-app-navigation.md#html-files).

## Show a web app in a browser pane

A **browser pane** shows a page from a URL, such as the agent's dev server, a library's documentation or an issue. Laura's browser loads the page, and the pane shows the page live. When the page changes, the pane shows the change.

> [!NOTE]
> Browser panes are [experimental](cli.md#experimental-features). A browser pane is a minimal implementation of a pattern that is still evolving.

To show a page in a browser pane:

1. Install Chrome or Edge.
2. Start Laura with both variables:

    ```bash
    LAURA_EXPERIMENTAL_WEB=1 LAURA_EXPERIMENTAL_BROWSER=1 laura --web
    ```

    In PowerShell, run `$env:LAURA_EXPERIMENTAL_WEB=1; $env:LAURA_EXPERIMENTAL_BROWSER=1; laura --web`.

3. In the shell, run `laura open http://localhost:5173`. Use the URL of your page.

Laura finds Chrome, Edge or Chromium where their installers put them. To use a different browser, set `LAURA_BROWSER` to its path before you start Laura.

`laura open <url>` returns one of these errors when Laura can't show the page:

- Without `LAURA_EXPERIMENTAL_BROWSER=1`: `browser panes are experimental: set LAURA_EXPERIMENTAL_BROWSER=1 before starting Laura`.
- In the TUI: `browser panes need the web app: start Laura with --web`.
- With no Chrome or Edge: `browser panes need Chrome or Edge: install one or set LAURA_BROWSER to its path`.

Laura's browser is separate from the user's own browser. Laura's browser starts with the first browser pane, and stops when the workspace ends.

The pane header shows the page's title and URL. The **live badge** shows `● live` while the pane shows the page. When the page's frames arrive more than 100 ms late, the live badge shows the lag in amber, such as `● 140 ms`. Hover the live badge to see the frames a second and the lag.

To refresh the pane, press `Ctrl+R` or click the pane header's refresh button. Laura discards the pane's unsubmitted inline review and the agent's highlight, and reloads the page.

### Use the page

The user uses the page in a browser pane as in a browser tab:

- **Click.** Click a link or a button in the page, and the page responds as in a browser tab. A right-click goes to the page, and the web app shows no menu. A link or a script that opens a new tab or a new window opens the page in the same pane. Click a dropdown list, then press `↑` or `↓` to change the choice.
- **Scroll.** Scroll with the mouse wheel or the touchpad over the page, or press the arrow keys, `PageUp`, `PageDown` or `Space`.
- **Type.** Click a text field in the page, and the keystrokes go to the page. When the page focuses a text field as it loads, or a click makes the page focus a text field, such as a button that opens a form, the keystrokes go to the page too. The hint line shows `Keystrokes go to the page`. Press `Esc` to give the keystrokes back to Laura. The field in the page loses the focus, and the scroll keys scroll the page again. `Ctrl+V` (`Cmd+V` on macOS) pastes the clipboard into the field.
- **Copy.** Drag over text in the page to select it. When the user releases the mouse button, Laura copies the selected text to the clipboard.
- **Go back and forward.** Click the back or forward button before the page's title, or press `Alt+←` or `Alt+→`.
- **Go to a URL.** Click the URL in the pane header, type a URL, and press `Enter`. Press `Esc` to put back the page's URL. A URL with no scheme, such as `localhost:5173`, opens with `http://` when the host is an IP address, a name with no dot such as `localhost`, or a name that ends in `.localhost`, `.local` or `.internal`. Every other host gets `https://`. A browser pane opens only `http` and `https` URLs. For any other URL, Laura shows the notice `⚠ Use an http or https URL in a browser pane.`.

The pointer over the page takes the shape that the page gives it, such as a hand over a link. The pane header shows a spinner while the page loads, while the page still handles a click, or while the page waits on a request that a click started.

### Sign in to a site

Laura's browser uses a new, empty profile for each workspace. The page has none of the user's logins, cookies or extensions.

To use a site that needs a login, sign in to the site in the browser pane.

> [!IMPORTANT]
> The agent and every other program on this computer can read the pages in Laura's browser. This includes the cookies of a site that the user signs in to. Sign in only to a site that the agent can see.

When the workspace ends, Laura deletes the profile with its logins and cookies.

Laura's browser does not download files. When the user or the agent clicks a download link, Laura shows the notice `⚠ Laura's browser does not support downloads.`.

Laura's browser denies each permission request, such as for the location, the camera or notifications. The user sees no prompt.

### Comment on a browser pane

The user comments on an element in a browser pane, as on an [HTML page](#comment-on-an-html-page):

1. Point at the element and press `c`. Or click the comment-mode button in the pane header, then click the element. In comment mode, Laura outlines the element under the pointer and shows the element's CSS selector above the element. The page doesn't get the click.
2. Type the comment in the box below the element, then press `Enter`.
3. Press `Shift+S` to submit the inline review.

When the pointer is not on the page, `c` turns on comment mode. When the pointer is on the page but on no element, Laura shows the notice `⚠ Point at an element to comment on it.`. While the page loads, Laura shows the notice `⚠ When the page loads, try again.`.

Laura names each element with its CSS selector, such as `#email` or `main > form > button:nth-of-type(2)`, and with its quote, the first 60 characters of its text. The thread's chip shows the selector, and the thread's label shows the quote. When a selector is long, Laura shows its end, after `…`. In a web component or a frame inside the page, Laura names the web component or the frame.

Laura keeps each thread on its element when the page reloads or the dev server updates the page. When the element at the selector has other text, Laura uses the element whose text starts with the quote. When the user presses `c` on an element that has a thread, Laura opens the thread, also after the page changes the element's text.

After a page starts to load, Laura waits up to 5 seconds for an element that the page shows late, such as after the page gets its data.

When the page doesn't show a thread's element, Laura shows the thread only in the thread list, and shows a notice:

- For one thread, with the first 40 characters of the thread's first comment: `⚠ The thread "<comment>" is only in the thread list, because the page lost its element.`
- For more threads: `⚠ <n> threads are only in the thread list, because the page lost their elements.`

Laura shows no outline, pin or card for the thread on the page, and `n` and `N` skip the thread. In the thread list, Laura dims the thread and marks it `not on the page`. Laura keeps the thread in the inline review.

To reply to the thread or delete it, click the thread's row in the thread list. Laura opens the thread's card below the row. The card shows **Reply…**, or **Edit…** and **Delete** when the user wrote the last comment. When the user clicks **Reply…**, Laura opens the comment box in the card. While the user writes the comment, Laura keeps the thread list open. When the page loses the element while the user writes a reply to the thread, Laura moves the comment box into the thread list.

When the page shows the element again, Laura shows the thread on the page again and shows no notice.

Laura keeps each thread on its page. When the user follows a link to another page, the thread list shows the thread's page, such as `/settings`. When the user goes back, Laura shows the thread on the page again. In an app that puts its route after `#/`, such as `/#/settings`, each route is a page.

The agent can comment on an element and highlight an element by CSS selector (see [Browser panes](cli.md#browser-panes)). Laura shows the agent's threads on the elements, as the user's threads. Laura outlines the highlighted element and scrolls the page to the element. Press `h` to clear the highlight.

For the inline review that the agent receives, see [Read an inline review](agent-reference.md#read-an-inline-review).

### Answer the page's dialogs

When the page shows a dialog, such as `alert()`, `confirm()` or `prompt()`, the web app shows the dialog over the page. The dialog names the page's host, such as `localhost:5173 says`, and shows the page's message. The page waits until the user answers the dialog.

- Press `Enter` or click **OK** to accept the dialog. For a `prompt()` dialog, type the answer in the dialog's field first.
- Press `Esc` or click **Cancel** to close the dialog without accepting it.

When the user leaves a page that asks before it closes, the dialog asks `Leave this page? Changes you made may not be saved.` Press `Enter` or click **Leave** to leave the page. Press `Esc` or click **Stay** to stay on the page.

When the page doesn't load, Laura shows a notice with the error, such as `⚠ The page http://localhost:5173 did not load (net::ERR_CONNECTION_REFUSED).`. Start the dev server, then press `Ctrl+R`.

When the page crashes, Laura shows the notice `⚠ To load the page again after it stopped, press Ctrl+R.` Press `Ctrl+R` in the pane, and Laura loads the page again.

While Laura's browser starts, the user can keep working in the workspace. The agent's commands wait until Laura's browser has started, then run in order.

When the user presses `Ctrl+R` or enters a URL in the address field, and Laura's browser doesn't start, Laura shows the notice `⚠ Laura's browser didn't start. To try again, press Ctrl+R in a browser pane.` When the agent has run `laura ready`, Laura records why in the journal's `browser` event, with the last lines of the browser's own output. To read the event, run `laura journal type=browser -n 1`. For the event's fields, see [Journal](cli.md#journal).

When Laura's browser stops, Laura shows the notice `⚠ To start Laura's browser again, press Ctrl+R in a browser pane.` Laura keeps the browser panes. Press `Ctrl+R` in a browser pane, and Laura starts Laura's browser again with every browser pane's page.

For the browser pane's buttons and keystrokes, see [Browser panes](web-app-navigation.md#browser-panes).

### Work on a page with the agent

The agent can use a browser tool on the page in a browser pane. The agent clicks, types and runs scripts on the same page that the user sees. A browser tool is optional, and browser panes work without one. Laura doesn't install a browser tool. The user installs one and picks which. Both tools below need Node.

To let the agent use a browser tool, add the tool to the agent once per project. For Claude Code, run one of these commands:

```bash
# chrome-devtools-mcp: debugging, the console and network, performance traces
claude mcp add chrome-devtools -- npx chrome-devtools-mcp@latest --browser-url='${LAURA_BROWSER_URL}'

# Playwright MCP: flows through the app, Playwright tests
claude mcp add playwright -- npx @playwright/mcp@latest --cdp-endpoint='${LAURA_BROWSER_URL}'
```

Laura sets `LAURA_BROWSER_URL` in the shell when Laura can show browser panes. Keep the single quotes: Claude Code then reads `LAURA_BROWSER_URL` each time the tool starts, so the tool works in every workspace. The tool connects to Laura's browser on its first use after a browser pane opens. Before Laura's browser starts, the tool gets the error `Laura's browser is not running. Open a browser pane, then try again.` With `-s project`, Claude Code writes the tool into the project's `.mcp.json` and asks the user to approve the tool once. For another agent, see the setup steps of [chrome-devtools-mcp](https://github.com/ChromeDevTools/chrome-devtools-mcp) or [Playwright MCP](https://github.com/microsoft/playwright-mcp).

The tool's page list shows only the pages in browser panes. When the agent clicks, types or changes the page with the tool, the user sees each change in the browser pane at once.

Laura shows a ring where the agent clicks. Laura shows an outline on the field where the agent types. Each mark goes away after one second. Laura shows no mark for the user's own clicks and typing. When a page's own script clicks or changes a field, Laura can show a mark for it.

While the agent clicks or types with the tool, the pane header shows **Agent**. Laura hides **Agent** five seconds after the agent's last click or keystroke. When the agent only reads the page or opens another page, the pane header doesn't show **Agent**.

When the agent takes a screenshot of the page with the tool, Laura flashes the page in the browser pane once. The pane header shows **Screenshot** for four seconds. When the user's computer is set to reduce motion, Laura shows **Screenshot** with no flash.

The agent's screenshots and page snapshots show the page as the user's own browser would. They don't show Laura's rings, outlines, thread pins or highlights.

The user and the agent comment on the same elements. The agent finds an element with the tool, then runs `laura comment '<selector>'` or `laura highlight '<selector>'` on it. The user's threads and the agent's threads show on the elements in the same pane. When the tool closes the pane's page, Laura opens the page again at the same URL, with the threads.

When the tool emulates a device, such as a phone, the browser pane shows the page at the device's size. Laura puts the page in the middle of the pane, and makes the page smaller when the page is larger than the pane. The pane header shows the size, such as `393×852`. When the agent resets the emulation, the page fills the pane again.

The agent can also answer the page's dialogs with the tool.

> [!IMPORTANT]
> Laura takes browser tools on a port on `127.0.0.1` and passes them to Laura's browser. Every program on this computer can connect to the port and control the pages in Laura's browser.

## Quit

Press `Ctrl+Q`, then `y`, to end the workspace, as in the [TUI](navigation.md#global-keystrokes).

Don't press `Ctrl+C` in the terminal that started Laura. Laura ends the workspace at once, and you lose Neovim's unsaved edits.

On Linux, Firefox quits on `Ctrl+Q` before Laura receives the keystroke. Use a different browser instead.
