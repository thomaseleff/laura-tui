---
name: trail
description: Run a GitHub issue as an Entire trail, from the branch to the squash merge.
when_to_use: The user asks you to implement an issue as a trail, or gives a trail URL and asks you to continue the trail.
user-invocable: true
argument-hint: <issue | trail URL>
---

# Trail

## Getting started

Use this section only when the user asks how to use this skill.

To start a trail, type `/trail <issue | trail URL>`. Examples:

- `/trail 130` runs issue #130 as a new trail.
- `/trail https://entire.com/gh/thomaseleff/-/trails/4/changes/laura-tui/4` continues trail #4 from its last completed step.

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
   5. One `Dismissed:` line for each dismissed finding: the finding in one line, its id and the user's reason. Reviewers in later rounds read these lines and do not report these findings again.

   Do not write a status in the body. The gates, the findings, the approvals and the PR state show the status. Change the body only for a change to the story, a decision, a check or a dismissed finding.
7. Get the change URL from `entire trail show <P> --json`, in `changes[].url`. Use this URL in each link to the trail.

### Implement

8. Implement the plan. Follow the "Coding", "Testing", "Implementation requirements" and "Writing" conventions in `CLAUDE.md`.
9. Make the commits. Put `(#N)` in the subject of each commit.
10. Run the check suite before each push: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`.

### PR

11. Push the branch: `git push -u origin <branch>`.
12. Open the PR as a draft: `gh pr create --draft --base main --title "…" --body-file <file>`.
    - Write the title with the "Commits & PRs" conventions in `CLAUDE.md`. Do not put `#N` in the title.
    - Put `Closes #N` and the change URL in the body.
    - If Entire opened a draft PR in step 5, edit that PR with `gh pr edit <n>` instead. Keep the `<!-- entire-shadow-pr -->` line at the end of the body. Entire uses this line to close the PR at the same time as the trail.
    - Add each dismissed finding to a `Dismissed findings` list in the body (step 19).
13. Add `GitHub PR: <URL>` to the trail body with `entire trail update <P> --body "…"`.

### Review

A subagent does the first review of the draft PR, before the Entire reviewer. The session that wrote the code does not review its own PR.

14. Start one subagent (Agent tool, `isolation: "worktree"`) that runs `/pr-review <PR>`. Do this for every PR, also for a small PR. Give the subagent these instructions:
    - First, run `gh pr checkout <PR> --detach` in your worktree. Your worktree starts at `main`, and only the trail's worktree has the branch.
    - Read the `Dismissed findings` list in the PR body. Do not report these findings again.
    - Do not commit, do not post findings and do not change the trail.
    - Return the pr-review answer.
15. Check each finding against the code. Then compare each finding with the earlier findings from the command in step 21. If a finding repeats a resolved or dismissed finding, do not post the finding again. List the finding in the brief with the earlier finding id. Post each other finding on the change, also each Follow-up. Never post a finding as a comment. Use these severities: Blocker → `high`, Should fix → `medium`, Follow-up → `low`.

    ```
    entire trail finding add --trail <repo>/<n> --severity <high|medium|low> --confidence <0..1> --file <path> --line <n> -m "…"
    ```

16. The Agent result gives the worktree path and the branch of the subagent. After the subagent gives its answer, remove that worktree and that branch, if they exist: `git worktree remove --force <path>` and `git branch -D <branch>`. Do not remove the worktree or the branch of a different subagent. The probe edits of the subagent are not committed, so `--force` is necessary.
17. Brief the user (see "Brief"). If the review has open findings, stop. If the review has no open findings, go to step 20 and do not stop.
18. After the user approves the fixes, switch into plan mode. Use the user's answers and the current code to write the fix plan.
19. Fix the findings. Commit the fixes, run the check suite and push the branch. Then resolve each fixed finding. In the message, give the commit of the fix and how you checked the fix:

    ```
    entire trail finding resolve <id> --trail <repo>/<n> -m "Fixed in <sha>: <how you checked>"
    ```

    Do these steps for each dismissed finding:

    1. Dismiss the finding with the user's reason:

       ```
       entire trail finding dismiss <id> --trail <repo>/<n> -m "<the user's reason>"
       ```

    2. Add a `Dismissed:` line to the trail body (step 6) with `entire trail update <P> --body "…"`.
    3. Add the finding to the `Dismissed findings` list in the PR body with `gh pr edit <n> --body-file <file>`.

    Log a Follow-up as a GitHub issue only if the user does not want the fix in this PR. To log the issue, run `gh issue create`. Write the issue in ASD-STE100, with a link to the PR. Then dismiss the finding with the steps above, with `-m "Logged as #<n>, as the user agreed"`.
20. If a decision or a check changed, update the trail body. Mark the PR as ready for review: `gh pr ready <n>`. Then the Entire reviewer starts its review.

### Gates

21. Get all findings, also the findings from the Entire reviewer:

    ```
    entire trail finding <repo>/<n> --freshness any --status any
    ```

22. Each open finding blocks the merge, also a low finding. If the round has open findings, brief the user and stop. If the round has no open findings, go to step 26.
23. After the user approves the fixes, switch into plan mode. Use the user's answers to write a plan for the open findings.
24. Fix and resolve the findings with the procedure in step 19.
25. Do not dismiss a finding without approval from the user. Use the procedure in step 19.
26. Wait for CI: `gh pr checks <n> --watch`.
27. If a CI job fails and your change cannot cause the failure, run the failed jobs again one time: `gh run rerun <run-id> --failed`. Tell the user about the rerun in the next brief.
28. Run `entire trail merge <repo>/<n> --dry-run`. Entire reviews each new head again, so there can be new findings. Do steps 21 to 28 again until the dry run shows only the approval gate.

### Final review

29. Start one subagent that runs `/pr-review <PR>` on the full PR. Use the procedure in step 14. After the subagent gives its answer, remove its worktree and its branch with the procedure in step 16. Do the final review one time only.
30. Check and post each finding with the procedure in step 15, also each Follow-up. Do not log a Follow-up as an issue before the user makes a decision.
31. If the review has no new findings, go to step 33. If the review has new findings, brief the user and stop.
32. After the user approves the fixes, do steps 18 and 19. Then do steps 21 to 28 again, and go to step 33. Do not do a second final review.

### Approve and merge

33. Stop. Brief the user, and ask the user to approve the trail.
34. When the user says "Approve", approve the trail: `entire trail approve <repo>/<n> -m "<what the user approved>"`. Do not approve the trail before the user says "Approve".
35. Run `entire trail merge <repo>/<n> --dry-run` again. If a gate fails, tell the user the name of the gate and the cause of the failure.
36. When the user tells you to merge the PR, run `entire trail merge <repo>/<n>`. Entire squash-merges the PR.
37. Make sure that GitHub closed the issue: `gh issue view <N> --json state`.
38. If the `status` field in `entire trail show <P> --json` is still `open`, close the trail: `entire trail update <P> --status closed`.
39. Go back to the main checkout. Remove the worktree, and update `main`:

    ```
    cd <main checkout>
    git worktree remove tmp/worktrees/<slug>-<N>
    git pull
    ```

40. Do not promote the memories of the branch in this skill. `/memory-closeout` promotes the memories of the branch later.

## Brief

Write every stop in ASD-STE100 Simplified Technical English. Do not change code, commands, file names or error text. Give the user all the data for each decision. The user must not have to read the code.

Write these items in this sequence:

1. **TL;DR**: 2 to 4 sentences. Tell the results of the round and the decisions for the user.
2. **Links**: the trail URL, the change URL and the PR URL.
3. **Findings**: the number of findings for each severity, open and resolved. Then a table with these columns: the finding id, the severity, the finding in one line, the users that the finding affects, and the recommendation (fix now, log as an issue, or dismiss). Recommend "fix now" for a Follow-up when the fix is small and in the scope of the PR. For a finding that repeats an earlier finding, give the earlier finding id.
4. **Before / after**: for each finding, what the user sees now and after the fix. Show the command output, the notice text, the doc sentence or a short code snippet. For a UI change, show an ASCII mockup of the pane, the pane header or the notice, before and after.
5. **Questions**: each decision for the user, with the options and a recommendation.
6. **Gates**: each gate and its state, and each CI rerun.
7. **Not checked**: the manual checks for the user.
8. **Next step**: for example "Approve the fixes" or "Approve".

Example of a before / after item:

```
F2 · medium · A file pane with an unsubmitted inline review moves the thread on a reload
Before:  laura open notes.md → c on line 12 → (the agent edits notes.md) → the pane reloads, and the thread moves to line 14
After:   the pane freezes, the thread stays on line 12, and the pane shows the notice
         "⚠ notes.md changed on disk — Shift+S submit · Ctrl+R refresh (discards unsubmitted inline review)"
┌ notes.md ─────────────────────┐      ┌ notes.md ─────────────────────┐
│ 12 ● Retry the request        │  →   │ 12 ● Retry the request        │
│ 13   Log the error            │      │ 13   Log the error            │
│                               │      │ ⚠ Shift+S submit · Ctrl+R     │
│                               │      │   discards inline review      │
```
