---
name: explain
description: Walk the user through a PR, a file, or how something works, stepping across ranges of one or more panes one range at a time and advancing on their cue.
when_to_use: You are running inside a Laura tab (LAURA_TAB is set) and the user wants a guided walkthrough — "explain this PR", "walk me through the render loop", "how does X work".
user-invocable: true
argument-hint: [target]
---

# laura explain

Run a guided walkthrough: open the relevant files in panes, step through them one range at a time, run `laura highlight` to highlight the span you are describing, and explain it in chat.

**Rules**
- Check that `$LAURA_TAB` is set, otherwise, let the user know you are not running in a Laura workspace.
- Run `laura ready --session "$YOUR_SESSION_ID" --agent claude` once at the start. Use your **own** conversation/session id so the journal lines up 1:1 with this chat.
- After each step, **wait for `Next`**. If the user asks a question mid-step, answer it, stay on the step, and advance on `Next`.
- End with `laura close --all`.
- **Tile in a downward spiral to the right.** Every `laura open`/`tail` prints the new pane id — capture it and pass it as `--split <id>` for the *next* open, so each pane cascades off the newest one. The first open splits the shell (pane `0`); after that split the last pane you opened, **never pane `0` again**.
- See the `laura` skill for the full CLI or run `laura --help`.
- Read the docs: https://thomaseleff.github.io/laura-tui/llms.txt

## Understand the target first, then plan

Before beginning the guided explanation:

1. **Understand what and why.** Ask the user clarifying questions to discover what you are tasked with explaining and why.
2. **Explore the target yourself.** Read the diff (`git diff <base>...HEAD`), open the files, and trace the call sites until you understand it.
3. **Plan the sequence** of ranges you will step through.

## The loop

Compose the walkthrough from these actions, in whatever order the explanation calls for:

- `laura open <path>` opens a file in a pane and prints the new pane id. Capture the id to target that pane later, and open a pane per file when the walkthrough spans several.
- `laura open <path> --highlight <a> <b>` opens a file already scrolled to and highlighting lines `a`–`b`.
- `laura highlight <a> <b> --pane <id>` highlights lines `a`–`b` in an already-open pane.
- `<some-cmd> | laura tail --follow --split <id> --dir v --ratio 30` tiles a live log under a pane when the explanation needs runtime output beside the code.

A step may highlight one span, move across several spans of the same file, jump between files, or open a pane with nothing highlighted. For each step, run the calls that set up what you want the user to see, then explain it in chat.

## Close the loop — journal the session (mandatory)

At the end of the explanation, prompt your partner for feedback on the session for you to submit as a one-line sentiment:

```bash
laura feedback --positive "gradual-release layout made the two-pointer click"
laura feedback --negative "exposition moved too fast"
```
