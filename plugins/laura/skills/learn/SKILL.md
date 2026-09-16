---
name: learn
description: Create a lesson and teach the user a concept, algorithm, or codebase live — expose it step by step, have them produce something, review their work in place, and revise.
when_to_use: You are running inside a Laura tab (LAURA_TAB is set) and the user wants to learn something hands-on — "learn two-sum", "teach me recursion", "learn socratic borrow checker".
user-invocable: true
argument-hint: [style] [difficulty] [prompt]
---

# laura learn

Run a lesson: teach, produce, review, revise, over real files in panes, with the learner setting the pace.

Args are positional and all optional: an optional leading **style** (`socratic` / `montessori` / `direct` / `gradual`), an optional **difficulty** (`beginner` / `intermediate` / `advanced` / `expert`), then the **prompt**. For example: `learn two-sum`, `learn socratic recursion`, `learn direct beginner 101 crash course on Rust`. Infer any omitted arg; an explicit one always wins.

**Rules**
- Check that `$LAURA_TAB` is set, otherwise, let the user know you are not running in a Laura workspace.
- Run `laura ready --session "$YOUR_SESSION_ID" --agent claude` once at the start — **mandatory**, so `retro` can measure this session. Use your **own** conversation/session id so the journal lines up 1:1 with this chat.
- The learner sets the pace: after each step **wait for `Next`**. If they ask a question, answer it, stay put, and resume on `Next`.
- Journal a closing sentiment at the end (see below).
- **Just `laura open <path>` — Laura dwindles for you** (splits the newest pane, alternates orientation); no manual `--split` threading.
- See the `laura` skill for the full CLI or run `laura --help`.
- Read the docs: https://thomaseleff.github.io/laura-tui/llms.txt

## Understand the subject and the learner first

Before composing a lesson:

1. **Understand the subject.** If it is code, read the actual algorithm and files.
2. **Gauge the learner.** Ask questions like "Are you familiar with X?", "Have you used pointers before?", "Do you want the intuition or the proof?". Treat the `difficulty` arg as a starting anchor and keep adjusting as answers come in.

## The four-phase loop

- **Exposition** — teach in ordered steps: open a markdown pane, run `laura highlight` to highlight the span you are narrating, `laura close` the prior pane as you `laura open` the next.
- **Produce** — write a scratch stub file and open it with `laura open`. The learner edits it in the shell (`vim`, `code .`); the pane reloads on save.
- **Review** — deliver **one complete review in chat, referencing `file:line`**. As you walk each point, run `laura highlight <line> --pane <id>` to highlight that line in the open pane.
- **Revise** — the learner edits again, the pane reloads, and you loop back as needed.

## Teaching style

A style is a pedagogical approach: learners take a concept best in different ways, so these styles frame different ways of forming the lesson. Use the method the user provided, or, infer a style from the request either by the request itself ("teach me socratically") or via the style that would best suite the topic.

- **Direct / explicit** — you state the concept, demonstrate it, then have the learner apply it, leading with the answer and the *why* before practice.
- **Socratic** — you teach by asking: pose a question, let the learner reason, and let their answer steer the next one, so the concept is *drawn out* rather than handed over.
- **Montessori** — you prepare an environment and let the learner discover the concept by fixing something visibly wrong, noting and naming the concepts afterward.
- **Gradual release** ("I do, we do, you do") — you shift ownership in stages: model it fully, solve one together, then hand it off entirely.

## Close the loop — journal the session (mandatory)

At the end of the lesson, prompt the learner for feedback on the session for you to submit as a one-line sentiment:

```bash
laura feedback --positive "gradual-release layout made the two-pointer click"
laura feedback --negative "exposition moved too fast"
```

