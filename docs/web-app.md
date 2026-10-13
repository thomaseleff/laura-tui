# Web app

Laura shows a workspace in a browser tab instead of the terminal: the shell, file panes and editor panes. The user drives them with the same keystrokes as the TUI. The agent runs in the shell and uses the `laura` CLI exactly as it does in the TUI.

> [!NOTE]
> The web app is [experimental](cli.md#experimental-features): a minimal first take on a browser view of the workspace. The pattern is still evolving.

## Start the web app

1. Set `LAURA_EXPERIMENTAL_WEB=1`.
2. Run `laura --web`, or `laura --web -- <cmd>` to host `<cmd>` instead of your shell.
3. Open the URL that Laura prints in Chrome or Edge.

Without the variable, Laura returns the error `the web app is experimental: start Laura with LAURA_EXPERIMENTAL_WEB=1`.

Laura serves the web app to this computer only. While the workspace runs, Laura shows only the URL in the terminal that started it.

## Keystrokes and clicks

Laura uses the TUI's keystrokes in the web app. See [Navigating the TUI](navigation.md) for the full list. Laura doesn't support commenting (`c`) or submitting an inline review (`Shift+S`) in the web app yet, so use the TUI for an inline review.

- Click a pane to focus it.
- Click a line in a file pane to move the cursor to that line.
- Drag to select text. Laura copies the selection when you release the button. From a file pane, Laura copies the source without the line-number gutter.
- Press `Ctrl+Shift+V` to paste. Laura sends `Ctrl+V` to the shell, as a terminal does.

The browser keeps some keystrokes for itself, so Laura never receives them:

- On Windows and Linux, the browser keeps `Ctrl+W`, `Ctrl+T`, `Ctrl+N` and `Ctrl+Tab`. In Neovim, run `:wincmd` in place of `Ctrl+W`.
- On macOS, the browser keeps only `Ctrl+Tab`.

## Quit

Press `Ctrl+Q`, then `y`, to end the workspace, as in the [TUI](navigation.md#global-keystrokes).

Don't press `Ctrl+C` in the terminal that started Laura. Laura ends the workspace at once, and you lose Neovim's unsaved edits.

On Linux, Firefox quits on `Ctrl+Q` before Laura receives the keystroke. Use Chrome or Edge instead.

## One workspace per run

Each `laura --web` run is its own workspace, shown in one browser tab at a time.

- When you open the URL in a second tab, the workspace moves to the second tab. In the first tab, Laura shows `This workspace moved to another tab.` and a **Continue here** button that moves the workspace back.
- When you close the browser tab, the workspace keeps running. Open the URL again to return to the workspace.
- When the workspace ends, Laura shows `This workspace ended.` in the browser tab.

## Editor panes

Laura asks about [its theme](navigation.md#lauras-theme) only in the TUI. Until you answer in the TUI, editor panes in the web app use your Neovim's own theme.
