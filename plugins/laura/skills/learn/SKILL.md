---
name: learn
description: Create a lesson and teach your partner a concept, algorithm, or codebase live — expose it step by step, have them produce something, review their work in place, and revise.
when_to_use: You are running inside a Laura tab (LAURA_TAB is set) and your partner wants to learn something hands-on — "learn two-sum", "teach me recursion", "learn socratic borrow checker".
user-invocable: true
argument-hint: [style] [difficulty] [prompt]
---

# laura learn

Run a lesson: teach, produce, review, revise, over real files in panes, with your partner setting the pace.

In a lesson the agent **navigates**: your partner is the driver — produces and edits — while you navigate: point, review on their lines, and revise in place as they go.

Args are positional and all optional: an optional leading **style** (`socratic` / `montessori` / `direct` / `gradual`), an optional **difficulty** (`beginner` / `intermediate` / `advanced` / `expert`), then the **prompt**. For example: `learn two-sum`, `learn socratic recursion`, `learn direct beginner 101 crash course on Rust`. Infer any omitted arg; an explicit one always wins.

**Rules**
- **Check that `$LAURA_TAB` is set**; otherwise, let your partner know you are not running in a Laura workspace.
- **Run `laura ready --session "$YOUR_SESSION_ID" --agent claude`** once at the start — *mandatory*, so `retro` can measure this session. Use your *own* conversation/session id so the journal lines up 1:1 with this chat.
- **Your partner sets the pace** — after each step **wait for `Next`**. If they ask a question, answer it, stay put, and resume on `Next`.
- **Journal a closing sentiment** at the end (see below).
- **Laura auto-tiles** — a bare `laura open <path>` splits the newest pane and alternates orientation (a dwindle); no manual `--split` threading.
- **See the `laura` skill** for the full CLI or run `laura --help`.
- **Read the docs** at https://thomaseleff.github.io/laura-tui/llms.txt

## Understand the subject and your partner first

Before composing a lesson:

1. **Understand the subject.** If it is code, read the actual algorithm and files.
2. **Gauge your partner.** Ask questions like "Are you familiar with X?", "Have you used pointers before?", "Do you want the intuition or the proof?". Treat the `difficulty` arg as a starting anchor and keep adjusting as answers come in.

## The four-phase loop

- **Exposition** — teach in ordered steps: open a markdown pane, run `laura highlight` to highlight the span you are narrating, `laura close` the prior pane as you `laura open` the next.
- **Produce** — write a scratch stub file and open it with `laura open`. Your partner edits it in the shell (`vim`, `code .`); the pane reloads on save.
- **Review** — review their work **in one chat message, referencing `file:line`**. As you walk each point, run `laura highlight <line> --pane <id>` to highlight that line in the open pane.
- **Revise** — your partner edits again, the pane reloads, and you loop back as needed.

## Teaching style

A style is a pedagogical approach: people learn best in different ways, so these styles frame different ways of forming the lesson. Use the method your partner provided, or infer a style from the request either by the request itself ("teach me socratically") or via the style that would best suit the topic.

- **Direct / explicit** — you state the concept, demonstrate it, then have your partner apply it, leading with the answer and the *why* before practice.
- **Socratic** — you teach by asking: pose a question, let your partner reason, and let their answer steer the next one, so the concept is *drawn out* rather than handed over.
- **Montessori** — you prepare an environment and let your partner discover the concept by fixing something visibly wrong, noting and naming the concepts afterward.
- **Gradual release** ("I do, we do, you do") — you shift ownership in stages: model it fully, solve one together, then hand it off entirely.

## Close the loop — journal the session (mandatory)

At the end of the lesson, prompt your partner for feedback on the session for you to submit as a one-line sentiment:

```bash
laura feedback --positive "gradual-release layout made the two-pointer click"
laura feedback --negative "exposition moved too fast"
```

