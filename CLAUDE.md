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

The repository uses squash merge only. GitHub merges each PR into `main` as one commit. The PR title becomes the subject of that commit. The PR body becomes the message of that commit.

- **Commits** on a branch: Put the issue number in the subject, for example `Horizontal scroll for pre-formatted content (#25)`. This links the commit to the issue. It does not close the issue.
- **PR title:** Write the title as the subject of the squash commit.
  - Write one sentence that tells what changes for the user, for example `Esc turns off comment mode from any pane but a terminal`.
  - Keep the title to 64 characters or fewer. GitHub adds the PR number to the end of the subject, for example ` (#131)`.
  - Do not put the issue number in the title. Do not use a prefix such as `feat:` or `fix:`. Do not put a period at the end.
  - Do not use a title that tells about the work, such as `Address review feedback` or `Fix #130`.
  - If the scope of the PR changes, change the title with `gh pr edit <n> --title "…"`.
- **PR body:** Write the body as the message of the squash commit.
  - Close the issue with a keyword in the body: `Closes #25` (or `Fixes` / `Resolves`). GitHub closes the issue when it merges the PR. Do not close the issue manually.
  - Tell what changed and why. Do not copy the list of the branch commits into the body.
  - If you push commits that change the result, update the body with `gh pr edit <n> --body-file <file>`.
- **Merge:** Use `gh pr merge <n> --squash`. GitHub does not allow a merge commit or a rebase merge.

## Testing

**Integration tests only — exercise Laura as a user (or the agent) actually would.** No per-function unit test suites.

- Tests live in `tests/`, drive the real public surface: the `laura` CLI binary and the per-tab socket protocol. Use `assert_cmd` + `tempfile` to run the binary and inspect its effects.
- Each test maps to a workstream's **Done when**, not to a function. "Feed an `open` message → pane state holds the file's content" — through the protocol, not by calling an internal fn.
- The TUI render loop can't be driven interactively in CI: assert on **state** (app state after a message, pane content, socket round-trip) rather than pixels.
- **The one exception** — pure logic a user can't reach through the CLI (DSR handshake reply from a byte stream, review-payload assembly, NDJSON framing). Test it through the smallest public entry point that reaches it; only if there's genuinely no such surface, a `#[cfg(test)]` check next to the code. Prefer exposing the seam over reaching into privates.

Run before calling anything done: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`.

**Manual runs.** `tmp/` is gitignored scratch for manual checks. Build a binary to run by hand with `CARGO_TARGET_DIR=tmp/target cargo build`, so a Laura running from `target/` doesn't lock the build.

## Implementation requirements

Implementation should always be comprehensive of code, tests, docs and skills.

- **Code** — the feature itself.
- **Tests** — integration coverage per the workstream's *Done when* (see Testing).
- **Docs** — `README.md` + `docs/` reflect the new verbs, flags, protocol messages, keybindings.
- **Skills** — `plugins/laura/skills/` teach the agent the new surface. Revise `skills/demo/` when the demo must show a new user-facing capability. Record a demo (`/record-demo`) only when the user asks. A change to `skills/demo/` is not a request to record the README demo. The user asks for the video after the feature works.

New CLI verb, protocol message, keybinding, or journal event field → assume all four apply and say so if you're skipping one. Docs and skills drift silently otherwise: nothing fails to compile when they fall behind.

## Writing

For prose in `README.md`, `docs/`, and skills.

- **Name the actor.** Laura, the user, or the agent is the subject of each sentence. A pane, keystroke, file, command, gerund phrase or page doesn't act.
- **Use plain, literal verbs.** Say what happens ("returns an error", "shows a notice"), never "refused" or a figurative verb.
- **Repeat the noun** instead of "it" when the reader could lose track of the subject.
- **Write complete sentences** in a calm, direct voice. Use the imperative for instructions.
- **Write for someone new to Laura.** Describe what the user sees, with no internals or jargon.
- **Lead with what it is and what to do, then how.** Use numbered steps for setup.
- **One idea per paragraph.** Split a dense paragraph into a list.
- **Document each fact in one place.** Elsewhere, link to that place instead of repeating the fact.
- **Use the glossary terms exactly.** When citing an error or notice, copy its exact text.
- **Experimental means early, not unsafe.** Describe an experimental feature as a minimal implementation of a pattern that's still evolving, not as a warning.

## Glossary

The following glossary defines the vocabulary for Laura's components and interactions.

| Use | Meaning | Never |
|---|---|---|
| **pane** | A tile in the tab's split tree. | tile (noun), window |
| **pane header** | The top bar of a pane in the web app, with the pane's name, state and buttons. | title bar, toolbar |
| **pane selector** | `Ctrl+P` in the web app, which finds a pane by file name or pane id. | popup, palette |
| **shell** | A terminal running the shell or agent CLI, pane `0`. | PTY, PTY pane, shell pane |
| **chat** | An agent's conversation, running in the shell. Use for conversation content; use *shell* for the pane. | — |
| **file pane** | A pane rendering a file (`open`, `diff`). | panel, doc pane |
| **tail pane** | A file pane opened by `laura tail`. | log pane |
| **browser pane** | A pane showing a page from a URL through Laura's browser (`open <url>`, experimental). | web pane, browser tab, preview |
| **Laura's browser** | The headless Chrome or Edge that Laura starts for browser panes. | the pane browser, Chromium (unless naming Chromium itself) |
| **live badge** | The ● live mark in a browser pane's pane header, which shows the lag in ms when the stream falls behind. | latency badge, fps counter, cast badge |
| **editor pane** | A pane where the user edits a file in an editor, over its file pane (`open --edit`, experimental). | editor (alone), nvim pane, vim pane |
| **editor view** | An editor pane showing the editor, which takes the keystrokes (the default). | Neovim view, nvim mode |
| **file view** | An editor pane showing its file pane, flipped with `Ctrl+L`. | review view, review mode |
| **Neovim** | The editor that editor panes run today. Name it only for Neovim's own behavior (`:w`, its config, diagnostics, its file-changed prompt). | nvim (in prose) |
| **comment** (n., v.) | A single inline annotation on a pane, or an action the user takes (`c`). | note, annotation, mark up, in-line / in line comment |
| **annotate** (v.) | An action the agent takes when running `laura comment`. | note, annotation, mark up, in-line / in line comment |
| **comment mode** | On an HTML page or in a browser pane in the web app, a click picks an element to comment on. | review mode, pick mode |
| **thread** | A set of inline comments on a single line of a pane. | comment thread, inline thread |
| **review body** | The text typed after `Shift+S` providing overall direction in the review. | overall comment, overall note |
| **inline review** | A pane's threads plus the review body, submitted into the chat as one `[laura review · pane #<id> · <path>]` block. | review (noun), inline file review |
| **review** (v.) | The action a user takes to provide inline feedback in a pane. | — |
| **submit** | Write the inline review into the chat (`Shift+S`). | ship, send, inject, land |
| **unsubmitted inline review** | An in-progress inline review on a pane. | unsubmitted comments, pending comments, open comments |
| **refresh** | To discard a pane's unsubmitted inline review and reload the file contents (`Ctrl+R`). | clear, reset |
| **reload** | A file pane re-reads the file contents when it changes on disk. | re-render, auto-update, catch up, live-watched |
| **frozen** / **freeze** | A file pane whose file changed on disk while it has an unsubmitted inline review. | paused, stale |
| **pause** / **paused** | A tail pane that the user stopped from reloading (`p`). | freeze, hold |
| **snapshot** | The content of a pane with an unsubmitted inline review. | reviewed content |
| **highlight** (n., v.) | Lines marked by `laura highlight`. | spotlight |
| **theme** | Laura's colors and gutter in editor panes, chosen in the startup wizard. | look |
| **workspace** | The TUI or the web app, including all panes. | — |
| **web app** | The browser view of a workspace (`laura --web`). | web UI, frontend |
| **notice** | A message in the bottom-right corner of a pane, or of the workspace above the hint line. | toast, standing warning |
| **hint line** | A bottom line in the workspace listing the keystrokes for the current context. | footer |
| **status bar** | The list on the right of the web app's bottom line of what needs the user's attention. | footer |
| **unseen marker** | A dot before a pane's name in the web app when the pane changed while the user worked in another pane. | notice, badge, unread dot |
| **keystroke** | A key or key combination the user presses (`c`, `Ctrl+L`, `]d`). | key |
| **overflow** (n., v.) | Content taller than its pane, which the user scrolls to read in full. | clipped, below the fold, doesn't fit |
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

## Entire

<!-- entire-agent:begin -->
Read .entire/agent-guide.md for this repository's workflow, source inspection, and verification guidance.
@.entire/agent-guide.md
<!-- entire-agent:end -->
