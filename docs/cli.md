# CLI reference

`laura` with no subcommand runs the TUI host; with a subcommand it acts as a thin client that sends one message to the current tab's socket (`$LAURA_TAB`) and exits. Client verbs are silent on success.

## Host

```
laura                   Run the TUI, hosting your default shell in tab 1.
laura -- <cmd> [args]   Run the TUI, hosting <cmd> in tab 1. New tabs still get your shell.
```

## Client verbs

```
laura open <path>       Open (or replace) the panel in the current tab, rendering <path>.
      --no-focus        Don't move focus into the panel.
laura close             Close the current tab's panel.
laura ready             Mark the tab as hosting an agent (enables review submission).
```

Client verbs require `$LAURA_TAB` to be set — i.e. run them from inside a Laura-hosted shell. Outside a tab they error with `not inside a Laura tab (LAURA_TAB unset)`.

## Global

```
laura --help            Print help (also per-subcommand: laura open --help).
laura --version         Print version.
```

The wire messages behind these verbs are documented in the [protocol](protocol.md).
