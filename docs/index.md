# Laura

*LOW-rah* — a TUI workspace your agent builds with you while you work.

Laura hosts your agent's shell in a PTY, and you or the agent open files into live panels — splitting the tab into a composable tree of panes. Mark a file up in place and your review is injected straight back into the agent's chat. The *show → react → revise* loop never leaves the terminal.

![Laura demo](assets/laura-demo.gif)

## Quickstart

**1. Install Laura.** Requires Rust (stable); installs a `laura` binary:

```bash
cargo install --git https://github.com/thomaseleff/laura-tui laura --locked
```

**2. Install the skill** so your agent knows how to drive `laura`. In Claude Code:

```
/plugin marketplace add thomaseleff/laura-tui
/plugin install laura@laura-tui
```

**3. Start Laura** — it hosts your default shell in a tab:

```bash
laura -- claude "/laura:demo"
```

**4. Chat with your agent** in that shell. With the skill installed, the agent drives the `laura` CLI itself — splitting panes and opening files into them while your shell stays live alongside. You comment on a line in place and submit; your review is injected straight back into the agent's chat and it revises.

New to it? The [tutorial](tutorial.md) walks through one full loop, including the review keys.

## Where to go next

- **[Tutorial](tutorial.md)** — your first Laura session, start to finish.
- **[How-to](how-to.md)** — task recipes.
- **[CLI reference](cli.md)** — the verb set.
- **[Protocol](protocol.md)** — the wire format.
- **[Explanation](explanation.md)** / **[Technical vision](technical-vision.md)** — why Laura exists.
