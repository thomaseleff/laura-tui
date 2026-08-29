# Technical Vision

The engineering objectives. Product context lives in [explanation.md](explanation.md); the concrete wire protocol in [protocol.md](protocol.md).

---

## The core — invariants the build must not violate

### laura is the hub
laura is the one long-lived process. It owns every tab's PTY and every tab's socket — hub-and-spoke, laura at the center, one spoke per tab. Every client (the agent CLI, a statusline, a future MCP server) is a spoke that talks *to laura*, never to another client.

Three invariants follow:

1. **All state and routing live in laura.** A client sends a message; laura holds the resulting state (`tab.agent`, the open panel, …) and does all routing between tabs. Clients are stateless.
2. **The wire `Message` is transport-agnostic.** No transport specifics leak into message semantics. A message means the same thing whether it arrived over the per-tab socket, a stdio MCP server, or anything later.
3. **Transports are swappable adapters.** The CLI socket, a per-tab stdio MCP server, a shared daemon — each is a thin adapter that lands on the same `Message` set and the same laura-held state.

### The injection boundary
Everything laura does splits by direction of data flow:

- **laura → screen (view).** `open`, panels, a future statusline or diff. laura reads a source and renders it beside the shell. **Zero dependency** on what runs in the PTY — a panel renders identically whether the tab hosts an agent, a bare shell, or a dead one.
- **laura → PTY (injection).** Review submission, and later cross-tab messaging. laura writes into someone else's input stream, so it needs a live *consumer* on the other end.

> **Anything laura *shows* is unconditional and always safe. Anything laura *writes into a PTY* requires the target to be a ready consumer.**

The whole "is an agent listening?" question — and the only place the fragile-signal problem exists — lives at that one boundary. `open`/`close` never touch it; `S`/review and messaging do.

### Readiness
laura must not *guess* whether a PTY hosts something that will consume an injected message. Both guesses are unreliable:

- **Sniffing PTY output** for a known prompt — new agent CLIs ship constantly; you'd maintain a signature list forever.
- **Inferring from the launch command** (`laura -- claude`) — no better: `laura -- ls` hosts no agent, `laura -- python` hosts a REPL that isn't one.

So the agent **declares itself** instead. Today that's a one-shot `laura ready` (a client verb that flips `tab.agent = true`); it covers both `laura -- claude` and "launch a shell, then run the agent" with one signal, and it **fails closed** — no declaration, no injection.

---

## Trajectory — where the invariants let us go

### Transports are adapters, not the model (CLI and MCP coexist)
When an MCP server arrives it is added *alongside* the CLI, not instead of it. `open`/`close`/`ready` become MCP tools that cross the *same seam* and set the *same state* as the `laura` subcommands. Keeping both costs almost nothing (each is a thin adapter; routing/state/rendering are shared) and buys coverage the other can't:

- **CLI** — humans, shell scripts, Makefiles, git hooks; zero setup; fire-and-forget; composes in a pipeline.
- **MCP** — typed tools, structured returns, and *connection liveness* the CLI can't express.

The only cost is drift, and the rule that prevents it: **adapters carry no logic; both map onto `Message`.**

A connection-based transport also *upgrades* readiness: *connected = ready, disconnected = not ready* falls out of the transport, giving liveness the one-shot `laura ready` can't (it can only ever set the bit true; a stale `true` lingers if the agent dies but the shell survives). It just becomes another way to flip — and unflip — the same laura-held bit.

### Cross-agent messaging is laura routing, not a transport topology
Agents never need to talk to each other; they talk to laura, and laura routes: tab A → laura → inject into tab B's PTY (the same injection path as review submission). This is why per-tab MCP servers being isolated from one another is a non-problem — they were never the bus. laura is. And it's why a shared MCP daemon is not needed: that would be a second hub redundant with laura, and it would cost the free addressing that `LAURA_TAB` gives a per-tab client.

### The view side — arrangement, hooks, saved frames
The trajectory above follows the injection boundary (`laura → PTY`) all the way out. The `laura → screen` side has the same room to grow, under the same rule that keeps the injection side honest: **one mutation protocol, no special cases.** Every piece below is another producer speaking `Message` into laura-held state — never a privileged path the agent gets and an extension can't.

- **Arrangement is a verb set, not a fixed split.** Today a tab has one panel in a hardcoded layout. `open` is the first verb; split, place, size, and stack are the rest. They let the agent *assemble* the frame for the task — logs bottom, plan right, a statusline strip on top — instead of every tab looking the same. Position and size are just fields on the mutation; laura already owns the screen, so honoring them is rendering work, not new architecture.
- **Panels refresh from background hooks.** `update` (reserved today, [protocol.md](protocol.md#message-shapes)) is the seam. A producer — the agent, a dev's script, a statusline binary — registers a command laura runs on an interval or event, and its output re-renders the panel. That's the same protocol a human `laura open` crosses; a live metadata panel is just a producer that keeps talking. This is how a context-usage statusline or a tailed `kubectl` panel lands without a bespoke feature each.
- **A composed frame is saveable config.** Once arrangement is verbs, a frame *is* a sequence of them, so it serializes. Write the layout you like to config; reload it next session. No new mechanism — a recorded `Message` stream, replayed.

All three are `laura → screen`: unconditional and always safe (the injection boundary never applies), all landing on the same laura-held state, all reachable by any producer. The proof-of-concept renders one markdown panel; the protocol is built to assemble the whole workspace.

### The one genuine one-way door — identity
Under the invariants above, transport choice is *not* irreversible — start with the socket, add MCP later, run both. The only genuinely forward-consequential decision is the **addressing / identity model**: today a tab is addressed by its ephemeral `LAURA_TAB` socket name and lifecycle verbs act on the tab's single panel (no handles). Stable identity (address "the reviewer agent," or "tab 2") is what cross-agent messaging and multi-surface tabs may eventually need.
