# Tutorial

In this tutorial, you run one full loop with your coding agent in Laura: your agent shows you a file, you comment on the file, and you submit an inline review into the chat.

Before you start, install Laura and the skills. See the [Quickstart](index.md#quickstart).

## 1. Start your agent inside Laura

Start your coding agent in Laura:

```bash
laura -- claude
```

Laura opens the workspace in your browser, with your agent running in the shell:

```
+- ~/projects/app ------------------------- [ Find a pane ] [ ? ] -+
| 0  shell                                                          |
| > _                                                               |
|                                                                   |
|                                                                   |
+-------------------------------------------------------------------+
```

> [!TIP]
> To follow the tutorial in the terminal, run `laura --tui -- claude`. The keystrokes are the same.

## 2. Ask to see a file

Ask your agent to *show* you a file in a pane, such as *"Show me the protocol doc."* Your agent uses the `laura` CLI to open the file in a new pane:

```
+- ~/projects/app ------------------------- [ Find a pane ] [ ? ] -+
| 0  shell                 | 1  protocol.md          [ + ] [ x ]  |
| > ...                    |  # Protocol                          |
|                          |  The protocol is NDJSON ...          |
|                          |                                      |
+--------------------------+--------------------------------------+
```

Laura reloads the file pane when the file changes, for example when your agent edits the file.

## 3. Comment on a line

Click a line, then click **+** or press `c`. Type your comment, then press `Enter`. Your comments add up to one inline review, so comment on as many lines as you like.

## 4. Submit the inline review

Click **Submit review** or press `Shift+S`, type a review body, then press `Enter`.

Laura submits your inline review into the chat, with the review body and each thread, like a PR review. Your agent starts working on your feedback.

## Next

Run `/laura:demo` to have your agent show you the rest: laying out panes, reviewing a plan and its code, tailing a running API, showing a page, switching workspaces, and recording feedback. For every keystroke and click, see [Navigating the workspace](navigation.md).
