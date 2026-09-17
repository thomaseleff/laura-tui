---
name: laura
description: Dynamically tile panes while you pair-program using `laura`, an API over the TUI.
when_to_use: You are running inside a Laura tab (LAURA_TAB is set), or a "[laura review · …]" block arrives in your input.
user-invocable: true
argument-hint: [path | prompt]
---

# laura

Laura is a TUI workspace that provides you with an **API over the TUI**, allowing you to dynamically tile panes while pair-programming. This skill gives you the CLI reference, the common motions, and a skills reference for longer-running workflows.

Pair-programming is two people at one workstation: a **driver** writing code while a **navigator** reviews iteratively and guides the strategic direction — edge cases, conventions, future problems — freeing the driver to focus on the tactical task. In pair-programming you and your partner collaborate, learn, riff, and improvise continuously.

Who drives during pair-programming determines which `laura` motions you reach for:

- **Agent drives** (default) — you are the driver, producing and editing the codebase, while your partner navigates and reviews. The `explain` skill defaults to this mode.
- **Agent navigates** — your partner is the driver, producing and editing, while you navigate and review. The `learn` skill defaults to this mode.

Regardless of either mode, you compose the TUI workspace, run interactions within panes, and run code, tests, and builds. 

**Rules**
- **Check that `$LAURA_TAB` is set**, otherwise, let the user know you are not running in a Laura workspace.
- **Run `laura ready --session <id> --agent <name>`** first so pane interactions like review submission are enabled, and use your *own* conversation id for `--session` so the journal lines up 1:1 with this chat.
- **Run `laura layout`** before opening or closing panes, see what's open and `laura close` anything stale.
- **Prefer *fewer* panes.** Tile panes to maximize content and minimize clutter.
- **Proactively show a file, diff, or logs** in a pane over pasting or referring to the content in the chat.
- **Laura auto-tiles** A bare `laura open <path>` open splits the newest pane and alternates orientation — a dwindle. No need to capture ids and thread `--split`. Pass `--dir`/`--ratio`/`--split` only to override to create a custom layout. To swap a file into an existing pane, use `laura open <path> --panel <id>` — it replaces content in place (same id, rect, focus, no new split) rather than opening another pane.
- **Read the docs** at https://thomaseleff.github.io/laura-tui/llms.txt.

## Reference

Every `laura` command and its flags. Run `laura --help` (or `laura <cmd> --help`) for the full, current reference — the block below is a snapshot.

```
laura open <path>       Split a pane and render <path> in the new pane. Prints the new pane id.
                        Relative paths resolve against the *calling* process's cwd, so `cd`
                        inside the PTY then `laura open ./x` works. Warns on stderr (still exit 0)
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
                        defaults to start. Opens the pane already scrolled to and spotlighting
                        the range — the one-call "show me where" gesture. e.g. `laura open x.rs
                        --highlight 40 52`.
      --diff            Open straight into the inline diff view (vs git HEAD).
      --panel <id>      Swap a pane's file in place, no new split — same id, rect, focus.
                        Ignores --split/--dir/--ratio/--side.
laura close [<id>]      Close a pane (default: the focused one).
      --all             Close every pane, back to shell-only.
laura focus <id>        Focus a pane by id.
laura highlight [start] [end]
                        Highlight lines start..=end (1-based, inclusive) in a pane and
                        scroll them into view. end defaults to start (single line). Line numbers
                        are the file's real source lines (an editor / wc -l / git blame), for
                        markdown too — a source line inside a hand-wrapped paragraph maps to
                        that whole block. e.g. `laura highlight 40 52`.
      --off             Clear the pane's highlight (start optional; user can also press `h`).
      --pane <id>       pane to highlight (default: the focused pane).
laura diff              Toggle a pane's inline diff view vs git HEAD (interleaved +/- lines).
      --pane <id>       pane to toggle (default: the focused pane).
      --off             Turn the diff view off (default: toggle).
laura layout            Print the layout: per-pane rects + overflow (JSON).
laura ready             Mark the tab as hosting an agent (enables pane interactions). Prints the journal path.
      --session <id>    Name the journal session (default: laura-<pid>-<n>).
      --agent <name>    Attribute journal events to this agent name.
laura feedback          Append a feedback signal (layout/render quality, a missing tool) to the journal.
      --positive        Positive signal.        (one of --positive/--negative is required)
      --negative        Negative signal.
      [<body>]          Optional free-text note.
some-cmd | laura tail   Spool piped stdin to an internal file and show it in a live pane.
      --title <t>       Pane title (also names the spool file).
      --follow          Autoscroll to the newest line as output arrives.
      --split/--dir/--ratio  Same split controls as `open`.
```

`layout` and `open --dry-run` both emit a JSON report, one entry per pane with its rect and overflow, so you can size a pane before (or without) rendering it:

```json
{
  "area": {"x": 0, "y": 1, "width": 120, "height": 39},
  "panes": [
    {"id": 0, "kind": "pty",   "path": null,       "rect": {"x":0,"y":1,"width":48,"height":39},
     "content_rows": null, "visible_rows": 37, "overflow_rows": 0,  "clipped": false},
    {"id": 1, "kind": "panel", "path": "spec.md",  "rect": {"x":48,"y":1,"width":72,"height":39},
     "content_rows": 120, "visible_rows": 37, "overflow_rows": 83, "clipped": true}
  ]
}
```

`overflow_rows > 0` (or `clipped`) means the pane is taller than its rect — widen/reshape the split or lower `--ratio` until it fits. A real `open` surfaces the same condition as a terse `overflows:` / `too small` line on stderr.

## Motions

Common workflows with the `laura` CLI to improve interactions between you and your pair-programmer and when to use them.

### Show a file

**Motion**
1. Run `laura layout` to see what is currently open.
2. Run `laura close <id>` (or `laura close --all`) to clear any pane that is no longer relevant.
3. Run `laura open <path>` to split a pane and render it — `--dry-run` simulates the fit before you commit, and the pane updates live as the source is edited.

**Use when** your pair-programmer asks to see a file, or when you want to put one on screen as part of collaborating, riffing, or improvising.

**Handle stderr warnings** `open` and `tail` warn on `stderr` and still exit 0, so read the warning and re-open with different layout params when the layout can be improved:

- **`overflows:`** — the doc is taller than the pane. A large overflow may indicate you should open it vertically (`--dir v`) to gain height.
- **`too small`** — the pane cannot render the content at all; raise `--ratio` or change `--dir`.
- **`cannot read <path>: …`** — the file path cannot be found.
- **`diff markers unavailable`** — `git` is not installed, so gutter marking and the diff view are disabled.

### Review

**Motion**
1. Run `laura open <path>` so your pair-programmer can mark up the file in place
   - Your partner uses `↑`/`↓` to move to a line, `c` to comment, `Shift+S` to submit the review, `x` to close the pane, and `Esc` to switch back to chat.
2. Once submitted, their review lands in your input as a `[laura review · …]` chat message.
3. Resolve any open questions with your partner first, then act on the review.

**Use when** your pair-programmer asks to review a file, or when you want a review on something you just wrote or modified. In pair-programming, your partner reviews code while you edit, thinking about the big picture, edge cases, conventions, and strategic direction.

### Highlight

**Motion**
1. If the file is not open yet, run `laura open <path> --highlight <start> [end] --no-focus` opens *and* highlights in one call leaving your partners focus in chat.
2. If the file is open, run `laura highlight <start> [end] --pane <id>` to highlight a different section.
3. Re-call to move the highlight as the conversation moves.
4. Run `laura highlight --off [--pane <id>]` to clear it (or the user presses `h` on the focused pane).

**Use when** you want to show a reference while you hold a conversation in chat or when your partner asks for you to explain something step-by-step.

### Show what changed

**Motion**
1. If the file is not open yet, run `laura open <path> --diff` to open straight into the inline `+`/`-` diff against git `HEAD`.
2. if the file is open, run `laura diff --pane <id>` to toggle the pane into diff mode (pass `--off` to revert).

**Use when** you want to show *what* changed, not just *where*, or your partner asks to see the latest changes to a file.

By default an open file always shows added, modified and deleted lines via the line number gutter markers. Clean or untracked files, or a machine without `git`, are a no-op that warns on stderr.

### Tail live output

**Motion**
1. Pipe a running command into `laura tail` to tile a thin, autoscrolling log pane `some-cmd | laura tail --title logs --follow --split <id> --dir v --ratio 30` puts the source above and the streaming output below.

**Use when** you are running or debugging a service and want to display the logs for your partner to watch live, or, your partner asks for you to bring up or monitor a service.

## Workflows

Refer to the following additional `laura` skills for longer running workflows, which sequence motions into interactive experiences with your pair-programmer.

- **`demo`** runs a guided, scripted tour of Laura — the review loop, panes, tailing, and feedback.
- **`explain`** walks the user through a PR, file, or flow, stepping across highlighted ranges one at a time on their cue.
- **`learn`** teaches a concept hands-on through different styles and complexities: it exposes the idea step by step, has the learner produce something, then reviews it in place and revises.
- **`retro`** reads back the feedback and reviews recorded across sessions — sentiment, negative notes, grouping — so you can *summarize how sessions have gone over time*.
