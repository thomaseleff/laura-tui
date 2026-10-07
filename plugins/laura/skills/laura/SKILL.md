---
name: laura
description: Dynamically tile panes while you pair-program using `laura`, an API over the TUI.
when_to_use: You are running inside a Laura workspace, or a "[laura review · …]" block arrives in your input.
user-invocable: true
argument-hint: [path | prompt]
---

# laura

Laura is a TUI workspace that gives you an **API over the TUI**, so you can tile panes while you pair-program with your partner. Use this skill for the CLI reference, the common motions, and the workflow skills for longer sessions.

Pair-programming is two people at one workstation. The **driver** writes the code. The **navigator** reviews as the driver works and guides the strategic direction, such as edge cases, conventions and future problems. The navigator frees the driver to focus on the tactical task. You and your partner collaborate, learn, riff and improvise throughout.

Use different `laura` motions depending on who drives:

- **Agent drives** (default) — you are the driver, producing and editing the codebase, while your partner navigates and reviews. The `explain` skill defaults to this mode.
- **Agent navigates** — your partner is the driver, producing and editing, while you navigate and review. The `learn` skill defaults to this mode.

In either mode, you compose the workspace, run interactions in panes, and run code, tests and builds.

**Rules**
- **Run `laura ready --session <id> --agent <name>` at the start of every new chat.** `laura ready` enables commenting and inline reviews. Use your *own* conversation id for `--session`, so the journal matches this chat 1:1. If `laura ready` fails, tell your partner you're not running in a Laura workspace and stop. `laura ready` prints JSON with:
  - `journal`: the journal path.
  - `experimental`: the experimental features enabled in this workspace. Use only those.
  - `layout`: what's already open. `laura close` anything no longer relevant. If `layout` is `null`, run `laura layout`.
- **Run `laura layout`** before later opens or closes to see what's open, and `laura close` anything no longer relevant.
- **Prefer *fewer* panes.** Tile panes to maximize content and minimize clutter.
- **Proactively show a file, diff, or logs** in a pane instead of pasting the content into the chat or referring to it.
- **Let Laura tile.** With no split flags, Laura splits the newest pane and alternates the orientation (a dwindle), so you don't need to capture ids or pass `--split`. Pass `--dir`, `--ratio` or `--split` only for a custom layout. To swap a file into an existing pane, use `laura open <path> --panel <id>`, which replaces the pane's content in place.
- **On an `unsubmitted inline review` or `frozen` error**, ask your partner to submit (`Shift+S`) or refresh (`Ctrl+R`). **On an `unsaved edits` error**, ask your partner to save (`:w`) or quit Neovim in that pane. Don't retry either.
- **Annotate only when you need your partner's answer in a thread.** `laura comment` starts an unsubmitted inline review that only your partner can submit (`Shift+S`) or refresh (`Ctrl+R`). Until your partner does, you can't close the pane or replace its file. To explain something, highlight the lines with `laura highlight` and explain in the chat.
- **Laura holds `laura` commands while your partner is typing a comment or review body.** Don't kill or retry a command that is waiting.
- **Read the docs** at https://thomaseleff.github.io/laura-tui/llms.txt.

## Reference

Every `laura` command and its flags. Run `laura --help` (or `laura <cmd> --help`) for the full reference.

```bash
laura open <path>       Split a pane and render <path> in the new pane. Prints the new pane id.
                        Relative paths resolve against the *calling* process's cwd, so `cd`
                        in the shell then `laura open ./x` works. Warns on stderr (still exit 0)
                        when the file can't be read (`cannot read <path>: …`) or the pane doesn't
                        fit (`overflows:` / `too small`) — so `laura layout` isn't required just to
                        check fit, but still run it first to see what's open (see Rules).
      --split <id>      pane to split (default: the focused pane).
      --dir <h|v>       Split orientation: h side-by-side, v stacked (inferred).
      --ratio <1..99>   Percent of the split given to the new pane (inferred).
      --side <first|second>  Which side the new pane lands on (inferred).
      --no-focus        Don't move focus into the pane.
      --follow          Autoscroll: pin the cursor to the last line on open and every reload.
      --dry-run         Print the would-be overflow report; open nothing.
      --highlight <start> [end]  Highlight a line range once open (1-based, inclusive); end
                        defaults to start. Opens the pane already scrolled to and highlighting
                        the range: `laura open x.rs --highlight 40 52`.
      --diff            Open straight into the inline diff view (vs git HEAD).
      --panel <id>      Swap a pane's file in place, no new split — same id, rect, focus.
                        Ignores --split/--dir/--ratio/--side. Errors (exit 1) while the pane
                        has an unsubmitted inline review or unsaved Neovim edits.
      --edit            Experimental: run Neovim on <path> in an editor pane. Never takes
                        focus. With --panel on the same file, attaches in place and keeps
                        its threads. Errors (exit 1) with the reason when editor panes are
                        off, and with --highlight or --diff: editor panes show neither.
laura close [<id>]      Close a pane (default: the focused one). Errors (exit 1) while the
                        pane has an unsubmitted inline review or unsaved Neovim edits.
      --all             Close every pane, back to shell-only. Errors (exit 1), closing nothing,
                        while any pane has an unsubmitted inline review or unsaved Neovim edits.
laura focus <id>        Focus a pane by id.
laura highlight [start] [end]
                        Highlight lines start..=end (1-based, inclusive) in a pane and
                        scroll them into view. end defaults to start (single line). Line numbers
                        are the file's real source lines (an editor / wc -l / git blame), for
                        markdown too — a source line inside a hand-wrapped paragraph maps to
                        that whole block: `laura highlight 40 52`. Errors (exit 1) on a
                        frozen pane or an editor pane.
      --off             Clear the pane's highlight (start optional; works on a frozen pane;
                        your partner can also press `h`).
      --pane <id>       pane to highlight (default: the focused pane).
laura diff              Toggle a pane's inline diff view vs git HEAD (interleaved +/- lines).
                        Errors (exit 1) on an editor pane.
      --pane <id>       pane to toggle (default: the focused pane).
      --off             Turn the diff view off (default: toggle).
laura comment <line> <body>
                        Write to a line's thread: start a thread or reply on it.
                        <line> is a 1-based source line; <body> is required. A <line>
                        past the file's end errors (exit 1) and places no comment.
      --author <name>   Attribute the comment (default: the session's ready --agent name, else agent).
      --pane <id>       pane to comment on (default: the focused pane).
laura layout            Print the layout: per-pane rects + overflow (JSON).
laura ready             Mark the workspace as hosting an agent (enables commenting and inline reviews). Prints the journal path,
                        the experimental features on, and the layout (JSON).
      --session <id>    Name the journal session (default: laura-<pid>-<n>).
      --agent <name>    Attribute journal events to this agent name.
laura feedback          Append a feedback signal (layout/render quality, a missing tool) to the journal.
      --positive        Positive signal.        (one of --positive/--negative is required)
      --negative        Negative signal.
      [<body>]          Optional free-text body.
laura journal [FIELD=VALUE…]
                        Print journaled events across every session (NDJSON, oldest first).
                        Works outside a workspace. FIELD=VALUE matches a top-level field
                        (`type=feedback sentiment=-`); a repeated field ORs, different fields AND.
      --since <dur>     Only events newer than <dur>: 30m, 24h, 7d.
      -n <N>            Print the newest N matches (default 50); -n 0 prints only the count.
some-cmd | laura tail   Spool piped stdin to an internal file and show it in a tail pane.
                        The tail pane opens unfocused.
      --title <t>       Pane title (also names the spool file).
      --follow          Autoscroll to the newest line as output arrives.
      --split/--dir/--ratio  Same split controls as `open`.
```

`laura layout` and `laura open --dry-run` both print a JSON report with one entry per pane, its rect and overflow, so you can size a pane before opening it:

```json
{
  "area": {"x": 0, "y": 0, "width": 120, "height": 40},
  "panes": [
    {"id": 0, "kind": "pty",   "path": null,       "rect": {"x":0,"y":0,"width":48,"height":40},
     "content_rows": null, "visible_rows": 38, "overflow_rows": 0,  "clipped": false},
    {"id": 1, "kind": "panel", "path": "spec.md",  "rect": {"x":48,"y":0,"width":72,"height":40},
     "content_rows": 120, "visible_rows": 38, "overflow_rows": 82, "clipped": true}
  ]
}
```

When `overflow_rows` is above 0, or `clipped` is `true`, the content overflows the pane and your partner has to scroll to read the full content. Change `--dir` or `--ratio` until the content fits. `laura open` also warns on stderr when the content overflows (`pane #N overflows: …`), and when the pane is too small to show any of it (`pane #N too small to render …`).

## Motions

Each motion is a common sequence of `laura` commands, with when to use it.

### Show a file

**Motion**
1. Run `laura layout` to see what is currently open.
2. Run `laura close <id>` (or `laura close --all`) to close any pane that's no longer relevant. `laura close` returns an error instead of closing a pane with an unsubmitted inline review or unsaved Neovim edits.
3. Run `laura open <path>` to split a pane and render the file. Run it with `--dry-run` first to check the fit. Laura reloads the pane when the file changes.

**Use when** your partner asks to see a file, or when you want to put one on screen as part of collaborating, riffing, or improvising.

**Handle stderr warnings.** `laura open` and `laura tail` warn on stderr and still exit 0. Read the warning, and open the file again with a different layout when a different layout would fit better:

- **`overflows:`** — the content overflows the pane. For a large overflow, open the file stacked (`--dir v`) to give the pane more height.
- **`too small`** — the pane is too small to show any of the content. Raise `--ratio` or change `--dir`.
- **`cannot read <path>: …`** — Laura can't read the file. Check the path.
- **`diff markers unavailable`** — `git` isn't installed, so Laura shows no gutter markers or diff view.
- **`already open in pane #N`** — reuse pane N (`laura highlight --pane N`). Open a second pane of one file only for two distant sections. If the warning ends with `, which has an unsubmitted inline review`, use the new pane and leave pane N to your partner's inline review.

### Call and respond

**Motion**
1. Run `laura comment <line> "…"` to annotate a file open in a pane. Each comment starts a thread on the line or adds to an existing one: a call and response with your partner.
   - Your partner presses `c` to comment on the same line, then `Shift+S` to submit the inline review into the chat as a `[laura review · …]` block. Laura then clears the pane's threads.

**Use when** your partner asks you to review a file, or you need their answer in a thread, such as a question or a suggestion to accept or reject. Subagents can also annotate an open pane.

### Review

**Motion**
1. Run `laura open <path>` so your partner can comment on the file in place.
   - Your partner moves with `↑`/`↓`, jumps between threads with `n`/`N`, comments with `c`, collapses or expands threads with `r`, submits with `Shift+S`, refreshes with `Ctrl+R` (discarding the unsubmitted inline review), closes the pane with `x`, and returns to the chat with `Esc`.
   - The border's `[review: N ↑a ↓b]` counts the threads at or above (`a`) and below (`b`) your partner's cursor, so your partner knows about off-screen threads before scrolling.
   - After the file name, the border's `+N -M` counts the lines changed since git `HEAD`, and `● unsaved` marks an editor pane with unsaved Neovim edits. Point your partner to these to show what changed.
2. When your partner submits, you receive the inline review as a `[laura review · …]` block in the chat, and Laura clears the pane's threads. Each `L<n>` is a thread, and each comment starts with its author (`> [user] …`, `> [agent] …`).
3. Resolve any open questions with your partner first, then act on the inline review.
4. Respond to the inline review in the chat. Annotate again only for a follow-up you need answered in a thread. If you edit a file while your partner has an unsubmitted inline review on it, Laura freezes the pane until your partner submits (`Shift+S`; the inline review starts with a `⚠ file changed` banner) or refreshes (`Ctrl+R`).

**Use when** your partner asks to review a file, or when you want your partner to review something you just wrote or modified.

### Highlight

**Motion**
1. If the file is not open yet, run `laura open <path> --highlight <start> [end] --no-focus` to open *and* highlight in one call, leaving your partner's focus in chat.
2. If the file is open, run `laura highlight <start> [end] --pane <id>` to highlight a different section.
3. Run `laura highlight` again to move the highlight as the conversation moves on.
4. Run `laura highlight --off [--pane <id>]` to clear the highlight (or your partner presses `h` on the focused pane).
   - On a frozen pane, `laura highlight` returns an error, but `--off` still clears the highlight.

**Use when** you want to show a reference while you talk in the chat, or when your partner asks you to explain something step by step.

### Show what changed

**Motion**
1. If the file is not open yet, run `laura open <path> --diff` to open straight into the inline `+`/`-` diff against git `HEAD`.
2. If the file is open, run `laura diff --pane <id>` to turn on the pane's diff view (`--off` turns it off).

**Use when** you want to show *what* changed, not just *where*, or your partner asks to see the latest changes to a file.

Laura marks added, modified and deleted lines in every file pane's gutter. On a clean or untracked file, or without `git`, there is nothing to diff: `laura diff` returns an error, and `laura open --diff` warns on stderr.

### Edit a file together

**Motion**
1. Only if `laura ready` listed `editor` under `experimental`: run `laura open <path> --edit` to run Neovim on the file in an editor pane next to the chat. To turn a file pane you already opened into an editor pane, run `laura open <path> --panel <id> --edit`; its threads carry over.
2. Laura never focuses an editor pane, so tell your partner the editor pane is open. Your partner presses `Ctrl+P` then the pane id (shown on its border) to edit, and `Ctrl+P 0` to return to the chat.
3. Annotate an editor pane with `laura comment --pane <id>`, as on a file pane. An editor pane shows no highlight or diff, and `laura highlight` and `laura diff` return an error on it, so annotate the line you mean instead. While editing, your partner sees each comment as a `✎` diagnostic on its line and the thread count (`[review: N]`) on the border. After you annotate, tell your partner to press `Ctrl+L` to read your comments.
4. Your partner reviews an editor pane in its **file view**. Your partner presses `Ctrl+L` to flip the pane between the editor view and the file view, and uses the usual keys in the file view (`c` to comment, `Shift+S` to submit). The file view shows the saved file, so your partner saves first. When your partner submits, Laura flips the pane back to the editor view.
5. If `laura close` or `--panel` returns an error because the editor pane has unsaved edits, ask your partner to save (`:w`) or quit Neovim, and don't retry.
6. If `--edit` returns `editor panes are experimental: …` or `editor panes need Neovim: …`, open the file with plain `laura open` and don't retry. Mention `LAURA_EXPERIMENTAL_EDITOR=1` only if your partner asks about editing in Laura.

**Use when** your partner wants to edit a file themselves, or you want them to make a change by hand while you watch.

### Tail live output

**Motion**
1. Pipe a running command into `laura tail` to open a tail pane that follows the newest output: `some-cmd | laura tail --title logs --follow --split <id> --dir v --ratio 30` opens the tail pane below pane `<id>`, at 30% of the height.

**Use when** you are running or debugging a service and want to show the logs for your partner to watch live, or when your partner asks you to start or monitor a service.

## Workflows

For longer sessions, use these `laura` skills, which chain motions into a workflow with your partner:

- **`demo`**: give your partner a guided, live tour of Laura: the inline review loop, panes, tailing, and feedback.
- **`explain`**: walk your partner through a PR, file or flow, one highlighted range at a time, on your partner's cue.
- **`learn`**: teach a concept hands-on. Explain the idea step by step, have your partner produce something, then review it in place and revise.
- **`retro`**: read back the feedback and inline reviews from past sessions with `laura journal`, and summarize how sessions have gone over time.
