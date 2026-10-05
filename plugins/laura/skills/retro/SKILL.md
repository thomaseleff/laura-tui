---
name: retro
description: Look back at the feedback and inline reviews Laura recorded across sessions — summarize sentiment, list negative feedback, group by agent or date.
when_to_use: Your partner wants a summary of past feedback/inline reviews — "retro my sessions", "what feedback have I logged", "how did the lessons go".
user-invocable: true
argument-hint: [filter]
---

# laura retro

Read and summarize the feedback and inline reviews in the journals. Laura writes one NDJSON journal per session. Query the journals with `laura journal`, which works inside or outside a Laura workspace.

**Rules**
- **Run `laura ready --session "$YOUR_SESSION_ID" --agent <name>`** at the start. If `laura ready` fails, you're outside a Laura workspace. Carry on, because `laura journal` works outside a workspace too.
- **Read only.** Never edit the journals.
- **Summarize in the chat**: a sentiment count, the recurring negative feedback, and anything worth opening as an issue (https://github.com/thomaseleff/laura-tui/issues).
- **See the `laura` skill** for the full CLI or run `laura --help`.
- **Read the docs** at https://thomaseleff.github.io/laura-tui/llms.txt.

## Recipes

`laura journal` prints one event per line, oldest first, across every session: `{type,ts,session,agent,...}`. `ts` is a Unix time in milliseconds.

- `feedback` events carry `{sentiment,body,pane}`, with `sentiment` either `+` or `-`.
- `review` events carry `{path,comments,body}`, plus `error` when Laura couldn't write the inline review into the chat.

`FIELD=VALUE` matches a top-level field. Repeat a field to match any of its values; different fields must all match. `-n <N>` keeps the newest N matches (default 50). When `laura journal` leaves out matches, it warns `showing N of M events; pass -n for more` on stderr. Raise `-n` to see more.

```bash
# Feedback and inline reviews together — the overview
laura journal type=feedback type=review

# Sentiment tally: -n 0 prints nothing but the `showing 0 of M` line; M is the count (no line means 0)
laura journal type=feedback sentiment=+ -n 0
laura journal type=feedback sentiment=- -n 0

# Every negative feedback
laura journal type=feedback sentiment=-

# The last week only
laura journal type=feedback --since 7d

# One agent's feedback (group by running it per agent)
laura journal type=feedback agent=<name>

# Inline reviews submitted (read `path` and `body`; `error` is set on ones Laura couldn't write)
laura journal type=review

# One session only
laura journal session=<session-id>
```
