# Protocol

An agent mutates a tab's panel by running `laura open <file>` — a **separate process** from the TUI host. This is the seam it crosses.

## Message shapes

Typed messages, one JSON object per line (NDJSON), internally tagged by `type`:

```json
{"type":"open","path":"spec.md","focus":true}
{"type":"close"}
{"type":"ready"}
{"type":"update","path":"spec.md"}
```

`open`'s `focus` moves focus into the panel (default `true`, so an older `{"type":"open"}` still focuses); `close` removes the tab's panel; `ready` marks the tab as hosting an agent, which gates review injection (see below). `update` is a reserved re-render nudge, not yet emitted.

## Addressing

`LAURA_TAB` holds the tab's **namespaced** socket name (Windows named pipe / Unix namespaced). One socket per tab. A producer reaches a tab by connecting to that name and writing one frame.

> Scoping is a consequence of addressing, not a security boundary — anything that can read `LAURA_TAB` can write the tab.

## CLI

Client verbs read `$LAURA_TAB`, send one message, and exit:

- `laura` — run the TUI, hosting your default shell in tab 1.
- `laura -- <cmd>` — run the TUI, hosting `<cmd>` in tab 1 (new tabs still get the shell).
- `laura open <file> [--no-focus]` — open a panel; focus moves into it unless `--no-focus`.
- `laura close` — close the tab's panel.
- `laura ready` — mark the tab as hosting an agent (enables review submission).

`--help`, `--version`, and per-subcommand `--help` are provided by clap.

## Review payload

Submitting a review (`S` on a commented panel) doesn't send a protocol message — it **injects text straight into the tab's agent PTY**, so the agent reads its own review from its input stream. Injection is **gated on `ready`** and fails closed: until the tab has received a `ready` message, both `c` (comment) and `S` (submit) are inert — no point building comments there's no consumer to read. This is the [injection boundary](technical-vision.md#the-injection-boundary) — anything laura *shows* is unconditional; anything it *writes into a PTY* needs a declared consumer. The assembled block:

```
[laura review · <path>]

<overall body>            ← line omitted when overall is empty

L<n>  <line n text>
      > <comment>
      > <second comment on the same line>
```

`L<n>` is 1-based (matching the comment UI); comments on one line group under a single header. If the file shrank past a commented line, the `L<n>` header is emitted without body text. Surrounding-context lines aren't included — bare `L<n>` refs only.

The block is wrapped in **bracketed paste** (`ESC[200~ … ESC[201~`) with a single trailing `\r` outside the close marker: embedded newlines stay inside the markers so a line-reading REPL doesn't submit each line early, and the block submits once. Paste-honoring is REPL-specific — confirm it against the target agent's REPL; a bare shell ignores the markers. Per-tab messaging will reuse this injection path.
