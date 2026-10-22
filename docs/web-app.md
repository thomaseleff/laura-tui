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

Laura serves the web app to this computer only. While the workspace runs, Laura shows only the line `Open this URL in a web browser:` and the URL in the terminal that started it.

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
3. Click **Done** or press `Esc` to turn off comment mode. Laura hides the thread until the user moves the cursor.

Press `c` to comment on the element at the cursor; Laura shows no **+** in a page. When no element sits at the cursor, `c` turns on comment mode. Press `Shift+S` to submit the inline review. Laura quotes the line of the element's start tag in the inline review, as in the TUI.

For how Laura shows an HTML page, see [HTML files](web-app-navigation.md#html-files).

## Quit

Press `Ctrl+Q`, then `y`, to end the workspace, as in the [TUI](navigation.md#global-keystrokes).

Don't press `Ctrl+C` in the terminal that started Laura. Laura ends the workspace at once, and you lose Neovim's unsaved edits.

On Linux, Firefox quits on `Ctrl+Q` before Laura receives the keystroke. Use a different browser instead.
