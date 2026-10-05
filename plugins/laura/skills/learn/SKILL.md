---
name: learn
description: Create a lesson and teach your partner a concept, algorithm, or codebase live — expose it step by step, have them produce something, review their work in place, and revise.
when_to_use: You are running inside a Laura workspace and your partner wants to learn something hands-on — "learn two-sum", "teach me recursion", "learn socratic borrow checker".
user-invocable: true
argument-hint: [style] [difficulty] [prompt]
---

# laura learn

Run a lesson over real files in panes: teach, have your partner produce something, review it, and revise. Your partner sets the pace.

In a lesson, you **navigate**. Your partner is the driver and produces and edits the code. You point out lines, review your partner's work in place, and guide the revisions.

All arguments are positional and optional: a **style** (`socratic` / `montessori` / `direct` / `gradual`), a **difficulty** (`beginner` / `intermediate` / `advanced` / `expert`), then the **prompt**. For example: `learn two-sum`, `learn socratic recursion`, `learn direct beginner 101 crash course on Rust`. Infer any omitted argument. An explicit argument always wins.

**Rules**
- **Run `laura ready --session "$YOUR_SESSION_ID" --agent <name>`** at the start. The lesson requires `laura ready`, so `retro` can measure this session. Use your *own* conversation id for `--session`, so the journal matches this chat 1:1. If `laura ready` fails, tell your partner you're not running in a Laura workspace and stop.
- **Your partner sets the pace.** After each step, wait for `Next`. If your partner asks a question, answer it, stay on the step, and continue on `Next`.
- **Journal what you noticed** as you go (see below).
- **Let Laura tile.** With no split flags, Laura splits the newest pane and alternates the orientation (a dwindle), so you don't need to pass `--split`.
- **See the `laura` skill** for the full CLI or run `laura --help`.
- **Read the docs** at https://thomaseleff.github.io/laura-tui/llms.txt.

## Understand the subject and your partner first

Before composing a lesson:

1. **Understand the subject.** If the subject is code, read the actual algorithm and files.
2. **Gauge your partner.** Ask questions like "Are you familiar with X?", "Have you used pointers before?", "Do you want the intuition or the proof?". Treat the `difficulty` argument as a starting point, and adjust as your partner answers.

## The four-phase loop

- **Exposition** — teach in ordered steps. Open a markdown pane, and highlight the lines you're explaining with `laura highlight`. Close the previous pane with `laura close` as you open the next one with `laura open`. If `laura close` returns an error because an editor pane has unsaved edits, ask your partner to save (`:w`) or quit Neovim, and don't retry.
- **Produce** — write a stub file for your partner to complete, then open the stub:
  - If `laura ready` listed `editor` under `experimental`, run `laura open <stub> --edit` to open an editor pane next to the chat. Laura never focuses an editor pane, so tell your partner to press `Ctrl+P` then the pane id to edit, and `Ctrl+P 0` to return to the chat.
  - Otherwise, or if `--edit` returns an error, run plain `laura open <stub>` and don't retry. Your partner edits the file in the shell (`vim`, `code .`), and Laura reloads the pane on save.
- **Review** — review your partner's work in **one chat message that references `file:line`**. As you walk through each point, highlight that line with `laura highlight <line> --pane <id>`. An editor pane shows no highlight or diff, and `laura highlight` returns an error on it, so annotate the lines with `laura comment --pane <id>` instead. Then tell your partner to press `Ctrl+L` to read and reply in the file view.
- **Revise** — your partner edits again, Laura reloads the pane, and you loop back as needed. In an editor pane, the whole loop stays in one pane: when your partner submits, Laura flips the pane back to the editor view for the next revision.

## Teaching style

A style is a way of teaching. People learn in different ways, so each style shapes the lesson differently. Use the style your partner asked for, or infer one from the request ("teach me socratically") or from the topic.

- **Direct / explicit** — you state the concept, demonstrate it, then have your partner apply it. Lead with the answer and the *why* before practice.
- **Socratic** — you teach by asking. Pose a question, let your partner reason, and let your partner's answer steer the next question, so you *draw out* the concept rather than hand it over.
- **Montessori** — you prepare an environment and let your partner discover the concept by fixing something visibly wrong. Name the concepts afterward.
- **Gradual release** ("I do, we do, you do") — you shift ownership in stages: model the solution fully, solve one together, then hand it off entirely.

## Close the loop — journal what you noticed

When something notable happens (a layout reads badly, a tool is missing, a step clearly works), log it yourself with `laura feedback --positive "<one line>"` or `laura feedback --negative "<one line>"`. Never ask your partner for feedback. If your partner offers some ("log that as feedback"), record it.

```bash
laura feedback --positive "gradual-release layout made the two-pointer click"
laura feedback --negative "exposition moved too fast"
```
