# Why Laura exists

## The problem

Coding agents run in the terminal, but the terminal is a stream you scroll, not a workspace you compose. When an agent wants to show you something richer than text — a rendered doc, a diff worth discussing, a running demo — it can't. So you leave for a browser, a diff tool, a PR, an IDE, and the show-me, react, retry loop breaks across surfaces.

## What Laura is

Laura is an **API over the TUI**. The agent assembles the screen for the task at hand — what to show, where, how it refreshes — instead of handing you the same fixed layout every session. One rendered pane is the proof of concept; the protocol is the general form for external processes to interact with the TUI.

Laura owns the screen and organizes work into tabs. Each tab holds one shell — a PTY where an agent or you runs — and the panes the agent opens within that tab. Every draw and update goes through one mutation protocol: the agent is a client, and so is any future producer — a statusline, a status pane, an extension — over the same protocol.

## The model

- **Tab** — the top-level unit. Owns one shell/PTY and its own panes.
- **Shell** — an agent, or you, runs in the tab's PTY. Laura never wraps or reinterprets it.
- **Pane** — a view opened within a tab (code, a rendered doc), markable with inline comments. File-backed panes are live: they track their source and re-render when it changes.
- **Protocol** — how a producer opens and updates panes, collects comments, and submits reviews. Transport is a per-tab local socket named from `LAURA_TAB`. Scoping falls out of addressing, not security — every client is local and spawned by you. See [protocol.md](../docs/protocol.md).

## The core loop

The agent runs in a tab's shell, opens a pane, you see it, you comment on a line, your feedback flows back, the agent revises. You never leave the terminal.

## Design principles

- **The shell is sacred.** Laura never intercepts, wraps, or reinterprets what you run. A plain-terminal workflow works unchanged.
- **One protocol, no special cases.** The agent has no path an extension couldn't use.
- **Show, don't tell.** Every capability exists to let the agent show work and let you react in place.
- **Live by default.** File-backed panes re-render on disk change.
- **Elegant and bare.** Nothing on screen you didn't ask for.
- **Local and fast.** Runs on your machine; the shell never stutters.

## Where Laura sits

- **Agent multiplexers** run N agents in panes and watch status. Laura hosts one shell and gives the agent the screen.
- **Diff-review tools** bolt review onto a diff. Laura puts review in the pane where you're already working.
- **The space Laura takes** — the terminal as a canvas you and the agent both draw on and mark up.

## Non-goals

- Not a coding agent (no model, prompts, or harness — bring your own).
- Not a multiplexer or orchestrator (the shell is substrate, not the pitch).
- Not an IDE (no LSP, build system, or project model).
- Not a cloud product (single machine, local-first).

Rule of thumb: a feature that makes Laura a better worker or orchestrator is out; one that makes it a better canvas is in.

The engineering invariants behind the protocol seam are in [technical-vision.md](technical-vision.md).
