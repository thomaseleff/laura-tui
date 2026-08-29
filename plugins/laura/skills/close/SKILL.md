---
name: close
description: Close a file panel (or all of them) in the current Laura tab. Use when running inside a Laura tab (LAURA_TAB is set) and you're done showing a file.
when_to_use: You are running inside a Laura tab (LAURA_TAB is set) and want to close an open panel.
user-invocable: true
---

# laura close

Close a panel in the current tab.

```bash
laura close          # close the focused panel
laura close <id>     # close a specific panel by id
laura close --all    # close every panel, back to shell-only
```

Docs: https://thomaseleff.github.io/laura-tui/llms.txt
