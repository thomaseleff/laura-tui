---
name: learn
description: Create a lesson and teach your partner a concept, algorithm, or codebase live — expose it step by step, have them produce something, review their work in place, and revise.
when_to_use: You are running inside a Laura workspace and your partner wants to learn something hands-on — "learn two-sum", "teach me recursion", "learn socratic borrow checker".
user-invocable: true
argument-hint: [style] [difficulty] [prompt]
---

# laura learn

Run a lesson: teach, produce, review, revise, over real files in panes, with your partner setting the pace.

In a lesson the agent **navigates**: your partner is the driver — produces and edits — while you navigate: point, review on their lines, and revise in place as they go.

Args are positional and all optional: an optional leading **style** (`socratic` / `montessori` / `direct` / `gradual`), an optional **difficulty** (`beginner` / `intermediate` / `advanced` / `expert`), then the **prompt**. For example: `learn two-sum`, `learn socratic recursion`, `learn direct beginner 101 crash course on Rust`. Infer any omitted arg; an explicit one always wins.

**Rules**
- **Run `laura ready --session "$YOUR_SESSION_ID" --agent claude`** once at the start — *mandatory*, so `retro` can measure this session. If it fails, tell your partner you're not running in a Laura workspace and stop. Use your *own* conversation/session id so the journal lines up 1:1 with this chat.
- **Your partner sets the pace** — after each step **wait for `Next`**. If they ask a question, answer it, stay put, and resume on `Next`.
- **Journal what you noticed** as you go (see below).
- **Laura auto-tiles** — a bare `laura open <path>` splits the newest pane and alternates orientation (a dwindle); no manual `--split` threading.
- **See the `laura` skill** for the full CLI or run `laura --help`.
- **Read the docs** at https://thomaseleff.github.io/laura-tui/llms.txt

## Understand the subject and your partner first

Before composing a lesson:

1. **Understand the subject.** If it is code, read the actual algorithm and files.
2. **Gauge your partner.** Ask questions like "Are you familiar with X?", "Have you used pointers before?", "Do you want the intuition or the proof?". Treat the `difficulty` arg as a starting anchor and keep adjusting as answers come in.

## The four-phase loop

- **Exposition** — teach in ordered steps: open a markdown pane, run `laura highlight` to highlight the span you are narrating, `laura close` the prior pane as you `laura open` the next. If a close is refused because an editor pane has unsaved edits, ask your partner to save (`:w`) or quit Neovim, and don't retry.
- **Produce** — write a scratch stub file and, if `laura ready` listed `editor` under `experimental`, open it with `laura open <stub> --edit`, an editor pane running Neovim next to the chat. It doesn't take focus: tell your partner to press `Ctrl+P` and its id to edit, and `Ctrl+P 0` to come back. Otherwise, or if `--edit` is refused, open it with plain `laura open` instead and don't retry; your partner edits it in the shell (`vim`, `code .`) and the pane reloads on save.
- **Review** — review their work **in one chat message, referencing `file:line`**. As you walk each point, run `laura highlight <line> --pane <id>` to highlight that line in the open pane. An editor pane shows no highlight or diff (`laura highlight` and `laura diff` are refused), so annotate lines with `laura comment --pane <id>` instead, and tell your partner to press `Ctrl+L` to read and reply in the file view.
- **Revise** — your partner edits again, the pane reloads, and you loop back as needed. In an editor pane the whole loop stays in one pane: submitting their replies flips it back to Neovim to revise.

## Teaching style

A style is a pedagogical approach: people learn best in different ways, so these styles frame different ways of forming the lesson. Use the method your partner provided, or infer a style from the request either by the request itself ("teach me socratically") or via the style that would best suit the topic.

- **Direct / explicit** — you state the concept, demonstrate it, then have your partner apply it, leading with the answer and the *why* before practice.
- **Socratic** — you teach by asking: pose a question, let your partner reason, and let their answer steer the next one, so the concept is *drawn out* rather than handed over.
- **Montessori** — you prepare an environment and let your partner discover the concept by fixing something visibly wrong, noting and naming the concepts afterward.
- **Gradual release** ("I do, we do, you do") — you shift ownership in stages: model it fully, solve one together, then hand it off entirely.

## Close the loop — journal what you noticed

When something notable happened — a layout read badly, a tool was missing, a step clearly
landed — log it yourself with `laura feedback --positive/--negative "<one line>"`. Never ask your
partner for feedback; if they offer some ("log that as feedback"), record it.

```bash
laura feedback --positive "gradual-release layout made the two-pointer click"
laura feedback --negative "exposition moved too fast"
```

