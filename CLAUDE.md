# Laura

See [README.md](README.md) for what Laura is and how to run it, and [docs/](docs/) for the protocol, CLI, and design rationale.

## Coding

Standard Rust. No house style on top of it.

- **Format:** `cargo fmt` — default rustfmt, no custom `rustfmt.toml` unless a real need appears.
- **Lint:** `cargo clippy --all-targets -- -D warnings` must pass. Fix lints, don't `#[allow]` them without a one-line reason on the attribute.
- **Edition:** latest stable (2024). Toolchain pinned in `rust-toolchain.toml`.
- **Errors:** `anyhow` at binary boundaries, `thiserror` for typed library errors. No `.unwrap()`/`.expect()` in code that handles runtime input (PTY bytes, socket messages, files) — reserve them for invariants that can't fail, with the reason in the `.expect()` string.
- **Naming/layout:** stock Rust conventions (snake_case, `mod`s per concern). Don't invent abstractions ahead of the second caller.

## Testing

**Integration tests only — exercise Laura as a user (or the agent) actually would.** No per-function unit test suites.

- Tests live in `tests/`, drive the real public surface: the `laura` CLI binary and the per-tab socket protocol. Use `assert_cmd` + `tempfile` to run the binary and inspect its effects.
- Each test maps to a workstream's **Done when**, not to a function. "Feed an `open` message → panel state holds the file's content" — through the protocol, not by calling an internal fn.
- The TUI render loop can't be driven interactively in CI: assert on **state** (app state after a message, panel content, socket round-trip) rather than pixels.
- **The one exception** — pure logic a user can't reach through the CLI (DSR handshake reply from a byte stream, review-payload assembly, NDJSON framing). Test it through the smallest public entry point that reaches it; only if there's genuinely no such surface, a `#[cfg(test)]` check next to the code. Prefer exposing the seam over reaching into privates.

Run before calling anything done: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`.
