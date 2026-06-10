---
description: Run tests for a specific crate, file, or the whole project
allowed-tools: ["Bash", "Monitor", "Read"]
---

# /test — Run PaddleBoard tests

Run cargo tests for the PaddleBoard project. Defaults to the `paddleboard` crate.

## Argument handling

Parse `$ARGUMENTS` (text the user typed after `/test`):

| Argument                          | Command                                                    |
|-----------------------------------|------------------------------------------------------------|
| *(empty)*                         | `cargo test -p paddleboard`                                |
| a crate name (e.g. `workspace`)  | `cargo test -p <crate>`                                    |
| a test name (e.g. `test_foo`)    | `cargo test -p paddleboard -- <name>` (filter by name)     |
| `-p <crate> <filter>`            | `cargo test -p <crate> -- <filter>`                        |
| `all`                             | `cargo test --workspace` (everything — slow)               |
| starts with `--`                  | Pass through verbatim: `cargo test -p paddleboard $ARGUMENTS` |

If the argument looks like a test function name (contains `test_` or `::`) but no `-p`, search for which crate defines it and scope the run accordingly.

## Execution

- Run from the repo root via the Bash tool.
- Full workspace tests are slow. Use `run_in_background: true` and Monitor for `all` or multi-crate runs. Single-crate runs can be foreground.
- GPUI tests need the foreground thread. If a test requires `#[gpui::test]`, it will work with `cargo test` (gpui spawns its own event loop).

## After the run

- **All pass**: report the count and elapsed time.
- **Failures**: show the failing test name and the assertion/panic message verbatim. Offer to investigate but don't auto-fix.

User input: $ARGUMENTS
