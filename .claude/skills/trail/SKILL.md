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

You do the work from the branch to the merge. Stop for the user at each plan, at each review brief and at the approval.

## Terms

- A **trail** (`<P>`) is the intent: the issue and the decisions. `<P>` is the trail number, for example `6`. Do not add the project, as in `gh/<owner>/6`.
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
   1. `Now:` what the user sees now. `After:` what the user sees after the change.
   2. One `Decision:` line for each decision.
   3. `Done when:` each acceptance check that shows the result.
   4. `GitHub issue: <URL>`. Add `GitHub PR: <URL>` after you open the PR.

   Do not write a status in the body. The gates, the findings, the approvals and the PR state show the status. Change the body only when the story, a decision or a check changes.
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

A subagent reviews the draft PR before the Entire reviewer reviews it. The session that wrote the code does not review its own PR.

14. Start one subagent (Agent tool, `isolation: "worktree"`) that runs `/pr-review <PR>`. Do this for every PR, also a small one. Tell the subagent: first run `gh pr checkout <PR> --detach` in your worktree, because the worktree starts at `main` and the trail's worktree has the branch. Do not commit, do not post findings, do not change the trail, and return the pr-review answer.
15. Check each finding against the code. Post each finding on the change, also each Follow-up. Never post a finding as a comment. Map the severities like this: Blocker → `high`, Should fix → `medium`, Follow-up → `low`.

    ```
    entire trail finding add --trail <repo>/<n> --severity <high|medium|low> --confidence <0..1> --file <path> --line <n> -m "…"
    ```

16. When the subagent is done, check `git worktree list` and `git branch --list 'worktree-agent-*'`. If the subagent's worktree or branch is still there, remove them: `git worktree remove --force <path>` and `git branch -D <branch>`. The probe edits of the subagent stay uncommitted in that worktree, so `--force` is necessary.
17. Brief the user (see "Brief"). If the review has open findings, stop. If it has none, go to step 20 with no stop.
18. When the user approves the fixes, switch into plan mode. Write the fix plan from the user's answers, against the current code.
19. Fix the findings, commit, run the check suite and push. Then resolve each fixed finding. Tell which commit fixed it and how you checked the fix:

    ```
    entire trail finding resolve <id> --trail <repo>/<n> -m "Fixed in <sha>: <how you checked>"
    ```

    For each finding that the user dismisses, dismiss the finding with the user's reason:

    ```
    entire trail finding dismiss <id> --trail <repo>/<n> -m "<the user's reason>"
    ```

    For each Follow-up that the user logs as an issue, run `gh issue create`. Then dismiss the finding with the same command and `-m "Logged as #<n>, as the user agreed"`.
20. Update the trail body if a decision or a check changed. Mark the PR as ready for review: `gh pr ready <n>`. The Entire reviewer starts.

### Gates

21. Get all findings, including the findings from the Entire reviewer:

    ```
    entire trail finding <repo>/<n> --freshness any --status any
    ```

22. Every open finding blocks the merge, including a low finding. If the round has open findings, brief the user. Stop. If it has none, go to step 26.
23. When the user approves the fixes, switch into plan mode. Write a plan that fixes the open findings from the user's answers.
24. Fix the findings, and resolve each finding, as step 19 tells.
25. Dismiss a finding only when the user agrees.
26. Wait for CI: `gh pr checks <n> --watch`.
27. If a CI job fails and your change cannot cause the failure, run the failed jobs again one time: `gh run rerun <run-id> --failed`. Tell the user about the rerun in the next brief.
28. Run `entire trail merge <repo>/<n> --dry-run`. Entire reviews each new head again, so new findings can come. Do steps 21 to 28 again until the dry run shows only the approval gate.

### Final review

29. Start one subagent that runs `/pr-review <PR>` on the full PR, as step 14 tells. Remove its worktree when it is done, as step 16 tells. Do this one time only.
30. Check each finding against the code. Log each Follow-up as a GitHub issue with `gh issue create`. Write the issue in ASD-STE100, with a link to the PR. Do not post the Follow-up as a finding.
31. If the review has no Blocker or Should-fix finding, go to step 33, and list the issue URLs from step 30 in that brief. Otherwise, post each Blocker and Should-fix finding as step 15 tells, brief the user, list the issue URLs from step 30, and stop.
32. When the user approves the fixes, do steps 18 and 19. Then do steps 21 to 28 again. Do not run a second final review. Then go to step 33.

### Approve and merge

33. Stop. Brief the user, and ask the user to approve the trail.
34. When the user says "Approve", approve the trail: `entire trail approve <repo>/<n> -m "<what the user approved>"`. Do not approve the trail before the user says "Approve".
35. Run `entire trail merge <repo>/<n> --dry-run` again. If a gate fails, tell the user which gate and why.
36. When the user tells you to merge, run `entire trail merge <repo>/<n>`. Entire squash-merges the PR.
37. Make sure that GitHub closed the issue: `gh issue view <N> --json state`.
38. If the `status` field in `entire trail show <P> --json` is still `open`, close the trail: `entire trail update <P> --status closed`.
39. Go back to the main checkout. Remove the worktree, and update `main`:

    ```
    cd <main checkout>
    git worktree remove tmp/worktrees/<slug>-<N>
    git pull
    ```

40. Do not promote the memories of the branch here. The branch waits for `/memory-closeout`.

## Brief

Write every stop in ASD-STE100 Simplified Technical English. Keep code, commands, file names and error text as they are. Give the user all the information to make each decision without reading the code.

Write these items in this sequence:

1. **TL;DR**: 2 to 4 sentences. Tell what the round found, and what the user must decide.
2. **Links**: the trail URL, the change URL and the PR URL.
3. **Findings**: the number of findings for each severity, open and resolved. Then a table with these columns: the finding id, the severity, the finding in one line, the users that the finding affects, and the recommendation (fix now, log as an issue, or dismiss). In the Review and Gates rounds, recommend "fix now" for a Follow-up when the fix is small and in the scope of the PR. In the final review, list the Follow-ups as the issue URLs from step 30, not in the table.
4. **Before / after**: for each finding, what the user sees now and after the fix. Show the command output, the notice text, the doc sentence or a short code snippet. For a UI change, show an ASCII mockup of the pane, the pane header or the notice, before and after.
5. **Questions**: each decision that the user must make, with the options and a recommendation.
6. **Gates**: each gate and its state, and each CI rerun.
7. **Not checked**: the manual checks that only the user can do.
8. **Next step**: for example "Approve the fixes" or "Approve".

Example of a before / after item:

```
F2 · medium · The paused tail pane loses its place on a reload
Before:  laura tail log.txt → p → (file grows) → the pane jumps to the end
After:   the pane stays on the line the user reads; "Paused" stays in the pane header
┌ log.txt ── Paused ─┐      ┌ log.txt ── Paused ─┐
│ 1041 GET /a 200    │  →   │ 1041 GET /a 200    │
│ 1042 GET /b 500    │      │ 1042 GET /b 500    │
```
