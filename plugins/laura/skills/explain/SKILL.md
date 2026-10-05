---
name: explain
description: Walk your partner through a PR, a file, or how something works, stepping across ranges of one or more panes one range at a time and advancing on their cue.
when_to_use: You are running inside a Laura workspace and your partner wants a guided walkthrough — "explain this PR", "walk me through the render loop", "how does X work".
user-invocable: true
argument-hint: [target]
---

# laura explain

Run a guided walkthrough: open the relevant files in panes, step through them one range at a time, highlight the lines you're describing with `laura highlight`, and explain them in the chat.

In a walkthrough, you **drive**. You open files, highlight lines and explain, while your partner navigates, reviewing and reacting as you go.

**Rules**
- **Run `laura ready --session "$YOUR_SESSION_ID" --agent <name>`** at the start. Use your *own* conversation id for `--session`, so the journal matches this chat 1:1. If `laura ready` fails, tell your partner you're not running in a Laura workspace and stop.
- **After each step, wait for `Next`.** If your partner asks a question mid-step, answer it, stay on the step, and continue on `Next`.
- **End with `laura close --all`.**
- **Let Laura tile.** With no split flags, Laura splits the newest pane and alternates the orientation (a dwindle), so you don't need to pass `--split`.
- **See the `laura` skill** for the full CLI or run `laura --help`.
- **Read the docs** at https://thomaseleff.github.io/laura-tui/llms.txt.

## Understand the target first, then plan

Before beginning the walkthrough:

1. **Understand what and why.** Ask your partner clarifying questions to learn what to explain and why.
2. **Explore the target yourself.** Read the diff (`git diff <base>...HEAD`), open the files, and trace the call sites until you understand the target.
3. **Plan the sequence** of ranges you will step through.

## The loop

Compose the walkthrough from these commands, in whatever order the explanation calls for:

- `laura open <path>` opens a file in a pane and prints the new pane id. Capture the id to target that pane later, and open one pane per file when the walkthrough spans several files.
- `laura open <path> --highlight <a> <b>` opens a file already scrolled to and highlighting lines `a`–`b`.
- `laura highlight <a> <b> --pane <id>` highlights lines `a`–`b` in an open pane.
- `<some-cmd> | laura tail --follow --split <id> --dir v --ratio 30` opens a tail pane below pane `<id>`, when the explanation needs live output next to the code.

A step may highlight one range, move across several ranges of the same file, jump between files, or open a pane with nothing highlighted. For each step, run the commands that set up what your partner should see, then explain the step in the chat.

Annotate with `laura comment` only when you need your partner's answer in a thread, for example "which of these two approaches do you prefer?". Until your partner submits or refreshes, you can't close the pane.

## Close the loop — journal what you noticed

When something notable happens (a layout reads badly, a tool is missing, a step clearly works), log it yourself with `laura feedback --positive "<one line>"` or `laura feedback --negative "<one line>"`. Never ask your partner for feedback. If your partner offers some ("log that as feedback"), record it.

```bash
laura feedback --positive "one file per step made the render loop easy to follow"
laura feedback --negative "highlighted ranges were too long to follow"
```
