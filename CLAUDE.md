# Laura

See [README.md](README.md) for what Laura is and how to run it, and [docs/](docs/) for the protocol, CLI, and design rationale.

## Coding

Standard Rust. No house style on top of it.

- **Format:** `cargo fmt` — default rustfmt, no custom `rustfmt.toml` unless a real need appears.
- **Lint:** `cargo clippy --all-targets -- -D warnings` must pass. Fix lints, don't `#[allow]` them without a one-line reason on the attribute.
- **Edition:** latest stable (2024). Toolchain pinned in `rust-toolchain.toml`.
- **Errors:** `anyhow` at binary boundaries, `thiserror` for typed library errors. No `.unwrap()`/`.expect()` in code that handles runtime input (PTY bytes, socket messages, files) — reserve them for invariants that can't fail, with the reason in the `.expect()` string.
- **Naming/layout:** stock Rust conventions (snake_case, `mod`s per concern). Don't invent abstractions ahead of the second caller.
- **Comments/docstrings:** terse; explain *why*, not *what*. Match the surrounding file's density.

## Commits & PRs

- **Commits** reference the issue by number in the subject: `Horizontal scroll for pre-formatted content (#25)`. This links the commit to the issue without closing it — a commit is a step, not the resolution.
- **PRs** close the issue with a keyword in the body: `Closes #25` (or `Fixes` / `Resolves`). GitHub auto-closes the issue on merge, so don't close it by hand.

## Testing

**Integration tests only — exercise Laura as a user (or the agent) actually would.** No per-function unit test suites.

- Tests live in `tests/`, drive the real public surface: the `laura` CLI binary and the per-tab socket protocol. Use `assert_cmd` + `tempfile` to run the binary and inspect its effects.
- Each test maps to a workstream's **Done when**, not to a function. "Feed an `open` message → pane state holds the file's content" — through the protocol, not by calling an internal fn.
- The TUI render loop can't be driven interactively in CI: assert on **state** (app state after a message, pane content, socket round-trip) rather than pixels.
- **The one exception** — pure logic a user can't reach through the CLI (DSR handshake reply from a byte stream, review-payload assembly, NDJSON framing). Test it through the smallest public entry point that reaches it; only if there's genuinely no such surface, a `#[cfg(test)]` check next to the code. Prefer exposing the seam over reaching into privates.

Run before calling anything done: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`.

## Implementation requirements

Implementation should always be comprehensive of code, tests, docs and skills.

- **Code** — the feature itself.
- **Tests** — integration coverage per the workstream's *Done when* (see Testing).
- **Docs** — `README.md` + `docs/` reflect the new verbs, flags, protocol messages, keybindings.
- **Skills** — `plugins/laura/skills/` teach the agent the new surface; revise `skills/demo/` when a user-facing capability is worth demoing.

New CLI verb, protocol message, or keybinding → assume all four apply and say so if you're skipping one. Docs and skills drift silently otherwise: nothing fails to compile when they fall behind.

## Glossary

The following glossary defines the vocabulary for Laura's components and interactions.

| Use | Meaning | Never |
|---|---|---|
| **pane** | A tile in the tab's split tree. | tile (noun), window |
| **shell** | A terminal running the shell or agent CLI, pane `0`. | PTY, PTY pane, shell pane |
| **chat** | An agent's conversation, running in the shell. Use for conversation content; use *shell* for the pane. | — |
| **file pane** | A pane rendering a file (`open`, `diff`). | panel, doc pane |
| **tail pane** | A file pane opened by `laura tail`. | log pane |
| **comment** (n., v.) | A single inline annotation on a pane, or an action the user takes (`c`). | note, annotation, mark up, in-line / in line comment |
| **annotate** (v.) | An action the agent takes when running `laura comment`. | note, annotation, mark up, in-line / in line comment |
| **thread** | A set of inline comments on a single line of a pane. | comment thread, inline thread |
| **review body** | The text typed after `Shift+S` providing overall direction in the review. | overall comment, overall note |
| **inline review** | A pane's threads plus the review body, submitted into the chat as one `[laura review · <path>]` block. | review (noun), inline file review |
| **review** (v.) | The action a user takes to provide inline feedback in a pane. | — |
| **submit** | Write the inline review into the chat (`Shift+S`). | ship, send, inject, land |
| **unsubmitted inline review** | An in-progress inline review on a pane. | unsubmitted comments, pending comments, open comments |
| **refresh** | To discard a pane's unsubmitted inline review and reload the file contents (`Ctrl+R`). | clear, reset |
| **reload** | A file pane re-reads the file contents when it changes on disk. | re-render, auto-update, catch up, live-watched |
| **frozen** / **freeze** | A file pane whose file changed on disk while it has an unsubmitted inline review. | paused, stale |
| **snapshot** | The content of a pane with an unsubmitted inline review. | reviewed content |
| **highlight** (n., v.) | Lines marked by `laura highlight`. | spotlight |
| **workspace** | The TUI, including all panes. | — |
| **notice** | A one-line message in the bottom-right corner of the workspace, above the hint line. | toast, standing warning |
| **hint line** | A bottom line in the workspace listing the keys for the current context. | footer |
| **the user** (`docs/`, README) / **your partner** (skills) | The developer / student. | human, developer, learner, pair-programmer, reader |

## Plans

- **Revision** at the top (`Revision N`); bump it on every revision.
- **Organize by commit, not by artifact.** Each commit carries its own code, tests, docs, and
  skills, passes the check suite on its own, and can be reviewed in isolation.
- **Reference note.** Put this under the title word for word, filling in `{branch}` and
  `{commit}`:

  > [!NOTE]
  > This plan was drafted against `{branch}` at `{commit}`. Code snippets are example implementations and line
  > numbers are references only; explore the current code and write an implementation plan for
  > your task before editing code.

- **Conventions section.** `file:line` pointers to the codebase patterns the implementer must
  follow, each with a one-line description.
