<div align="center">

$\Huge\textsf{Laura}$

</div>

<p align="center"><em>LOW-rah</em> — a tui workspace your agent builds with you in the terminal.</p>

---

Every coding agent UI in a terminal is a **stream you scroll**, another chat interface. Laura makes it a **surface you and your agent compose** — an **API over the TUI**. Your agent procedurally assembles the workspace — which panels, laid out where, refreshed by what — for the task in front of you, instead of every session getting the same fixed layout.

_Currently_, Laura hosts your agent's shell in a PTY and gives you both a live side-panel — you or the agent open a file, mark it up in place, and your review is injected straight back into the agent's chat. The *show → react → revise* loop never leaves the terminal. One rendered panel today is the proof-of-concept slice; the protocol is the general form.

<p align="center"><img src="docs/assets/laura-demo.gif" alt="Laura demo" width="100%"></p>

## Install

Requires Rust (stable). Installs a `laura` binary:

```bash
cargo install --git https://github.com/thomaseleff/laura-tui laura --locked
```

Or from a clone: `cargo build --release --locked` → `target/release/laura`.

### For agents

Install the skill so your agent knows how to use `laura`:

```
# Claude Code
/plugin marketplace add thomaseleff/laura-tui
/plugin install laura@laura-tui
```

## Quickstart

Start Laura — it hosts your default shell in a tab:

```bash
laura
```

Inside that shell run any coding agent and ask it to show you a file:

```bash
laura open docs/protocol.md
```

The file renders on the right; your shell stays live on the left. The panel is **live-watched** — edit the source and it re-renders on changes.

**Supported panel types:** `.md`/`.markdown` render with terminal styling (headings, bold, code);  every other file shows its raw bytes. Recognized code files get Nord syntax colours. Rendering is display-only — comments and reviews always quote the plain line text.

## The core loop

*Agent opens a panel → you see it → you comment in place → your review is injected back into the agent's shell → it revises.* Run `laura ready` once to enable review submission, focus the panel (`Ctrl+T` then `p`), press `c` to comment a line and `S` to submit.

See the [tutorial](docs/tutorial.md) for a first session, [how-to](docs/how-to.md) for task recipes, the [CLI reference](docs/cli.md) for the verb set, the [protocol](docs/protocol.md) for the wire format, and the [explanation](docs/explanation.md) / [technical vision](docs/technical-vision.md) for why Laura exists.

## Keys

`Ctrl+T` tab mode (then `←/→` browse · `Enter` select · `n` new tab · `p` focus panel) · `Ctrl+H` help · `Ctrl+Q` quit. In a focused panel: `↑/↓` move · `c` comment · `S` submit · `Esc` leave. Outside tab mode, every key reaches the shell untouched.

## Windows / ConPTY

The PTY reader runs on its own thread for two ConPTY reasons: it answers the DSR handshake (ConPTY withholds all child output until the host replies to `ESC[6n` with `ESC[<row>;<col>R`), and it detects child exit via `child.wait()` (the master reader never returns EOF on exit).

## License

MIT — see [LICENSE](LICENSE).
