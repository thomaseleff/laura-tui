# Protocol

The protocol is NDJSON over a socket, either a Windows named pipe or a Unix namespaced socket, that lets external processes tile panes and interact with a Laura workspace. All external interactions, including the `laura` CLI, flow through the protocol.

> [!TIP]
> Laura handles interactions in two ways: over the protocol, from an outside process, and in-process, inside the TUI. In-process interactions, like commenting on a line, scrolling, and submitting an inline review, don't use the protocol (see [In-process interactions](#in-process-interactions)).

## Addressing

Laura opens one socket for the workspace and sets `LAURA_TAB` to the socket's name in the shell. A client connects to the socket and sends a request.

The socket name includes the process id and a random nonce (`laura-<pid>-<nonce>-<n>`), so a new Laura process that reuses an old process id never reuses an old workspace's name. A client holding the `LAURA_TAB` of a closed workspace fails to connect instead of reaching a different, live workspace.

### Panes

The workspace's panes form a split tree: Laura adds a pane by splitting an existing pane in two. Laura gives each pane an integer id (`u64`), counting up within the workspace. The shell is always pane `0`. Laura never reuses an id within a workspace, so closing a middle pane leaves a gap (ids `0, 4` after closing `1..3`). Requests address panes by id.

## Messages

Each connection carries one request and one response: the client connects, sends a single request frame, reads a single response frame, and the socket closes. A *frame* is one JSON object on a single line (NDJSON). The response comes from the TUI process, which holds the live layout state.

Every message has a `type` field, such as `open` or `ok`, plus the fields listed under that message below, with their defaults. When a client reads the end of the stream with no response frame, the client treats the result as `ok`.

While the user types a comment or review body, Laura holds requests to the workspace and answers them once the user finishes or cancels.

### `open`

Split a pane and render a file in the new pane.

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
<dd>Replace this pane's content in place (same id, rect, focus) instead of splitting. Laura ignores <code>split</code>/<code>dir</code>/<code>ratio</code>/<code>side</code> when <code>panel</code> is set. Laura returns <code>error</code> for the shell, for an id with no pane, and while the pane has an unsubmitted inline review or unsaved Neovim edits.</dd>
<dt><code>edit</code> · boolean · <em>default: <code>false</code></em></dt>
<dd>Run Neovim on the file in an editor pane. Never moves focus, whatever <code>focus</code> says. With <code>panel</code> on the same file, attaches Neovim in place and keeps the pane's threads. Returns <code>error</code> with <code>highlight</code> or <code>diff</code>, which editor panes don't show, and with the reason when editor panes are off (see <a href="cli.md#editor-panes">Editor panes</a>).</dd>
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
  "diff": false,
  "panel": null,
  "edit": false
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

`opened` holds the new pane id, which `laura open` prints, and a list of warnings. Laura still opens the pane when it returns a warning. Laura returns these as warnings, not errors:

- `diff` when there is nothing to diff (`no changes vs HEAD — nothing to diff`).
- `already open in pane #N` when the file is already open in pane N, including on a `panel` replace. The warning ends with `, which has an unsubmitted inline review` when pane N has one, and reads `already open in editor pane #N` when pane N is an editor pane without one.

With `dry_run`, Laura returns a `report` instead of `opened`.

**Inference.** When `dir`, `ratio`, `side`, and `split` are all absent, Laura picks the split. Laura splits the newest pane at 50% and alternates the orientation by depth (a dwindle), so repeated bare opens each split off the newest pane. Setting any one of those fields turns inference off. When a split would shrink a pane below the minimum size Laura can render, Laura returns `error`.

### `close`

Close a pane, or close every pane and return the workspace to shell-only.

<table class="proto">
<tr>
<td valign="top">

<dl>
<dt><code>pane</code> · integer · <em>default: focused pane</em></dt>
<dd>Pane to remove.</dd>
<dt><code>all</code> · boolean · <em>default: <code>false</code></em></dt>
<dd>Return the workspace to shell-only.</dd>
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

<strong>Response</strong> · <code>ok</code> | <code>error</code>
<pre>
{
  "type": "ok"
}
</pre>

</td>
</tr>
</table>

Laura never closes the shell (pane `0`). Laura returns `error` when the pane has an unsubmitted inline review or unsaved Neovim edits. With `all`, Laura returns `error` and closes nothing when any pane has either.

### `focus`

Focus a pane by id.

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

Highlight a range of lines in a pane and scroll the lines into view.

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

<strong>Response</strong> · <code>ok</code> | <code>error</code>
<pre>
{
  "type": "ok"
}
</pre>

</td>
</tr>
</table>

On a frozen pane, Laura returns `error` for a `range` but still clears the highlight for `range: null`. On an editor pane, Laura returns `error` for both, so annotate the lines with [`comment`](#comment) instead.

Line numbers are the file's source lines, matching the gutter and the inline review's `L<n>`. In markdown:

- List items, blockquote lines, and callout lines keep one number per line, including blank lines.
- Laura shows a hand-wrapped paragraph as one block, and highlights the whole block for any of the paragraph's lines.
- Laura does the same for a fenced block or table inside a list item or blockquote.

The highlight stays until one of these happens:

- The agent sets a new highlight or clears it (`range: null`).
- The user presses `h` on the focused pane.
- The agent turns the file pane into an editor pane (`open --edit` on the same file).

Moving focus or the cursor doesn't change the highlight. Laura clamps out-of-range lines to the file, including after a reload that shortens the file. Clearing the highlight doesn't move the cursor or scroll the pane.

### `diffview`

Toggle a pane's inline diff view against git `HEAD`.

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

Laura returns `error` when there is nothing to diff (`git` isn't installed, or the file is clean or untracked). Laura also returns `error` on an editor pane, so annotate the lines with [`comment`](#comment) instead.

### `comment`

Annotate a line: start a thread on the line, or add a reply to the user's thread there. When the user presses `Shift+S`, Laura submits every thread as one inline review and clears the pane's threads. Laura returns `error` for an empty or absent `body`.

<table class="proto">
<tr>
<td valign="top">

<dl>
<dt><code>pane</code> · integer · <em>default: focused pane</em></dt>
<dd>Pane to comment on.</dd>
<dt><code>line</code> · integer</dt>
<dd>1-based source line of the thread (same mapping as <code>highlight</code>).</dd>
<dt><code>body</code> · string | null · <em>default: <code>null</code></em></dt>
<dd>Comment text. Starts a thread or appends a reply. An empty or absent body returns <code>error</code>.</dd>
<dt><code>author</code> · string | null · <em>default: session agent</em></dt>
<dd>Attribution label for the comment. When omitted, defaults to the session's <code>ready --agent</code> name, falling back to <code>agent</code> if the workspace was never readied.</dd>
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

Return the current layout without changing anything.

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

The report has one entry per pane, with `rect`, `content_rows`, `visible_rows`, `overflow_rows`, and `clipped`, so a client can check whether the content overflows the pane. `kind` is `pty` (the shell), `panel` (a file pane), or `editor` (an editor pane, with its `path`). Editor panes report `content_rows: null`, because Neovim sizes its own content.

### `ready`

Mark the workspace as hosting an agent, which enables commenting and submitting (see [In-process interactions](#in-process-interactions)).

<table class="proto">
<tr>
<td valign="top">

<dl>
<dt><code>session</code> · string | null · <em>default: <code>laura-&lt;pid&gt;-&lt;n&gt;</code></em></dt>
<dd>Names the journal session.</dd>
<dt><code>agent</code> · string | null · <em>default: <code>null</code></em></dt>
<dd>Attributes journal events, and the agent's comments, to this name.</dd>
</dl>

</td>
<td valign="top">

<strong>Request</strong>
<pre>
{
  "type": "ready",
  "session": "demo",
  "agent": "claude"
}
</pre>

<strong>Response</strong> · <code>ready</code>
<pre>
{
  "type": "ready",
  "journal": "~/.laura/sessions/&lt;session&gt;.ndjson",
  "experimental": ["editor"],
  "layout": { "area": {}, "panes": [ … ] }
}
</pre>

</td>
</tr>
</table>

`journal` is the path to the session's journal. `experimental` lists the experimental features enabled in the workspace (`editor`: editor panes, see the [CLI reference](cli.md#experimental-features)), and is empty when none are enabled. `layout` is the same report that [`layout`](#layout) returns. `layout` is `null` when the running Laura is older than the `laura` CLI and doesn't send the field.

### `update`

Reserved for asking a file pane to reload. The `laura` CLI doesn't send `update` yet, and Laura answers it with `ok`.

<table class="proto">
<tr>
<td valign="top">

<dl>
<dt><code>path</code> · string · <strong>required</strong></dt>
<dd>File whose pane to reload.</dd>
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

Laura handles the user's keys inside the TUI process, which already holds the layout, so in-process interactions never use the socket. The in-process interactions are:

- comment (`c`), collapse/expand threads (`r`), jump to the next/prev thread (`n`/`N`), submit (`Shift+S`), and refresh (`Ctrl+R`)
- focus (`Ctrl+P`), scrolling, and diff toggle (`d`)

See [navigating the TUI](navigation.md) for the in-TUI keys for all interactions.

Until the workspace receives a `ready` message, the user can't comment or submit an inline review.