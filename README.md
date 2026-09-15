# Laura

Laura is a TUI workspace that provides your agent with an **API over the TUI**, allowing your agent to dynamically tile panes while you pair-program.

<p align="center"><img src="docs/assets/laura-demo.gif" alt="Laura demo" width="100%"></p>

Your agent can open the file you're discussing, the plan you're following, the diff you just made, or tail logs of your running service. Laura supports every coding agent CLI and agents learn how to use the CLI through agent skills.

> [!NOTE]
> Laura is focused on making collaboration more enjoyable, celebrating the riffing, the live review and problem solving, and the improvisation between you and your coding agent, without ever leaving the terminal.

**Laura lets you:**

- **Stay in the flow** - view a file, a plan, a diff, or a log right in the TUI, without switching windows between the terminal, IDE, and browser.
- **Compose the TUI** - your agent tiles the workspace from your natural-language conversation through the `laura` CLI and skills.
- **Pick your coding agent CLI** - Laura hosts a shell your agent runs inside without abstraction, so every CLI and model is compatible.
- **Interact in every pane** - mark up any pane in place and inject the review straight into your conversation.
- **Learn while staying focused** - your agent shows and explains concepts live, step by step.

## How it works

Laura runs your agent CLI in a PTY and provides a set of agent skills you or your agent invoke. The skills instruct the agent on how to use the `laura` CLI, which drives the TUI over a small line-based protocol on a per-agent socket. See the [protocol reference](docs/protocol.md) to learn more.

## Quickstart

**1. Install Laura.** Requires Rust (stable); installs a `laura` binary:

```bash
cargo install --git https://github.com/thomaseleff/laura-tui laura --locked
```

Or from a clone: `cargo build --release --locked` → `target/release/laura`.

**2. Install the skill** so your agent knows how to drive `laura`. In Claude Code:

```
/plugin marketplace add thomaseleff/laura-tui
/plugin install laura@laura-tui
```

**3. Start Laura** and run the interactive demo:

```bash
laura -- claude "/laura:demo"
```

**4. Work with your agent.** Chat as you normally would. Ask it to show you a file, a plan, or a diff. Your agent will leverage the skills to tile panes alongside your chat. Comment on a line in place and submit; your review goes straight back into the agent's chat.

Panes are live-watched: edit the source and they re-render. Markdown renders with terminal styling; other files show their content with syntax colours, and lines changed since git `HEAD` are marked in the gutter.

See the [tutorial](docs/tutorial.md) for a first session and [navigating the TUI](docs/navigation.md) for the keys. Reference lives in the [CLI reference](docs/cli.md) for the verb set, the [protocol reference](docs/protocol.md) for the wire format, and the [agent reference](docs/agent-reference.md) for the recipes your agent drives on your behalf.

## License

MIT — see [LICENSE](LICENSE).
