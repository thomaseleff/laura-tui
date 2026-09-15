---
name: retro
description: Look back at the feedback and reviews Laura recorded across sessions — summarize sentiment, list negative notes, group by agent or date.
when_to_use: You are running inside a Laura tab (LAURA_TAB is set) and the user wants a summary of past feedback/reviews — "retro my sessions", "what feedback have I logged", "how did the lessons go".
user-invocable: true
argument-hint: [filter]
---

# laura retro

Read and summarize the feedback and reviews recorded in the journals — this skill only reads them. The journals are per-session NDJSON files, queried with `jq`.

**Rules**
- Check that `$LAURA_TAB` is set, otherwise, let the user know you are not running in a Laura workspace.
- Read only — never write to the journals here.
- Summarize back in chat: a sentiment count, the recurring negative notes, and anything worth opening as an issue (https://github.com/thomaseleff/laura-tui/issues).
- See the `laura` skill for the full CLI or run `laura --help`.
- Read the docs: https://thomaseleff.github.io/laura-tui/llms.txt

## Find the sessions dir

The journals live under `<data_dir>/laura/sessions/*.ndjson`. Resolve `<data_dir>` the way the binary does — honor `LAURA_DATA_DIR` first, else the OS data dir:

```bash
# Easiest anchor: laura ready prints the journal path; its grandparent is the sessions dir.
DIR=$(dirname "$(laura ready --session "$YOUR_SESSION_ID" --agent claude)")

# Or resolve directly:
if [ -n "$LAURA_DATA_DIR" ]; then BASE="$LAURA_DATA_DIR"
elif [ -n "$APPDATA" ]; then BASE="$APPDATA"                                   # windows
elif [ -n "$XDG_DATA_HOME" ]; then BASE="$XDG_DATA_HOME"                       # linux
elif [ -d "$HOME/Library/Application Support" ]; then BASE="$HOME/Library/Application Support"  # mac
else BASE="$HOME/.local/share"; fi                                            # linux fallback
DIR="$BASE/laura/sessions"
```

## Recipes

Each line is one event: `{type,ts,session,agent,...}`. `feedback` events carry `{sentiment,body,pane}`; `review` events carry `{path,comments,body}`. `ts` is unix ms.

```bash
cd "$DIR"

# Sentiment tally across every session
cat *.ndjson | jq -r 'select(.type=="feedback") | .sentiment' | sort | uniq -c

# Every negative note, newest file first
ls -t *.ndjson | xargs cat | jq -r 'select(.type=="feedback" and .sentiment=="negative") | .body'

# Group feedback by agent
cat *.ndjson | jq -r 'select(.type=="feedback") | "\(.agent)\t\(.sentiment)\t\(.body)"' | sort

# Reviews left (file + overall note)
cat *.ndjson | jq -r 'select(.type=="review") | "\(.path): \(.body)"'

# One session only
jq -r 'select(.type=="feedback") | "\(.sentiment): \(.body)"' <session-id>.ndjson

# Filter by time (events after a unix-ms cutoff)
cat *.ndjson | jq -r 'select(.type=="feedback" and .ts > 1700000000000) | .body'
```
