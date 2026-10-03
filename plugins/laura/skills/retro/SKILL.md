---
name: retro
description: Look back at the feedback and inline reviews Laura recorded across sessions — summarize sentiment, list negative feedback, group by agent or date.
when_to_use: You are running inside a Laura tab (LAURA_TAB is set) and your partner wants a summary of past feedback/inline reviews — "retro my sessions", "what feedback have I logged", "how did the lessons go".
user-invocable: true
argument-hint: [filter]
---

# laura retro

Read and summarize the feedback and inline reviews recorded in the journals — this skill only reads them. The journals are per-session NDJSON files, queried with `jq`.

**Rules**
- **Check that `$LAURA_TAB` is set**; otherwise, let your partner know you are not running in a Laura workspace.
- **Read only** — never write to the journals here.
- **Summarize back in chat**: a sentiment count, the recurring negative feedback, and anything worth opening as an issue (https://github.com/thomaseleff/laura-tui/issues).
- **See the `laura` skill** for the full CLI or run `laura --help`.
- **Read the docs** at https://thomaseleff.github.io/laura-tui/llms.txt

## Find the sessions dir

The journals live in `~/.laura/sessions/*.ndjson` (`DIR=~/.laura/sessions`).

## Recipes

Each line is one event: `{type,ts,session,agent,...}`. `feedback` events carry `{sentiment,body,pane}`; `review` events carry `{path,comments,body}`, plus `error` when Laura couldn't write the inline review into the shell. `ts` is unix ms.

```bash
cd "$DIR"

# Sentiment tally across every session
cat *.ndjson | jq -r 'select(.type=="feedback") | .sentiment' | sort | uniq -c

# Every negative feedback body, newest file first
ls -t *.ndjson | xargs cat | jq -r 'select(.type=="feedback" and .sentiment=="negative") | .body'

# Group feedback by agent
cat *.ndjson | jq -r 'select(.type=="feedback") | "\(.agent)\t\(.sentiment)\t\(.body)"' | sort

# Inline reviews submitted (file + review body)
cat *.ndjson | jq -r 'select(.type=="review") | "\(.path): \(.body)"'

# Inline reviews Laura couldn't write into the shell
cat *.ndjson | jq -r 'select(.type=="review" and .error) | "\(.path): \(.error)"'

# One session only
jq -r 'select(.type=="feedback") | "\(.sentiment): \(.body)"' <session-id>.ndjson

# Filter by time (events after a unix-ms cutoff)
cat *.ndjson | jq -r 'select(.type=="feedback" and .ts > 1700000000000) | .body'
```
