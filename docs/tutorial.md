# Tutorial

By the end of this tutorial you will have run one full loop in Laura with your coding agent.

This tutorial assumes you have `laura` installed along with the skills — see the [Quickstart](index.md#quickstart) to install.

## 1. Start your agent inside Laura

Run your coding agent inside the laura TUI:

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

Ask your agent to *show* you something in the pane:

> *"Show me the protocol doc."*

Or, invoke the skill directly:

> *"/laura:laura docs/protocol.md"*

Your agent will load and run the **laura** skill, splitting the TUI with the protocol markdown doc rendered in a second pane via the `laura` cli.

```
+- Laura ----------------------------------------------------+
| [ 1 ]                                                      |
+------------------------------------------------------------+
| shell                     | docs/protocol.md               |
| > ...                     | 1  # Protocol                  |
|                           | 2                              |
|                           | 3  An agent mutates a tab's ...|
|                           |                                |
|                           |                                |
|                           |                                |
+---------------------------+--------------------------------+
```

The file pane reloads when your agent edits the file.

## 3. Comment on a line

Panes are auto-focused by default when opened. To switch into a pane press `Ctrl+P` and type the pane's number to jump into it. Move up and down with `↑` and `↓` and place the cursor on a line to leave a comment.

Press `c`, type your comment, then press `Enter`. Comments accumulate into an inline review, so comment on as many lines as you like.

## 4. Submit the inline review

Still within the file pane, press `Shift+S` once you are done adding comments, then type a review body, and press `Enter`.

Laura submits your inline review into the chat: the review body plus each thread, just like a PR review. Your agent will automatically pick up the inline review and start working on your feedback.

## Next

Run the `/laura:demo` skill to task your agent with showing you the rest - stacking a plan and logs beside your work, tailing a running service, highlighting a range of lines.