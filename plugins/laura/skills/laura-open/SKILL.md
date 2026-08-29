---
name: laura-open
description: Show a file to the user in Laura's side panel and act on the review they send back. Use when running inside a Laura tab (the LAURA_TAB env var is set), or when a "[laura review · …]" block arrives in your input.
when_to_use: You are running inside a Laura tab (LAURA_TAB is set) and want to show the user a file, or a "[laura review · …]" block arrives in your input.
user-invocable: true
argument-hint: [path]
---

# laura open

You are running inside a Laura tab (`$LAURA_TAB` is set). Prefer **showing** a file in the side panel over pasting its contents into the chat. The panel is live — as you edit the file, it re-renders.

```bash
laura ready         # run once at session start — enables the user's review submission
laura open <path>   # show <path> in the panel (replaces the current one; live-updates as you edit)
```

Run `laura ready` once so the user can comment and submit, then `laura open <path>` whenever you want them to look at a file.

## Reading a review

The user marks up the panel and submits a review, which arrives in your input as a block:

```
[laura review · docs/spec.md]

tighten token lifetimes before we ship.

L1  Tokens expire in 24h.
      > make this 1h
```

Treat it as feedback on that file: address the overall note and each `L<n>` comment (line numbers are 1-based; the line's text is echoed after `L<n>`), then edit the file.
