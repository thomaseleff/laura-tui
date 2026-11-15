# How Laura got here

Laura started in the terminal and moved to the browser. This page tells the history of Laura, and shows how to run the TUI.

## Show, don't tell — the TUI

Laura started as a TUI, a workspace in the terminal. The agent opens the file, the plan or the diff in a pane next to the chat, instead of pasting the content into the chat. The user comments on the pane and submits an inline review into the chat.

The package keeps the name from this time: `laura-tui`.

## See it run — the browser

Laura then came to the browser. In the [web app](web-app.md), Laura shows HTML files as rendered pages. The agent opens the user's app in a [browser pane](web-app.md#show-a-web-app-in-a-browser-pane), so the user sees the app run next to the chat and comments on the page.

Plain `laura` now opens the workspace in the web app.

## Meet at the artwork — next

Next, the agent and the user meet at the artwork: the page, the app or the document that they make together.

## Run the TUI

Laura still supports the TUI. To show the workspace in the terminal, start Laura with `--tui`:

```bash
laura --tui -- claude
```

For the keystrokes in the TUI, see [Navigating the TUI](navigation.md). In the TUI, `laura open <url>` returns `browser panes need the web app: start Laura without --tui`.
