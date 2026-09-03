# Protocol

An agent mutates a tab's panel by running `laura open <file>` — a **separate process** from the TUI host. This is the seam it crosses.

## Request / response

One connection carries **one request and one response**: the producer connects, writes a single request frame, and reads a single response frame before the socket closes. The TUI run loop answers, because it holds the live layout state a reply reports on. A client that reads EOF with no frame treats it as `ok`.

## Request shapes

Typed messages, one JSON object per line (NDJSON), internally tagged by `type`. Fields have defaults, so older/shorter frames still parse:

```json
{"type":"open","path":"spec.md","split":null,"dir":"horizontal","ratio":50,"side":"second","focus":true,"dry_run":false}
{"type":"close","pane":null,"all":false}
{"type":"focus","pane":1}
{"type":"layout"}
{"type":"ready"}
{"type":"update","path":"spec.md"}
```

- **`open`** splits a pane (`split`, default: the focused pane) into a new panel rendering `path`. `dir` is `horizontal`/`vertical`, `ratio` (1..99) is the new panel's percent, `side` (`first`/`second`) is where the new panel lands, `focus` moves focus into it (default `true`), `dry_run` reports the would-be layout without mutating.
- **`close`** removes pane `pane` (default: the focused panel); `all` returns the tab to shell-only. The shell (pane `0`) can't be closed.
- **`focus`** focuses a pane by id.
- **`layout`** asks for the current layout report (no mutation).
- **`ready`** marks the tab as hosting an agent, which gates review injection (see below).
- **`update`** is a reserved re-render nudge, not yet emitted.

## Response shapes

One response per request, tagged by `type`:

```json
{"type":"ok"}
{"type":"opened","pane":1,"warnings":["panel shown, but run `laura ready` to enable review submission"]}
{"type":"report","area":{...},"panes":[{"id":0,"kind":"pty","rect":{...},"overflow_rows":0,"clipped":false}, ...]}
{"type":"error","message":"no pane #7"}
```

`opened` carries the new pane id (which `laura open` prints) and any non-fatal warnings. `report` answers `layout` and `open --dry-run`: one `PaneReport` per pane with `rect`, `content_rows`, `visible_rows`, `overflow_rows`, and `clipped`, so a producer can measure fit. `error` is a typed failure (`laura` prints the message and exits non-zero).

## Pane identity

A tab is a recursive binary split tree; each leaf is a pane with a per-tab monotonic `u64` id. The shell is always pane `0`. Ids are stable and never reused within a tab, so closing a middle pane leaves a gap (ids `0, 4` after closing `1..3`). Requests address panes by id; the `^p` panes popup maps a 1-based positional label to the current id.

## Addressing

`LAURA_TAB` holds the tab's **namespaced** socket name (Windows named pipe / Unix namespaced). One socket per tab. A producer reaches a tab by connecting to that name and writing one frame. The name carries per-process entropy (`laura-<pid>-<nonce>-<n>`) so a reused PID can't re-mint a dead tab's name: a stale inherited `LAURA_TAB` fails to connect rather than routing into a live tab.

> Scoping is a consequence of addressing, not a security boundary — anything that can read `LAURA_TAB` can write the tab.

## CLI

Client verbs read `$LAURA_TAB`, send one message, and exit:

- `laura` — run the TUI, hosting your default shell in tab 1.
- `laura -- <cmd>` — run the TUI, hosting `<cmd>` in tab 1 (new tabs still get the shell).
- `laura open <file> [--split <id>] [--dir h|v] [--ratio n] [--side first|second] [--no-focus] [--dry-run]` — split a pane and open a panel; prints the new pane id.
- `laura close [<id>] [--all]` — close a panel (default: focused; `--all` for shell-only).
- `laura focus <id>` — focus a pane.
- `laura layout` — print the layout report (JSON).
- `laura ready` — mark the tab as hosting an agent (enables review submission).

See the [CLI reference](cli.md) for flag defaults and the report shape.

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
