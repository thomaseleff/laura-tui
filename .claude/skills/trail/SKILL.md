---
name: trail
description: Run a GitHub issue as an Entire trail, from the branch to the squash merge.
when_to_use: The user asks you to implement an issue as a trail, or gives a trail URL and asks you to continue it.
user-invocable: true
argument-hint: <issue | trail URL>
---

# Trail

## Getting started

Use this section only when the user asks how to use this skill.

To start a trail, type `/trail <issue | trail URL>`. Examples:

- `/trail 130` runs issue #130 as a new trail.
- `/trail https://entire.com/gh/thomaseleff/-/trails/4/changes/laura-tui/4` continues trail #4 at the step where it stopped.

You do the work from the branch to the merge. Stop for the user at two points: the plan and the approval.

## Terms

- A **trail** (`<P>`) is the intent: the issue, the decisions and the status.
- A **change** (`<repo>/<n>`, for example `laura-tui/4`) is the branch of the trail in one repository. Trails and changes have different numbers. Do not use one number for the other.
- A **finding** is review feedback from an agent. A **comment** is feedback between people. Post your feedback as findings.
- Read the exact flags with `entire agent-help trail <subcommand>`. Do not guess a flag.

## Workflow

### Start

1. Read the issue with `gh issue view <N>`.
2. Switch into plan mode. Write the plan with the "Plans" conventions in `CLAUDE.md`.
3. Record each decision that changes the *Done when* of the issue. The trail body must show each such decision.
4. Create a worktree for the branch. Then other agents can run other trails in the main checkout at the same time.

   ```
   git worktree add --no-track -b <fix|feat|chore>/<slug>-<N> tmp/worktrees/<slug>-<N> origin/main
   cd tmp/worktrees/<slug>-<N>
   ```

   Do all of the work of the trail in this worktree. `--no-track` sets no upstream, so a bare `git push` cannot push to `main`.

### Trail

5. Create the trail before the first commit. If the branch has commits, `entire trail create` opens a draft PR for the trail. You open the PR yourself in step 12.

   ```
   entire trail create --title "<Thing> (#N)" --type <bug|feature|task> --base main --body "…"
   ```

6. Write the body as plain lines. Entire removes Markdown from the body. Use this sequence:
   1. The intent: what the user sees now, and what the user sees after the change.
   2. One `Decision:` line for each decision.
   3. `GitHub issue: <URL>`. Add `GitHub PR: <URL>` after you open the PR.
   4. `Done when:` and the checks that show the result.
   5. `Status:` and the current state of the work.
7. Get the change URL from `entire trail show <P> --json`, in `changes[].url`. Use this URL when you link the trail.

### Implement

8. Implement the plan. Follow the "Coding", "Testing", "Implementation requirements" and "Writing" conventions in `CLAUDE.md`.
9. Make the commits. Put `(#N)` in the subject of each commit.
10. Run the check suite before each push: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`.

### PR

11. Push the branch: `git push -u origin <branch>`.
12. Open the PR as a draft: `gh pr create --draft --base main --title "…" --body-file <file>`.
    - Write the title with the "Commits & PRs" conventions in `CLAUDE.md`. Do not put `#N` in the title.
    - Put `Closes #N` and the change URL in the body.
    - If Entire opened a draft PR in step 5, edit that PR with `gh pr edit <n>` instead. Keep the `<!-- entire-shadow-pr -->` line at the end of the body. Entire uses it to close the PR when the trail closes.
13. Add `GitHub PR: <URL>` to the trail body with `entire trail update <P> --body "…"`.

### Review

Review the draft PR yourself before the Entire reviewer reviews it.

14. Run `/pr-review <PR>`.
15. Post each finding on the change. Never post a finding as a comment.

    ```
    entire trail finding add --trail <repo>/<n> --severity <high|medium|low> --confidence <0..1> --file <path> --line <n> -m "…"
    ```

16. Fix the findings, commit, and resolve each finding as step 21 tells.
17. Mark the PR as ready for review: `gh pr ready <n>`.

### Gates

18. Get all findings, including the findings from the Entire reviewer:

    ```
    entire trail finding <repo>/<n> --freshness any --include-dismissed
    ```

19. Every open finding blocks the merge, including a low finding. Switch into plan mode, and write a plan that fixes all open findings.
20. Fix the findings, and commit.
21. Resolve each finding. Tell which commit fixed it and how you checked the fix:

    ```
    entire trail finding resolve <id> --trail <repo>/<n> -m "Fixed in <sha>: <how you checked>"
    ```

22. Dismiss a finding only when the user agrees.
23. Push. Then wait for CI: `gh pr checks <n> --watch`.
24. If a CI job fails and your change cannot cause the failure, run the failed jobs again one time: `gh run rerun <run-id> --failed`. Record the rerun in the `Status:` line.
25. Run `entire trail merge <repo>/<n> --dry-run`. Entire reviews each new head again, so new findings can come. Do steps 18 to 25 again until the dry run shows only the approval gate.
26. Keep the `Status:` line current with `entire trail update <P> --body "…"`.

### Approve and merge

27. Stop. Ask the user to approve the trail.
28. When the user says "Approve", approve the trail: `entire trail approve <repo>/<n> -m "<what the user approved>"`. Do not approve the trail before the user says "Approve".
29. Run `entire trail merge <repo>/<n> --dry-run` again. If a gate fails, tell the user which gate and why.
30. When the user tells you to merge, run `entire trail merge <repo>/<n>`. Entire squash-merges the PR.
31. Make sure that GitHub closed the issue: `gh issue view <N> --json state`.
32. If `entire trail show <P>` still shows `Status: open`, close the trail: `entire trail update <P> --status closed`.
33. Go back to the main checkout. Remove the worktree, and update `main`:

    ```
    cd <main checkout>
    git worktree remove tmp/worktrees/<slug>-<N>
    git pull
    ```

34. Do not promote the memories of the branch here. The branch waits for `/memory-closeout`.

## Answer

Write the answer in ASD-STE100 Simplified Technical English. Keep code, commands, file names and error text as they are.

At each stop, write these items in this sequence:

1. The trail URL and the change URL.
2. The PR URL.
3. The number of findings for each severity, open and resolved.
4. Each gate and its state.
5. The next step for the user, for example the approval.
