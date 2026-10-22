---
name: record-demo
description: Record the README demo GIF of the Laura web app. Use only when the user asks for a new README demo.
when_to_use: The user asks you to record the README demo, the demo video or the demo GIF. Do not use this skill while you work on a feature. Do not use this skill only because `plugins/laura/skills/demo/` changed.
user-invocable: true
---

# Record the README demo

This skill records one video, the README demo `docs/assets/laura-demo.gif`. The README and `docs/index.md` show this GIF. There are no other demo scenes.

`scripts/demo/record.py` plays the scene `scripts/demo/scenes/demo.py` in the web app. The scene follows the beats of `plugins/laura/skills/demo/SKILL.md`, with the same `agent.say` text and `laura` commands. A beat can use shorter text, so that the video does not become slow.

The scene has three actors:

- **The user** presses keystrokes in the browser. Playwright operates Chromium. A box in the bottom-right corner shows each keystroke.
- **The agent** writes chat replies and runs `laura` commands. A chat stub (`scripts/demo/chat.py`) runs as the shell program in the workspace. The chat stub looks like Claude Code in auto mode.
- **The camera** records the browser.

The recorder works on Windows only.

## Setup

Do these steps one time:

1. Install `uv`.
2. Install Chromium for Playwright: `uv run --with playwright==1.63.0 playwright install chromium --no-shell`.
3. Add Neovim to `PATH`, because the demo has an editor pane.

Before you record, build the debug binary: `CARGO_TARGET_DIR=tmp/target cargo build`. To use a different binary, add `--laura <path>`.

## Record

When your partner asks for a new README demo:

1. Make `demo.py` agree with the demo skill. Change the scene only at this step. Do not change the scene together with a feature change to the demo skill.
2. Run the recorder:

   ```bash
   uv run scripts/demo/record.py scripts/demo/scenes/demo.py
   ```

3. If the run fails, read `tmp/demo/demo/failure.png` and the traceback. The traceback shows the line of the scene that failed. If the web app has a page error, the run also fails.
4. You cannot watch a video, so read each `camera.shot` PNG instead. Make sure that the frame, the fit of each pane and the keystroke box are correct.
5. Write `tmp/demo/demo/review.md`. Put each PNG in it with a heading, and add a link to `demo.mp4`.
6. Run `laura open tmp/demo/demo/review.md`, so that your partner can comment on each shot.
7. Change the scene as the inline review tells you. Then record again.
8. When your partner approves the video, copy `tmp/demo/demo/demo.gif` to `docs/assets/laura-demo.gif`.

`record.py` writes these files to `tmp/demo/demo/`:

- `demo.mp4` and `demo.gif`.
- One PNG for each `camera.shot`.
- `laura.log`, the output of Laura.
- `home/acme-api/`, the scratch repo. The top bar shows `~/acme-api`. `record.py` copies the fixtures in `scripts/demo/scenes/demo/` into the scratch repo and commits them.

## Change the scene

`demo.py` has one async function, `scene(user, agent, camera)`. The scene uses these helpers:

| Helper | Action |
|---|---|
| `agent.say(text)` | Shows the text in the chat as one agent reply, then holds the frame for sufficient time to skim the text. A bar along the top edge shows the time until the next action. |
| `agent.laura(*args, stdin=lines, shown=None)` | Runs `laura <args>` from the chat and gives the stdout, for example the pane id. If the exit code is not 0, raises an error. `stdin` sends the lines one at a time, at 0.3 s intervals, for `laura tail`. `shown` is the command line that the chat shows instead, for example `python worker.py 2>&1 \| laura tail …`. |
| `agent.wait_for_review()` | Waits until a submitted inline review comes into the chat. Gives the text of the review. |
| `agent.write(path, text)` | Writes the file in the scratch repo. The chat shows `Update(<path>)` with the count of added and removed lines, or `Write(<path>)` for a new file. |
| `agent.edit(path, old, new)` | Replaces `old` with `new` in the file, as `agent.write` does. If `old` is not in the file, raises an error. |
| `agent.repo` | The `Path` of the scratch repo. Write files after the commit to show changes in the gutter and in `--diff`. |
| `user.focus(pane)` | Moves the focus to the pane from the keyboard. The user presses `Ctrl+P`, waits one beat with the pane selector open, and types the pane id. From a file pane to the shell, the user presses `Esc`. If the pane has the focus, does nothing. |
| `user.goto_line(pane, line)` | Focuses the file pane, then presses `↓` or `↑` until the cursor is on the line. If the cursor cannot reach the line, raises an error. |
| `user.press(key)` | Presses a keystroke and shows it in the keystroke box. After `c` or `Shift+S`, waits for the comment box or the review body. After `Enter` in a box, waits until the box closes. |
| `user.type(text, keys=False)` | Types the text at a readable speed. Use `type`, not `press`, for keystrokes to Neovim. With `keys=True`, the keystroke box shows the text, for a Neovim command such as `]d` or `:w`. |
| `user.click(locator)` | Moves an arrow pointer to a Playwright locator or a CSS selector, then clicks. |
| `user.focused(pane)` | Waits until the pane has the focus. Use `user.focused` after `user.click` and before keystrokes. A keystroke before the focus moves goes to the pane that had the focus. |
| `user.page` | The Playwright `Page`, for all other actions. `user.page.get_by_label(...)` finds a pane header button. |
| `camera.shot(name)` | Writes `<name>.png`. |
| `camera.pause(ms)` | Holds the frame. |

Obey these rules when you change the scene:

- Do each action from the keyboard, so that the keystroke box shows it. Use `user.click` only for an action with no keystroke, such as a click on an HTML page.
- Each pane is `#p<id>`. To wait for a change in the web app, wait for a selector, for example `user.page.locator(f"#p{doc} .head .reloaded").wait_for(state="visible")`. Do not use `page.wait_for_function`, because the CSP of the web app blocks it.
- In the editor view, the web app sends all keystrokes to Neovim, except `Ctrl+P` and `Ctrl+L`. Neovim draws on a canvas. To examine an edit, read the file from `agent.repo` after `:w`. Use `camera.pause` to wait for Neovim.
- Write each `agent.say` text in the voice of `plugins/laura/skills/demo/SKILL.md`. Obey the Writing rules and the Glossary in `CLAUDE.md`.
- Keep panes wide. Stack panes with `--dir v`, because the web app hides the file of a narrow pane.
