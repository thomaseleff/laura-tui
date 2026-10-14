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
- Drag to select text. Laura copies the selection when you release the button. From a file pane, Laura copies the source without the line-number gutter.
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
