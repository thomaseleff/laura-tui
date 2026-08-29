# Why Laura exists

## The problem

Coding agents live in the terminal, but the terminal is a firehose, not a workspace. When an agent wants to show you something richer than text — a rendered doc, a diff worth discussing, a running demo — it can't. So you leave to a browser, a diff tool, a PR, an IDE. Context shatters across surfaces and the tight loop of *show me → let me react → try again* breaks.

## The idea

Terminals treat agent output as a **stream you scroll**. Laura treats it as a **surface you and your agent compose** — an **API over the TUI**. The agent **procedurally assembles** the UI for your prompt: it decides what to show, where, and how it refreshes, rather than handing you the same fixed layout every session. One rendered panel today is the proof-of-concept slice of that; the protocol is the general form.

Laura owns the screen and organizes work into **tabs**. Each tab hosts a **shell (PTY)** where an agent runs, plus **panels** the agent opens *within its tab*. Drawing and updating panels happens through **one mutation protocol** anything can speak: the agent is a client, and so is any future producer — a statusline, a status panel, an extension — all the same protocol.

## The model

- **Tab** — the top-level unit. Each tab owns one shell/PTY and its own set of panels.
- **Shell** — an agent (or you) runs in the tab's PTY. Laura never wraps or reinterprets it.
- **Panel** — a view you or the agent opens within its tab (code, rendered doc), markable with in-line comments. File-backed panels are **live**: they track their source and re-render as it changes.
- **Protocol** — the interface a producer uses to open/update panels, collect comments, and submit reviews. Transport is a per-tab local socket named from `LAURA_TAB`; scoping is a consequence of addressing, not a security boundary — every client is local and spawned by you. See [protocol.md](protocol.md).

## The core loop

*Agent runs in a tab's shell → agent opens a panel in that tab → you see it → you comment in place → your feedback flows back → agent revises.* Never leave the terminal.

## Design principles

- **The shell is sacred.** Laura never intercepts, wraps, or reinterprets what you run. A plain-terminal workflow works unchanged.
- **One protocol, no special cases.** The agent has no privileged path an extension couldn't use.
- **Show, don't tell.** Every capability exists to let the agent show work and let you react in place.
- **Live by default.** File-backed panels are watched and re-render on disk change.
- **Elegant and bare.** Calm, minimal, screenshot-worthy. Nothing on screen you didn't ask for.
- **Local, private, fast.** Runs on your machine; the shell never stutters.

## Where Laura sits

- **Agent multiplexers** run N agents in panes and watch status. Laura hosts shells.
- **Diff-review tools** bolt review onto a diff. Laura makes review-as-canvas the center.
- **The space Laura takes:** the terminal as a shared, programmable canvas you and the agent both draw on and mark up.

*Others let you watch your agents. Laura lets your agent show you — and you show it back.*

## Non-goals

- Not a coding agent (no model, prompts, or harness — bring your own).
- Not a multiplexer/orchestrator (the shell is substrate, not the pitch).
- Not an IDE (no LSP, build system, or project model).
- Not a cloud product (single machine, local-first).

Rule of thumb: features that make Laura a better *worker* or *orchestrator* are out; features that make it a better *canvas and workspace* are in.

The engineering invariants behind the protocol seam are in [technical-vision.md](technical-vision.md).
