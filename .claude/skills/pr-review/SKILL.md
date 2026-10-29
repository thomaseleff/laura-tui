---
name: pr-review
description: Review a commit or PR, and put the findings in order of severity.
when_to_use: The user asks for a code review of a commit, a branch or a PR.
user-invocable: true
argument-hint: <commit | PR> [plan]
---

# PR review

## Getting started

Use this section only when the user asks how to use this skill.

To start a review, type `/pr-review <commit | PR> [plan]`. Examples:

- `/pr-review a8cc2f6` reviews one commit.
- `/pr-review #98` reviews all the commits in PR #98.
- `/pr-review a8cc2f6 plans/rendered-markdown-89.md` also compares the commit with the *Done when* in the plan.

You write a plan for the fixes. Do not change the code until the user tells you to do so.

## Workflow

1. Make a scratch pad at `tmp/pr-review-<commit | PR>.md`. While you do the review, write each finding, its severity, its `file:line` reference, its script and its status in the scratch pad. Write each note when you find it, not at the end.
2. Get the diff. Use `git show <commit>` or `gh pr diff <n>`. If the user gives a plan, read the plan.
3. Review only the changes in the diff. If you find a bug from an earlier commit, record the bug as a Follow-up.
4. For each changed path, read the code that calls the path. Read the code that uses the output of the path.
5. Find the bugs that a user can see in usual operation. Usual operation includes correct files, usual links and usual keystrokes.
6. Give each finding one of these severities:
   - **Blocker**: A documented feature does not operate in usual operation, or the user loses data.
   - **Should fix**: The feature operates in usual operation, but the result is not clear or not complete. The fix is small.
   - **Follow-up**: An edge case, such as incorrect input, an unusual URL or an unusual file name. A Follow-up can also be an improvement or a bug from an earlier commit.
7. For each Blocker and Should-fix finding, write a script in `tmp/`. The script must show the bug through the CLI, the socket or a browser. Build with `CARGO_TARGET_DIR=tmp/target cargo build`.
8. If the script does not show the bug, remove the finding from the scratch pad.
9. Write a fix plan for each Blocker and Should-fix finding. Each fix must include the code, the tests, the docs, the comments and the skills, as `CLAUDE.md` tells you.
10. Apply each fix temporarily. Run the same script again to make sure that the fix operates correctly.
11. Remove the temporary changes.
12. Compare the change with the conventions in `CLAUDE.md`. Include the Writing rules and the Glossary. Record a convention problem only if it can cause a reader to make an error. Examples are an old comment, a doc that does not agree with the code, or plan text in the code.
13. Run the check suite: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`.

## Answer

Write the answer from the scratch pad. Use ASD-STE100 Simplified Technical English. Keep code, commands, file names and error text as they are.

Write these items in this sequence:

1. The number of findings for each severity, on one line.
2. A table of the findings. Use these columns: the severity, the finding in one line, the commit that caused it, and the users that the finding affects.
3. A fix plan for each Blocker and Should-fix finding. Include the `file:line` references, the code change, and the changes to the tests, the docs and the comments.
4. The Follow-ups. Write each Follow-up on one line, so that the user can copy it into a GitHub issue.
5. The result of the check suite. If a check fails, give the output of the failure.

Keep the answer short. Do not record a convention problem that cannot cause a reader to make an error. Do not recommend a next step.
