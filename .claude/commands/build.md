---
description: Build the PaddleBoard application via cargo (defaults to debug build of the `paddleboard` crate)
allowed-tools: ["Bash", "Monitor"]
---

# /build — Build PaddleBoard

Run a cargo build of the PaddleBoard application. Default target is the `paddleboard` binary crate at `crates/paddleboard/`.

## Argument handling

Parse `$ARGUMENTS` (text the user typed after `/build`) and map to a command:

| Argument                   | Command                                                                 |
|----------------------------|-------------------------------------------------------------------------|
| *(empty)*                  | `cargo build -p paddleboard`                                            |
| `release`                  | `cargo build --release -p paddleboard`                                  |
| `run`                      | `cargo run -p paddleboard`                                              |
| `run release`              | `cargo run --release -p paddleboard`                                    |
| `check`                    | `cargo check -p paddleboard`                                            |
| starts with `-p ` or `--`  | Pass through verbatim after `cargo build` (e.g. `/build -p agent_ui`)   |
| anything else              | Treat as extra cargo flags appended after `cargo build -p paddleboard`  |

If `$ARGUMENTS` is ambiguous, do the most reasonable thing and tell the user what you ran.

## Execution

- Run the build via the Bash tool. For any full debug or release build of the app (which can take 5+ minutes from a cold cache), use `run_in_background: true` and stream output with Monitor.
- For small scoped builds (`-p single_crate` on a leaf crate, or `cargo check`), foreground is fine.
- The repo root is the working directory regardless of where the user invoked the command.

## After the build

- **Success**: report elapsed time and binary location (e.g. `target/debug/paddleboard`). Do **not** auto-run the binary unless the user said `run`.
- **Failure**: surface the first compiler error verbatim — do not paraphrase. Offer to investigate, but don't start fixing things unless asked.

## Notes

- For lints, use `./script/clippy` (per `CLAUDE.md`), **not** `cargo clippy`. `/build` does not run clippy.
- Prefer `cargo check` or scoped `-p <crate>` while iterating; reach for a full debug build only when you need a runnable binary.

User input: $ARGUMENTS
