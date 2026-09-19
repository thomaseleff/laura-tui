# Protocol

The protocol is NDJSON communication over a socket, either a Windows named pipe or a Unix namespaced socket, that allows for external processes to dynamically tile panes or interact with a Laura workspace. Whether through the `laura` CLI or an MCP service, all external interactions flow through the protocol.

> [!TIP]
> A tab has only two kinds of interaction: over the protocol, from an outside process, and in-process, inside the TUI. In-process interactions, like commenting on a line, scrolling, submitting a review, do not communicate via the protocol (see [In-process interactions](#in-process-interactions)).

## Addressing

The `LAURA_TAB` environment variable holds the Laura workspace socket name, one socket per tab. A client reaches a tab by connecting to that name and sending a request.

The name includes per-process entropy (`laura-<pid>-<nonce>-<n>`) so a reused PID cannot recreate a dead tab's name. A stale `LAURA_TAB` inherited by a later process fails to connect rather than routing into a live tab.

### Panes

A Laura workspace is a recursive binary split tree; each leaf is a pane with a per-tab monotonic `u64` id. The shell is always pane `0`. Ids are stable and never reused within a tab, so closing a middle pane leaves a gap (ids `0, 4` after closing `1..3`). Requests address panes by id.

## Messages

Each connection carries one request and one response: the client connects, sends a single request frame, reads a single response frame, and the socket closes. A *frame* is one JSON object on a single line, e.g. NDJSON. The response comes from the TUI process, which holds the live layout state. Messages are internally tagged by `type` with fields and default values. A client that reads EOF with no response frame treats the result as `ok`.

### `open`

Splits a pane into a new pane rendering a file.

<table class="proto">
<tr>
<td valign="top">

<dl>
<dt><code>path</code> · string · <strong>required</strong></dt>
<dd>File to render.</dd>
<dt><code>split</code> · integer · <em>default: focused pane</em></dt>
<dd>Pane to split.</dd>
<dt><code>dir</code> · string · <em>optional — absent → inferred</em></dt>
<dd><code>horizontal</code> or <code>vertical</code>.</dd>
<dt><code>ratio</code> · integer · <em>optional — absent → inferred</em></dt>
<dd>New pane's percent, <code>1..99</code>.</dd>
<dt><code>side</code> · string · <em>optional — absent → inferred</em></dt>
<dd><code>first</code>/<code>second</code> — where the new pane lands.</dd>
<dt><code>focus</code> · boolean · <em>default: <code>true</code></em></dt>
<dd>Move focus into the new pane.</dd>
<dt><code>dry_run</code> · boolean · <em>default: <code>false</code></em></dt>
<dd>Report the resulting layout without changing anything.</dd>
<dt><code>highlight</code> · [int, int] · <em>default: <code>null</code></em></dt>
<dd><code>[start, end]</code>, applied as the pane first paints.</dd>
<dt><code>diff</code> · boolean · <em>default: <code>false</code></em></dt>
<dd>Open straight into the inline diff view.</dd>
<dt><code>panel</code> · integer · <em>default: <code>null</code></em></dt>
<dd>Replace this pane's content in place (same id, rect, focus) instead of splitting; the PTY and absent ids error, and <code>split</code>/<code>dir</code>/<code>ratio</code>/<code>side</code> are ignored when set.</dd>
</dl>

</td>
<td valign="top">

<strong>Request</strong>
<pre>
{
  "type": "open",
  "path": "spec.md",
  "split": null,
  "dir": null,
  "ratio": null,
  "side": null,
  "focus": true,
  "dry_run": false,
  "highlight": null,
  "diff": false
}
</pre>

<strong>Response</strong> · <code>opened</code>
<pre>
{
  "type": "opened",
  "pane": 1,
  "warnings": []
}
</pre>

</td>
</tr>
</table>

`opened` carries the new pane id (which `laura open` prints) and any warnings — a `diff` refusal surfaces here rather than as an error. With `dry_run`, the response is `report` instead.

**Inference.** When `dir`, `ratio`, `side`, and `split` are all absent, the server picks the split: it splits the newest pane, alternating orientation by depth — a dwindle — so repeated bare opens split off the newest pane, at a flat 50%. Any one of those fields present bypasses inference. A split that would collapse a pane below the renderable minimum is refused with an `error`.

### `close`

Removes a pane, or returns the tab to shell-only.

<table class="proto">
<tr>
<td valign="top">

<dl>
<dt><code>pane</code> · integer · <em>default: focused pane</em></dt>
<dd>Pane to remove.</dd>
<dt><code>all</code> · boolean · <em>default: <code>false</code></em></dt>
<dd>Return the tab to shell-only.</dd>
</dl>

</td>
<td valign="top">

<strong>Request</strong>
<pre>
{
  "type": "close",
  "pane": null,
  "all": false
}
</pre>

<strong>Response</strong> · <code>ok</code>
<pre>
{
  "type": "ok"
}
</pre>

</td>
</tr>
</table>

The shell (pane `0`) cannot be closed.

### `focus`

Focuses a pane by id.

<table class="proto">
<tr>
<td valign="top">

<dl>
<dt><code>pane</code> · integer · <strong>required</strong></dt>
<dd>Pane to focus.</dd>
</dl>

</td>
<td valign="top">

<strong>Request</strong>
<pre>
{
  "type": "focus",
  "pane": 1
}
</pre>

<strong>Response</strong> · <code>ok</code>
<pre>
{
  "type": "ok"
}
</pre>

</td>
</tr>
</table>

### `highlight`

Highlights a range of lines in a pane and scrolls it into view.

<table class="proto">
<tr>
<td valign="top">

<dl>
<dt><code>pane</code> · integer · <em>default: focused pane</em></dt>
<dd>Pane to highlight.</dd>
<dt><code>range</code> · [int, int] · <em>default: <code>null</code></em></dt>
<dd>Lines <code>[start, end]</code>, 1-based inclusive. <code>null</code> clears the pane's highlight.</dd>
</dl>

</td>
<td valign="top">

<strong>Request</strong>
<pre>
{
  "type": "highlight",
  "pane": null,
  "range": [40, 52]
}
</pre>

<strong>Clear</strong> · <code>range: null</code>
<pre>
{
  "type": "highlight",
  "pane": null,
  "range": null
}
</pre>

<strong>Response</strong> · <code>ok</code>
<pre>
{
  "type": "ok"
}
</pre>

</td>
</tr>
</table>

Line numbers are source-file lines, matching the gutter and review `L<n>`; for markdown a hand-wrapped paragraph collapses onto one rendered row, so any of its source lines maps to that block. The highlight is independent of focus and of the cursor, and persists until re-set, cleared (`range: null`), or the file reloads shorter. Out-of-range values clamp to the file. Clearing leaves the cursor and scroll untouched; the user can also press `h` on the focused pane to clear.

### `diffview`

Toggles a pane's inline diff view against git `HEAD`.

<table class="proto">
<tr>
<td valign="top">

<dl>
<dt><code>pane</code> · integer · <em>default: focused pane</em></dt>
<dd>Pane to toggle.</dd>
<dt><code>on</code> · boolean | null · <em>default: <code>null</code></em></dt>
<dd><code>null</code> to toggle, <code>true</code>/<code>false</code> to set.</dd>
</dl>

</td>
<td valign="top">

<strong>Request</strong>
<pre>
{
  "type": "diffview",
  "pane": null,
  "on": null
}
</pre>

<strong>Response</strong> · <code>ok</code> | <code>error</code>
<pre>
{
  "type": "ok"
}
{
  "type": "error",
  "message": "nothing to diff"
}
</pre>

</td>
</tr>
</table>

Returns `error` when there is nothing to diff — no `git` binary, or a clean or untracked file.

### `comment`

Writes to a line's review thread from the agent side. `body` starts a thread on the line (or appends a reply to the human's thread there). A comment is a **call and response**: the human's `Shift+s` ships every thread as one review block and clears the pane, so nothing outlives a submit. An empty or absent `body` is a typed error.

<table class="proto">
<tr>
<td valign="top">

<dl>
<dt><code>pane</code> · integer · <em>default: focused pane</em></dt>
<dd>Pane to comment on.</dd>
<dt><code>line</code> · integer</dt>
<dd>1-based source line the thread hangs on (same mapping as <code>highlight</code>).</dd>
<dt><code>body</code> · string | null · <em>default: <code>null</code></em></dt>
<dd>Note text. Starts a thread or appends a reply. An empty or absent body is a typed error.</dd>
<dt><code>author</code> · string | null · <em>default: session agent</em></dt>
<dd>Attribution label for the note. When omitted, defaults to the session's <code>ready --agent</code> name, falling back to <code>agent</code> if the tab was never readied.</dd>
</dl>

</td>
<td valign="top">

<strong>Request</strong>
<pre>
{
  "type": "comment",
  "pane": null,
  "line": 42,
  "body": "handled — see the retry loop",
  "author": "agent"
}
</pre>

<strong>Response</strong> · <code>ok</code> | <code>error</code>
<pre>
{
  "type": "ok"
}
{
  "type": "error",
  "message": "no pane #3"
}
</pre>

</td>
</tr>
</table>

### `layout`

Requests the current layout without changing anything.

<table class="proto">
<tr>
<td valign="top">

<em>No parameters.</em>

</td>
<td valign="top">

<strong>Request</strong>
<pre>
{
  "type": "layout"
}
</pre>

<strong>Response</strong> · <code>report</code>
<pre>
{
  "type": "report",
  "area": {},
  "panes": [
    {
      "id": 0,
      "kind": "pty",
      "rect": {},
      "overflow_rows": 0,
      "clipped": false
    }
  ]
}
</pre>

</td>
</tr>
</table>

One `PaneReport` per pane, with `rect`, `content_rows`, `visible_rows`, `overflow_rows`, and `clipped`, so a client can measure fit.

### `ready`

Marks the tab as hosting an agent, which gates interactivity (see [In-process interactions](#in-process-interactions)).

<table class="proto">
<tr>
<td valign="top">

<em>No parameters.</em>

</td>
<td valign="top">

<strong>Request</strong>
<pre>
{
  "type": "ready"
}
</pre>

<strong>Response</strong> · <code>ok</code>
<pre>
{
  "type": "ok"
}
</pre>

</td>
</tr>
</table>

### `update`

Reserved for a re-render nudge; not yet emitted.

<table class="proto">
<tr>
<td valign="top">

<dl>
<dt><code>path</code> · string · <strong>required</strong></dt>
<dd>File whose pane to re-render.</dd>
</dl>

</td>
<td valign="top">

<strong>Request</strong>
<pre>
{
  "type": "update",
  "path": "spec.md"
}
</pre>

</td>
</tr>
</table>

## In-process interactions

Interactions inside the TUI run in-process and do not cross a process boundary, so they skip the socket entirely. A keypress handler holds the live layout state directly and calls the relevant code path instead of serializing a message to itself:

- review comment (`c`), collapse/expand threads (`r`), jump to the next/prev thread (`n`/`N`), submit (`Shift+s`), and refresh (`Ctrl+R`)
- focus (`^p`), scrolling, and diff toggle (`d`)

See [navigating the TUI](navigation.md) for the in-TUI keys for all interactions.

Interactivity is gated on `ready` and fails closed: until the tab has received a `ready` message declaring an agent, in-process interactions are unavailable.