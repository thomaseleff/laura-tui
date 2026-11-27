# The web app

Rules for `src/web/`: the web app's server (`mod.rs`, `frames.rs`, `html.rs`, `glance.rs`) and its client (`client/`). The root [`CLAUDE.md`](../../CLAUDE.md) holds the rules for all of Laura.

## Client modules

The client is ES modules with no build step. `index.html` loads `main.js` as `type="module"`, and Laura serves each module from `ASSETS` (`mod.rs`). The vendor scripts (`xterm.js`, its addons and `idiomorph.js`) stay classic scripts with their globals.

- `state.js`: Laura's layout, the socket, `send` and `act`, the wait rule, `esc`, `ui`, and the modes with `mode()`. Every module imports `state.js`, and `state.js` imports nothing.
- `term.js`: the shell's and each editor pane's xterm.js, `termKeys`, the shell's links (`PATH`), and the grid's cell.
- `head.js`: the pane header.
- `panes.js`: each pane's element, `onLayout`, the lines between panes, the start canvas and the project at a glance, the drawer and its grip and start height, a browser pane's pointer, `focusPane` and `followFocus`.
- `browser-pane.js`: a browser pane's screen frames, live badge, marks, pins and dialog.
- `bottom.js`: the notice, the hint line, the status bar and the shell button.
- `chat.js`: the chat in the drawer, its rows' clicks, and the links and the file paths in a say.
- `review.js`: threads, the comment's box, the popovers and the draft.
- `pages.js`: a file pane's rows and pages, comment mode, and a page's pins.
- `selector.js`: the pane selector and the help.
- `keys.js`: the keydown listener, one function for each mode and each kind of pane, and the window's paste, copy and resize.
- `workspaces.js`: the workspace switcher, its menu, the browser tab, and the overlay.
- `main.js`: `connect`, which hands each frame to its module, and the start.

Every focus move the user makes calls `focusPane` (`panes.js`), which tells Laura. A route that moves only the DOM focus calls `followFocus`, which follows Laura's focus.

Laura holds the modes in `Tab` (`src/tab/mode.rs`): the help, the pane selector, the quit question, the lock and the draft. The `layout` frame lists them in `modes`, with the open draft in `draft`. Only the overlay on close and the workspace switcher's menu live in the client. `modes()` lists every mode that is on, first to last: the client's two, then Laura's. `mode()`, its first, takes a keystroke. The keydown listener and `termKeys` ask `mode()`. `ui` holds only what the client draws of a mode, such as the pane selector's row and the draft's text.

A keystroke that changes a mode sends its `act` and patches the client's copy of `layout.modes` (`act`'s `patch`), so the next keystroke goes to the new mode before Laura's reply. The copy holds until Laura's `acted` for that act; a `layout` frame from before it keeps the patch. What a rule decides, such as the focus or the end of the draft, comes only from Laura.

The hint line shows the first mode in `modes()` that has a hint, so under the help and the overlay the hint line shows the mode below them. A new mode is a case of `Mode` in `mode.rs`, and of `modes()` only when it lives in the client.

A top-level `let` lives in the module that assigns it, and other modules read it through an import. When another module must change it, the owner exports a small function for that change, such as `setLayout`. Top-level code must not call an imported `const` function: a module in an import cycle can run before that function exists.

## Frames

Laura and the web app talk over one websocket per view. Each text frame is a JSON object, and its `t` names the frame.

Laura sends these frames to the view:

| Frame (`t`) | What Laura sends it for | Who sends it | What the client paints with it |
|---|---|---|---|
| `strings` | The help, the hint texts and the overlay texts, once per view | `WebServer::sync`, on connect | `paintHelp`, `paintSwitcher` |
| `workspaces` | The running workspaces, when the list changes | the thread in `WebServer::watch` | `paintSwitcher` |
| `layout` | The panes, their rects, the focus, the `modes` on and the open `draft`, the hint line (with `hints.mode`, the first mode's), the status bar, the `canvas` while no pane but the shell is open, the drawer's height with its `drawer_fit`, the drawer's view (`drawer_view`: `chat` or `shell`), and the `splits` the user drags (`Tab::handles`) | `layout_json` (`frames.rs`) | `onLayout` |
| `glance` | The project at a glance: the README as HTML, the git changes or latest commit, and the recent sessions, when the thread that reads it finishes, and again each time the canvas comes back. Until then the client shows skeleton cards | `send_canvas` (`mod.rs`), from `Glance` (`glance.rs`) | `paintGlance` |
| `chat` | The chat's rows (`tab/chat.rs`), with each say's HTML, when a row changes | `send_chat` (`frames.rs`) | `onChat` |
| `pane` | A file pane's rows as HTML, in full | `send_panes` (`frames.rs`) | `onPane` |
| `rows` | Only the rows of a file pane that changed | `send_panes` (`frames.rs`) | `onPane` |
| `page` | An HTML page in a file pane | `send_panes` (`frames.rs`) | `onPage` |
| `browser` | A browser pane's URL and state | `send_browsers` (`frames.rs`) | `onBrowser` |
| `editable`, `agent`, `screenshot`, `copy`, `rect`, `marks` | Events in a browser pane | `browser.rs`, through `take_out`; `sync` adds the pane `id` | their `case` in `connect` |
| `acted` | The reply to each `act`: `ok`, the text of an error, `retry` for a failed submit, and a begun comment's seed | `WebServer::sync` | the `acts` queue |
| `notice` | A notice, on a pane when the frame has `pane` | `WebServer::sync` | `showNotice` |
| `moved` | A newer view took over the workspace | `WebServer::sync` | the overlay on close |
| `exit` | The workspace ended | `WebServer::close` | the overlay on close |

A binary frame is `framed`: an 8-byte big-endian pane id, then the bytes. For the shell, the bytes are PTY output (`terms.get(id).write`). For a browser pane, the bytes are a screen frame (`onFrame`).

The view sends these frames to Laura, and `sync` reads each one in its inbound branch: `act` (a `UserAction`), `visit`, `view`, `shell`, `size`, `ack` and `quit`.

## Escaping

The client puts each string into the DOM through `esc` (`state.js`). Only the HTML of Laura's `pane` and `rows` frames, the README's HTML in the `glance` frame, and a say's HTML in the `chat` frame, goes in as markup. `html.rs` and `render.rs` build that HTML, and escape the file text.

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

The web tests are one test binary, `tests/web/`, with one module for each feature: `views`, `layout`, `shell`, `chat`, `file_panes`, `markdown`, `pages`, `review`, `modes`, `start`, `glance` and `links`. The `//!` paragraph of each module tells what the module covers.

- Add a web test to the module for its feature.
- Put a helper that one module calls in that module, and a helper that two or more modules call in `main.rs`.

### Client tests

The client tests run the web app's client. They are one test binary, `tests/client.rs`, and their rig is `tests/common/client.rs`. The rig's `//!` paragraph and the docs of its methods tell how a test opens the page, presses keys and waits.

A change to a keystroke, a focus move or a rule in the client adds a client test. A client test for a known bug is `#[ignore]`, and the reason names the issue that fixes the bug.

#### Waits

A client test that waits for the wrong state passes on a fast machine and fails at random in CI. Each of these rules comes from a test that failed that way.

- **Wait for a state that only Laura's reply makes.** The client patches its copy of the modes at once (`act`'s `patch`), so `layout.modes` is true before Laura reads the act. To wait for Laura, wait also for `!acts.length`, or for what the reply does, such as the focus that `followFocus` moves (`Client::focus`, the drawer's grip test in `tests/browser.rs`).
- **Measure only a layout at the page's size.** Laura can send the first layout before the page reports its size, and the next layout moves the panes. Before you read a rect, wait until the layout reaches the page's `--cols` (`split_center`, `editor_beside`).
- **Change the page's size only with `Client::resize`.** It waits for Chrome's `resize` event, which Chrome fires at its next rendering step, also after the test's next check under load.
- **Test a stale frame by replaying it.** The rig runs Laura only on `pump`, so the acts that the test sent stay unanswered until the next `until`. Send the keystrokes with no `until` between them, then replay Laura's old frame through `ws.onmessage` (`a_stale_layout_leaves_the_closed_pane_selector_closed`). Do not wait for a timing to happen.

`LINKS` in `tests/client.rs` lists the shell's lines and the links in each. A change to `PATH` or to the shell's link provider adds rows to `LINKS`.

The rig's JS, and the browser tests' JS (`tests/browser.rs`), read some of the client's names. A module's names are not globals, so `state.js` puts these names on `globalThis`, and the note above those lines names them. A test that needs another name reads the DOM, or adds the name to that note and those lines.
