# Laura

Laura is a TUI workspace that gives your agent an **API over the TUI**, so your agent can tile panes while you pair-program.

> [!TIP]
> **New:** Laura now runs in an experimental browser tab, adding support for rendered HTML panes with all the same interactions as markdown. [Start the web app](web-app.md#start-the-web-app).

![Laura demo](assets/laura-demo.gif)

Your agent can open the file you're discussing, the plan you're following, the diff you just made, or tail the logs of your running service. Laura works with every coding agent CLI, and your agent learns the `laura` CLI from Laura's agent skills.

> [!NOTE]
> Laura makes collaborating with your coding agent more enjoyable: riffing, reviewing live, solving problems and improvising together, without leaving the terminal.

**Laura lets you:**

- **Stay in the flow** - view a file, a plan, a diff, or a log right in the TUI, without switching windows between the terminal, IDE, and browser.
- **Compose the TUI** - your agent tiles the workspace as you chat, using the `laura` CLI and skills.
- **Pick your coding agent CLI** - Laura runs your agent in a plain shell, so Laura works with every agent CLI and model.
- **Interact in every pane** - comment on any file pane and submit the inline review into the chat.
- **Edit alongside your agent** *(experimental)* - edit a file in Neovim in an editor pane next to the chat, then flip to the file view to review your edits with your agent. [Enable editor panes](cli.md#editor-panes).
- **Work in a browser** *(experimental)* - run the workspace in a browser tab instead of the terminal, with the same keystrokes. Laura shows [HTML files as rendered pages](web-app-navigation.md#html-files) only in the web app. You comment on a page and submit the inline review as you do on a markdown file in the TUI. [Start the web app](web-app.md).
- **See your web app** *(experimental)* - show your dev server's page, or any web page, live in a browser pane in the web app, next to the chat. [Show a page in a browser pane](web-app.md#show-a-web-app-in-a-browser-pane).
- **Learn while staying focused** - your agent shows and explains concepts live, step by step.

## How it works

Laura runs your agent CLI in a shell and provides agent skills that you or your agent can invoke. Your agent learns the `laura` CLI from the skills. The `laura` CLI sends messages to the workspace over an NDJSON protocol, with one socket per workspace. See the [protocol reference](protocol.md) to learn more.

## Quickstart

**1. Install Laura.**

```bash
uv tool install laura-tui          # prebuilt for Linux, macOS and Windows
cargo install laura-tui --locked   # or build from source, with Rust (stable)
```

**2. Install the skills** so your agent knows how to use the `laura` CLI. In Claude Code:

```
/plugin marketplace add thomaseleff/laura-tui
/plugin install laura@laura-tui
```

**3. Start Laura** and run the interactive demo:

```bash
laura -- claude "/laura:demo"
```

**4. Pair-program.** Chat as you normally would with your coding agent. Ask your agent to show you a file, a plan, or a diff using the `/laura` skill.

Laura reloads a file pane when its file changes. Laura renders markdown with terminal styling in the TUI and as rendered markdown in the [web app](web-app-navigation.md#markdown-files). Laura shows other files with syntax colours, and marks the lines changed since git `HEAD` in the gutter.

## Where to go next

**Tutorial**

- **[Tutorial](tutorial.md)** A step-by-step walkthrough.

**How-to guides**

- **[Web app](web-app.md)** Start the web app, review a file, comment on an HTML page and show a page in a browser pane.

**Reference**

- **[Navigating the TUI](navigation.md)** The workspace keystrokes.
- **[Navigating the web app](web-app-navigation.md)** The web app keystrokes and clicks.
- **[Agent reference](agent-reference.md)** How your agent uses the `laura` CLI.
- **[CLI reference](cli.md)** Every `laura` command and its flags.
- **[Protocol reference](protocol.md)** The messages the `laura` CLI sends to the workspace.
