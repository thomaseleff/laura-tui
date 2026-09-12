# Tutorial

By the end of this tutorial you will have run one full loop in Laura with your coding agent.

This tutorial assumes you have `laura` installed along with the skills — see the [Quickstart](index.md#quickstart) to install.

## 1. Start your agent inside Laura

Run your coding agent inside the laura TUI:

```bash
laura -- claude
```

Laura opens with your agent running inside its PTY:

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

Ask your agent to *show* you something in the panel:

> *"Show me the protocol doc."*

Or, invoke the skill directly:

> *"/laura:open docs/protocol.md"*

Your agent will load and run the **open** skill, splitting the TUI with the protocol markdown doc rendered in a second panel via the `laura` cli.

```
+- Laura ----------------------------------------------------+
| [ 1 ]                                                      |
+------------------------------------------------------------+
| agent (pty)               | docs/protocol.md               |
| > ...                     | 1  # Protocol                  |
|                           | 2                              |
|                           | 3  An agent mutates a tab's ...|
|                           |                                |
|                           |                                |
|                           |                                |
+---------------------------+--------------------------------+
```

The panel is live so the file re-renders when your agent edits the file.

## 3. Mark up a line

Panels are auto-focused when opened. To switch into a panel press `Ctrl+P` and type the panel's number to jump into it. Move up and down with `↑` and `↓` and place the cursor on a line worth a note.

Press `c`, type your comment — *"tighten this"* — and press `Enter`. In-line comments accumulate within a review, comment on as many lines as you like.

## 4. Submit the review

Still within the file panel, press `Shift+S` once you are done adding comments, then type an overall note, and press `Enter`.

Your review is automatically injected into your agent chat with a review body and per-line summary of each comment, just like a PR review. Your agent will automatically pick up the review and start working on your feedback.

## Next

Run the `/laura:demo` skill to task your agent with showing you the rest - stacking a plan and logs beside your work, tailing a running service, pointing you at a range of lines.