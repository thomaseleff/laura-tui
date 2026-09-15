# Technical vision

The engineering objectives. Product context is in [explanation.md](explanation.md); the wire protocol in [protocol.md](../docs/protocol.md).

---

## The core — invariants the build holds

### laura is the hub

laura is the one long-lived process. It owns every tab's PTY and every tab's socket: hub-and-spoke, laura at the center, one spoke per tab. Every client — the agent CLI, a statusline, a future MCP server — talks to laura, never to another client.

Three invariants follow:

1. **State and routing live in laura.** A client sends a message; laura holds the resulting state (`tab.agent`, the open pane) and routes between tabs. Clients are stateless.
2. **The wire `Message` is transport-agnostic.** A message means the same thing whether it arrived over the per-tab socket, a stdio MCP server, or anything later. No transport detail leaks into message semantics.
3. **Transports are swappable adapters.** The CLI socket, a per-tab stdio MCP server, a shared daemon — each is a thin adapter onto the same `Message` set and the same laura-held state.

### The injection boundary

Everything laura does splits by direction of data flow:

- **laura → screen (view).** `open`, panes, a future statusline or diff. laura reads a source and renders it beside the shell. This depends on nothing in the PTY — a pane renders the same whether the tab hosts an agent, a bare shell, or a dead one.
- **laura → PTY (injection).** Review submission, and later cross-tab messaging. laura writes into someone else's input stream, so it needs a live consumer on the other end.

What laura shows is always safe. What laura writes into a PTY needs a ready consumer. The "is an agent listening?" question lives only at that second boundary — `open` and `close` never touch it; review submission and messaging do.

### Readiness

laura must not guess whether a PTY hosts something that will consume an injected message. Both guesses are unreliable:

- **Sniffing PTY output** for a known prompt — new agent CLIs ship constantly, so you'd maintain a signature list forever.
- **Inferring from the launch command** (`laura -- claude`) — no better: `laura -- ls` hosts no agent, `laura -- python` hosts a REPL that isn't one.

So the agent declares itself. Today that's a one-shot `laura ready`, a client verb that sets `tab.agent = true`. It covers both `laura -- claude` and "launch a shell, then run the agent" with one signal, and it fails closed: no declaration, no injection.

### Arrangement is a verb set, not a fixed split

A tab is a recursive split tree the agent drives through verbs. `open --split <id> --dir <h|v> --ratio <n> --side` splits any pane; `close` and `focus` address panes by a stable per-tab id (the shell is pane `0`; see [pane identity](../docs/protocol.md#pane-identity)); `layout` and `open --dry-run` report per-pane rects and overflow so a pane can be sized before it's committed. `dir`, `ratio`, and `side` are fields on the mutation — laura owns the screen, so honoring them is rendering work, not new architecture. The socket is [request/response](../docs/protocol.md#request-and-response): every verb gets one reply — the new pane id, a report, or a typed error — because the run loop holds the live layout to answer from.

---

## Trajectory — where the invariants lead

### CLI and MCP coexist

When an MCP server arrives, it's added alongside the CLI, not instead of it. `open`, `close`, and `ready` become MCP tools that cross the same seam and set the same state as the `laura` subcommands. Each transport carries coverage the other can't:

- **CLI** — humans, shell scripts, Makefiles, git hooks; zero setup; fire-and-forget; composes in a pipeline.
- **MCP** — typed tools, structured returns, and connection liveness the CLI can't express.

Keeping both costs almost nothing — each is a thin adapter, and routing, state, and rendering are shared. The only risk is drift, and one rule prevents it: adapters carry no logic; both map onto `Message`.

A connection-based transport also upgrades readiness. `laura ready` can only ever set the bit true, so a stale `true` lingers if the agent dies but the shell survives. A connection gives liveness for free — connected is ready, disconnected is not — as another way to flip, and unflip, the same laura-held bit.

### Cross-agent messaging is laura routing

Agents never talk to each other; they talk to laura, and laura routes: tab A → laura → inject into tab B's PTY, the same injection path as review submission. So per-tab MCP servers being isolated from one another is a non-problem — they were never the bus; laura is. And a shared MCP daemon isn't needed: it would be a second hub redundant with laura, and it would cost the free addressing that `LAURA_TAB` gives a per-tab client.

### The view side — hooks and saved frames

The view side (`laura → screen`) grows the same way the injection side does, under the same rule: one mutation protocol, no special cases. Each piece below is another producer speaking `Message` into laura-held state, not a privileged path the agent gets and an extension can't.

- **Panes refresh from background hooks.** `update` (reserved today, see [protocol.md](../docs/protocol.md#request-shapes)) is the seam. A producer — the agent, a script, a statusline binary — registers a command laura runs on an interval or event, and its output re-renders the pane. That's the same protocol a human `laura open` crosses. A context-usage statusline or a tailed `kubectl` pane lands this way, with no bespoke feature each.
- **A composed frame is saveable config.** Once arrangement is verbs, a frame is a sequence of them, so it serializes. Write the layout you like to config; replay it next session. No new mechanism — a recorded `Message` stream.

Both are `laura → screen`, so the injection boundary never applies: always safe, landing on the same laura-held state, reachable by any producer.

### The one forward-consequential decision — identity

Under these invariants, transport choice is reversible — start with the socket, add MCP later, run both. The one decision that isn't is the addressing and identity model. Today a tab is addressed by its ephemeral `LAURA_TAB` socket name, and panes within it carry stable ids — but tabs, and the agents across them, don't. Stable identity — addressing "the reviewer agent", or "tab 2" — is what cross-agent messaging and multi-surface tabs will eventually need.
