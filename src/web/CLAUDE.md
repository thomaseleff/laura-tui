# The web app

Rules for `src/web/`: the web app's server (`mod.rs`, `frames.rs`, `html.rs`) and its client (`client/`). The root [`CLAUDE.md`](../../CLAUDE.md) holds the rules for all of Laura.

## Frames

Laura and the web app talk over one websocket per view. Each text frame is a JSON object, and its `t` names the frame.

Laura sends these frames to the view:

| Frame (`t`) | What Laura sends it for | Who sends it | What `app.js` paints with it |
|---|---|---|---|
| `strings` | The help, the hint texts and the overlay texts, once per view | `WebServer::sync`, on connect | `paintHelp`, `paintSwitcher` |
| `workspaces` | The running workspaces, when the list changes | the thread in `WebServer::watch` | `paintSwitcher` |
| `layout` | The panes, their rects, the focus, the hint line and the status bar | `layout_json` (`frames.rs`) | `onLayout` |
| `pane` | A file pane's rows as HTML, in full | `send_panes` (`frames.rs`) | `onPane` |
| `rows` | Only the rows of a file pane that changed | `send_panes` (`frames.rs`) | `onPane` |
| `page` | An HTML page in a file pane | `send_panes` (`frames.rs`) | `onPage` |
| `browser` | A browser pane's URL and state | `send_browsers` (`frames.rs`) | `onBrowser` |
| `editable`, `agent`, `screenshot`, `copy`, `rect`, `marks` | Events in a browser pane | `browser.rs`, through `take_out`; `sync` adds the pane `id` | their `case` in `connect` |
| `acted` | The reply to each `act`: `ok`, the text of an error, and `retry` for a failed submit | `WebServer::sync` | the `acts` queue |
| `notice` | A notice, on a pane when the frame has `pane` | `WebServer::sync` | `showNotice` |
| `moved` | A newer view took over the workspace | `WebServer::sync` | the overlay on close |
| `exit` | The workspace ended | `WebServer::close` | the overlay on close |

A binary frame is `framed`: an 8-byte big-endian pane id, then the bytes. For the shell, the bytes are PTY output (`terms.get(id).write`). For a browser pane, the bytes are a screen frame (`onFrame`).

The view sends these frames to Laura, and `sync` reads each one in its inbound branch: `act` (a `UserAction`), `visit`, `view`, `shell`, `size`, `ack`, `hold` and `quit`.

## Escaping

The client puts each string into the DOM through `esc` (`app.js`). Only the HTML of Laura's `pane` and `rows` frames goes in as markup. `html.rs` builds that HTML, and escapes the file text with `render::escape`.

## Parity

Before a change to the web app, ask: does the TUI need the same change?

- If yes, put the rule in the domain that owns the state, and let both front-ends call it (root `CLAUDE.md`, "One rule, one place").
- If no, write in the PR body why the TUI does not get the change.

## Tests

`tests/common/web.rs` drives the web app as a view does:

- `Rig::new` (or `Rig::host` with a `Tab`) hosts a workspace in-process, and `pump` runs one turn of its loop.
- `connect` opens a view.
- `until` waits for a frame, and `idle` collects the frames of some turns.
- `act` sends an `act` and returns its `acted`.
- `run` and `drive` call the `laura` CLI against the workspace.

Assert on frames, not pixels.

The web tests are one test binary, `tests/web/`, with one module for each feature: `views`, `layout`, `shell`, `file_panes`, `markdown`, `pages`, `review`, `start` and `links`. The `//!` paragraph of each module tells what the module covers.

- Add a web test to the module for its feature.
- Put a helper that one module calls in that module, and a helper that two or more modules call in `main.rs`.

### Client tests

The client tests run `app.js` itself. They are one test binary, `tests/client.rs`, and their rig is `tests/common/client.rs`:

- `browser_rig::bare()` hosts the workspace in-process, so a test reads Laura's state through `r.tab` and calls the CLI with `r.drive`.
- `Client::open` loads the web app in a headless Chrome of the test's. The page is the workspace's only view: don't `connect` a second view, which would take the workspace over.
- `press` and `type_text` give the page the user's keys, and `focus` focuses a pane with the pane selector.
- `until` pumps Laura until a JS expression on the page is truthy.

A change to a keystroke, a focus move or a rule in the client adds a client test. A client test for a known bug is `#[ignore]`, and the reason names the issue that fixes the bug.

The rig's JS reads the client's top-level names `layout`, `terms` and `PATH`. A change that moves them, such as a split of `app.js` into ES modules, keeps them on `globalThis` (for example, `globalThis.terms = terms`).
