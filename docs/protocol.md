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

Split a pane and render a file in the new pane, or show a page from a URL in a browser pane.

<table class="proto">
<tr>
<td valign="top">

<dl>
<dt><code>path</code> · string · <strong>required</strong></dt>
<dd>File to render, or an <code>http://</code> or <code>https://</code> URL for a <a href="web-app.md#show-a-web-app-in-a-browser-pane">browser pane</a> (experimental). With a URL, Laura returns <code>error</code> with <code>highlight</code>, <code>diff</code> or <code>edit</code>, and ignores <code>follow</code>. With <code>panel</code> on a browser pane, Laura loads the URL in the same pane.</dd>
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
- ``already open in pane #N — use `laura open <url> --panel N` `` when the URL is already open in browser pane N.
- `already open in pane #N` when the file is already open in pane N, including on a `panel` replace. The warning ends with `, which has an unsubmitted inline review` when pane N has one, and reads `already open in editor pane #N` when pane N is an editor pane without one.

With `dry_run`, Laura returns a `report` instead of `opened`.

**Inference.** When `dir`, `ratio`, `side`, and `split` are all absent, Laura picks the split. Laura splits the newest pane and alternates the orientation by depth (a dwindle), so repeated bare opens each split off the newest pane. Laura sizes the new pane as follows:

- In a side-by-side split, the new pane gets 50%.
- In a stacked split, the new pane gets the rows of its file, up to 50%.
- With `follow`, `edit` or `diff`, or for an HTML page or a markdown file in the web app, the new pane gets 50%.

Setting any one of those fields turns inference off. When a split would shrink a pane below the minimum size Laura can render, Laura returns `error`.

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

Highlight a range of lines in a pane and scroll the lines into view. In a browser pane, highlight the element that a CSS selector finds.

<table class="proto">
<tr>
<td valign="top">

<dl>
<dt><code>pane</code> · integer · <em>default: focused pane</em></dt>
<dd>Pane to highlight.</dd>
<dt><code>range</code> · [int, int] · <em>default: <code>null</code></em></dt>
<dd>Lines <code>[start, end]</code>, 1-based inclusive, in a file pane.</dd>
<dt><code>selector</code> · string · <em>default: <code>null</code></em></dt>
<dd>CSS selector of an element on the page that a browser pane shows. With no <code>range</code> and no <code>selector</code>, Laura clears the pane's highlight.</dd>
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

<strong>Browser pane</strong>
<pre>
{
  "type": "highlight",
  "pane": 2,
  "selector": "main > h1"
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

<strong>Response</strong> · <code>ok</code> | <code>warning</code> | <code>error</code>
<pre>
{
  "type": "ok"
}
{
  "type": "warning",
  "message": "pane #2 is paused — …"
}
</pre>

</td>
</tr>
</table>

On a frozen pane, Laura returns `error` for a `range` but still clears the highlight for `range: null`. On an editor pane, Laura returns `error` for both, so annotate the lines with [`comment`](#comment) instead.

On a paused pane, Laura highlights the `range` and returns `warning`, because the user doesn't see new output in the pane: `pane #N is paused — the user doesn't see new output, so Laura highlights the content that the pane holds`. `laura highlight` prints the warning and exits 0.

Line numbers are the file's source lines, matching the gutter and the inline review's `L<n>`. In markdown, Laura maps source lines to rows as the [agent reference](agent-reference.md#highlight-a-section-for-the-user) describes.

The highlight stays until one of these happens:

- The agent sets a new highlight or clears it (no `range` and no `selector`).
- The user presses `h` on the focused pane.
- The agent turns the file pane into an editor pane (`open --edit` on the same file).

Moving focus or the cursor doesn't change the highlight. Laura clamps out-of-range lines to the file, including after a reload that shortens the file. Clearing the highlight doesn't move the cursor or scroll the pane.

A file pane takes a `range` and a browser pane takes a `selector`. Laura returns `error` for the other one, and for a selector that matches no element. [Browser panes](cli.md#browser-panes) lists each error.

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

Annotate a line: start a thread on the line, or add a reply to the user's thread there. In a browser pane, annotate the element that a CSS selector finds. When the user presses `Shift+S`, Laura submits every thread as one inline review and clears the pane's threads. Laura returns `error` for an empty or absent `body`.

<table class="proto">
<tr>
<td valign="top">

<dl>
<dt><code>pane</code> · integer · <em>default: focused pane</em></dt>
<dd>Pane to comment on.</dd>
<dt><code>line</code> · integer · <em>default: <code>null</code></em></dt>
<dd>1-based source line of the thread (same mapping as <code>highlight</code>), in a file pane.</dd>
<dt><code>selector</code> · string · <em>default: <code>null</code></em></dt>
<dd>CSS selector of an element on the page that a browser pane shows. A file pane takes a <code>line</code> and a browser pane takes a <code>selector</code>. Laura returns <code>error</code> for the other one, as <a href="cli.md#browser-panes">Browser panes</a> lists.</dd>
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

<strong>Browser pane</strong>
<pre>
{
  "type": "comment",
  "pane": 2,
  "selector": "#email",
  "body": "Add a label to this field"
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

The report has one entry per pane, with `rect`, `content_rows`, `visible_rows`, `overflow_rows`, and `clipped`, so a client can check whether the content overflows the pane. In the web app, a pane's border takes three rows, so `visible_rows` is the pane's height minus three. `kind` is `pty` (the shell), `panel` (a file pane), `editor` (an editor pane, with its `path`), or `browser` (a browser pane, with its URL as `path` and the page's `title`). A pane that the user paused also has `"paused": true`. Editor panes and browser panes report `content_rows: null`, because Neovim and Laura's browser size their own content.

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
  "experimental": ["web", "browser"],
  "browser_url": "http://127.0.0.1:&lt;port&gt;",
  "layout": { "area": {}, "panes": [ … ] }
}
</pre>

</td>
</tr>
</table>

`journal` is the path to the session's journal. `experimental` lists the experimental features enabled in the workspace (`editor`: editor panes; `web`: the [web app](web-app.md); `browser`: [browser panes](cli.md#browser-panes), listed only when the web app runs and Laura found Chrome or Edge; see the [CLI reference](cli.md#experimental-features)), and is empty when none are enabled. `browser_url` is the shell's `LAURA_BROWSER_URL`, where the agent's browser tools connect to Laura's browser (see [Work on a page with the agent](web-app.md#work-on-a-page-with-the-agent)). `browser_url` is set only when `experimental` lists `browser`, and is `null` otherwise. `layout` is the same report that [`layout`](#layout) returns. `layout` is `null` when the running Laura is older than the `laura` CLI and doesn't send the field.

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

Laura handles the user's keystrokes inside the TUI process, which already holds the layout, so in-process interactions never use the socket. The in-process interactions are:

- comment (`c`), collapse/expand threads (`r`), jump to the next/prev thread (`n`/`N`), submit (`Shift+S`), and refresh (`Ctrl+R`)
- focus (`Ctrl+P`), scrolling, diff toggle (`d`), and pause/resume a tail pane (`p`)

See [navigating the TUI](navigation.md) for the in-TUI keystrokes for all interactions.

The user's actions in the TUI or the [web app](web-app.md) aren't protocol messages, and the agent can't send them.

Until the workspace receives a `ready` message, the user can't comment or submit an inline review.