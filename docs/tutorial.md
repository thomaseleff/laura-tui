# Tutorial

In this tutorial, you will run one full loop with your coding agent in Laura: your agent shows you a file, you comment on the file, and you submit an inline review into the chat.

Before you start, install Laura and the skills. See the [Quickstart](index.md#quickstart).

## 1. Start your agent inside Laura

Start your coding agent in Laura:

```bash
laura -- claude
```

Laura opens with your agent running in the shell:

```
+- Laura ----------------------------------------------------+
| [ 1 ]                                                      |
+------------------------------------------------------------+
| > _                                                        |
|                                                            |
|                                                            |
|                                                            |
|                                                            |
|                                                            |
|                                                            |
+------------------------------------------------------------+
```

## 2. Ask to see a file

Ask your agent to *show* you a file in a pane:

> *"Show me the protocol doc."*

Or run the skill directly:

> *"/laura:laura docs/protocol.md"*

Your agent runs the **laura** skill, and uses the `laura` CLI to split the workspace and open the protocol doc in a new pane.

```
+- Laura ----------------------------------------------------+
| [ 1 ]                                                      |
+------------------------------------------------------------+
| shell                     | docs/protocol.md               |
| > ...                     | 1  # Protocol                  |
|                           | 2                              |
|                           | 3  The protocol is NDJSON ...  |
|                           |                                |
|                           |                                |
|                           |                                |
+---------------------------+--------------------------------+
```

Laura reloads the file pane when the file changes, for example when your agent edits the file.

## 3. Comment on a line

Laura focuses a new pane when the pane opens. To focus another pane, press `Ctrl+P` then the pane's number. Press `↑` and `↓` to move the cursor to the line you want to comment on.

Press `c`, type your comment, then press `Enter`. Your comments add up to one inline review, so comment on as many lines as you like.

## 4. Submit the inline review

When you're done commenting, press `Shift+S` in the file pane, type a review body, then press `Enter`.

Laura submits your inline review into the chat, with the review body and each thread, like a PR review. Laura returns focus to the shell, and your agent starts working on your feedback.

## Next

Run `/laura:demo` to have your agent show you the rest: laying out panes, the diff view, tailing a running service, and recording feedback.