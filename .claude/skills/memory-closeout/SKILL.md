---
name: memory-closeout
description: Review a merged branch's Entire memories in a Laura browser pane, then retract and promote them into main.
when_to_use: The user asks you to close out a branch's memories, to review or promote memories, or to clear the memory review queue.
user-invocable: true
argument-hint: [branch]
---

# Memory close-out

## Getting started

Use this section only when the user asks how to use this skill.

To start a close-out, type `/memory-closeout [branch]`. Examples:

- `/memory-closeout` lists the merged branches with memories that `main` does not have, and asks you to pick one.
- `/memory-closeout feat/browser-panes-107` closes out that branch.

You open the close-out page in a browser pane. The page has two steps:

1. **Branch.** The user selects **Keep** or **Retract** for each new memory, and an action for each proposal, then clicks **Done**. You retract and promote into `main`.
2. **Main.** The same page reloads with the proposals that the promote created on `main`. The user selects an action for each proposal, then clicks **Done**. You apply the actions.

In each step, the **Main** tab lists all of the memories on `main`. The user can retract a memory on `main` from that tab.

## Background

- Entire Brain keeps one memory store for each branch. A new branch starts with a copy of the `main` store. A git merge does not move memories into `main`.
- `entire brain memories promote` copies the active memories of a branch into `main`. A memory keeps its id, so a second promote adds nothing.
- A **proposal** says that a new memory duplicates an existing memory (`merge`) or supersedes it (`supersede`). The new memory stays a `candidate` until the user applies or rejects the proposal. Promote does not copy a candidate. The page shows proposals in the **Overlaps** tab.
- The bundled app `.claude/skills/memory-closeout/app.py` serves `page.html` for one step, `branch` or `main`. When the user clicks **Done**, the app writes the actions to the `--out` file and exits. The page then waits for the next step on the same port.
- `--print` writes the data of a step as JSON and exits. Use it to read the rows without the page.
- In this skill, `<B>` is the branch, and `<b__>` is the branch with each `/` changed to `__`.

## Workflow

### Prepare

1. Run `laura ready`. If the output does not list `browser`, stop. Tell the user to start the web app with browser panes, as `docs/web-app.md` tells.
2. Pick the branch:
   - If the user gives a branch, use it.
   - Else, if the current branch is not `main`, use the current branch.
   - Else, list the branches from `entire brain memories status --all-branches --json`, in `branches[].branch`. Ignore `main`. For each branch, run the app with `branch --print`. Show the branches that have rows in `memories` or `overlaps`, with their PR state. Ask the user to pick one.
   - Do not use the counts from `memories status`. They do not agree with the store.
3. Get the PR state: `gh pr list --head <B> --state all --json number,state`. If the PR is open, stop. Tell the user to close out the branch after the merge.
4. Run `entire brain distill --dry-run --json`. If `branches[]` shows `sessions_to_distill` more than 0 for `<B>`, tell the user that distill spends tokens. Then run `entire brain distill --branch <B>`.

### Step 1: branch

5. Serve the `branch` step for `<B>` in a new pane (see "Serve a step"). Keep the port of the URL for step 2.
6. Read the actions from `tmp/memory-closeout/<b__>-branch.json`. Apply them in this sequence:
   1. For each id in `overlaps`:
      - `apply`: `entire brain memories review --branch <B> --apply <id>`.
      - `reject`: `entire brain memories review --branch <B> --reject <id>`.
      - `drop`: reject the proposal as for `reject`, then `entire brain memories retract <id> --branch <B>`.
      - Do nothing for an id that is not in the file. The proposal stays in the queue, and promote does not copy the candidate.
   2. For each id in `memories`: `entire brain memories retract <id> --branch <B>`.
   3. For each id in `main`: `entire brain memories retract <id> --branch main`.
   4. Promote: `entire brain memories promote --from <B> --into main --json`.
   5. If `delete_local_branch` is `true`, delete the branch: `git branch -D <B>`. The app shows this option only when the PR is merged and `<B>` is not the current branch.
7. If a command fails, stop. Do not serve step 2. The page stays on "Applying the changes". Show the error to the user.

### Step 2: main

8. Serve the `main` step on the port from step 5 (see "Serve a step"). Do not open a new pane. The open page reloads into step 2. Serve step 2 also when `main` has no proposals, because the page waits for it.
9. Read the actions from `tmp/memory-closeout/<b__>-main.json`. Apply them with `--branch main`:
   1. For each id in `overlaps`: apply, reject or drop, as in step 6.
   2. For each id in `main`: `entire brain memories retract <id> --branch main`.
10. Close the pane: `laura close $P`.
11. Run `entire brain patterns skills`. List the candidates, if there are any. Do not run `patterns skills form` unless the user asks.

### Serve a step

1. Read the rows: `python3 -I .claude/skills/memory-closeout/app.py <step> --branch <B> --out /dev/null --print`.
2. Write a recommendation for each row in `memories` and `overlaps` to `tmp/memory-closeout/<b__>-<step>-advice.json`. Use the form `{"<id>": {"pick": "<action>", "why": "<one sentence>"}}`.
   - In `memories`, `pick` is `keep` or `retract`. Recommend `retract` for a memory that is not true now, or that records only a step of the task.
   - In `overlaps`, `pick` is `apply`, `reject`, `drop` or `skip`. Recommend `apply` only when the new memory keeps all of the facts of the existing memory. Recommend `drop` when the existing memory already tells all of the new memory.
   - In `main`, recommend only `retract`, for a memory that is not true now. Do not write advice for the other rows.
   - Write the sentence in ASD-STE100. Tell the fact that decides the action.
3. Run the app with the Bash tool, with `run_in_background: true`. In step 2, add `--port <port>`.

   ```
   python3 -I .claude/skills/memory-closeout/app.py <step> --branch <B> --out tmp/memory-closeout/<b__>-<step>.json --advice tmp/memory-closeout/<b__>-<step>-advice.json
   ```

4. Read the first line of the app's output. It is the URL.
5. In step 1 only, open the URL in a browser pane: `P=$(laura open <url>)`.
6. Tell the user what to select, and to click **Done** at the end. The page shows your recommendation below the buttons of each row. The page does not select it.
7. Wait. Do not poll. The harness tells you when the app exits.

If the app exits with an error, show the error to the user and stop. Do not retract or promote.

## Set up the nightly job

Use this section only when the user asks you to set up the nightly job. Each step changes the machine. Ask the user before each step.

The nightly job `.claude/skills/memory-closeout/nightly.sh` runs at 02:00:

- It distills `main`, with a budget of 100 sessions for each night.
- It distills each branch whose PR is merged. It skips a branch with an open PR or with no PR.
- It then runs `entire brain patterns verify --deep`. Then `patterns skills` can show skill candidates.
- It writes its output to `~/.local/state/entire/logs/nightly.log`.

1. Look at the plan without a distill: `DRY_RUN=1 .claude/skills/memory-closeout/nightly.sh`.
2. Link the units: `ln -s "$PWD"/.claude/skills/memory-closeout/entire-nightly.{service,timer} ~/.config/systemd/user/`.
3. Start the timer: `systemctl --user daemon-reload && systemctl --user enable --now entire-nightly.timer`.
4. Make sure that the timer runs when the user is not logged in: `loginctl enable-linger "$USER"`.
5. Make sure that the timer is set: `systemctl --user list-timers entire-nightly.timer`.

The Entire watcher (`entire-brain-watch.service`) and the session-end hook can also distill, on every branch. If the user wants only the nightly job to distill, find the setting with `entire agent-help brain` and `entire brain setup --help`. Then check `~/.local/state/entire/logs/watch.log` for `distill=false`. Do not use `entire brain setup --uninstall-daemon`. The watcher also keeps the brain current without tokens.

## Answer

Write the answer in ASD-STE100 Simplified Technical English. Keep code, commands, file names and error text as they are.

Write these items in this sequence:

1. The branch and its PR.
2. The counts: memories kept, retracted and promoted. Proposals applied, rejected, dropped and skipped, on the branch and on `main`. Memories retracted on `main`.
3. Whether you deleted the local branch.
4. The skill candidates from `patterns skills`, one on each line, or "none".
