---
description: Run ./script/clippy (the project's custom lint script) instead of raw cargo clippy
allowed-tools: ["Bash", "Monitor"]
---

# /clippy — Lint PaddleBoard

Run `./script/clippy` from the repo root. This project uses a custom clippy wrapper — never run `cargo clippy` directly.

## Argument handling

Parse `$ARGUMENTS` (text the user typed after `/clippy`):

| Argument                        | Command                                      |
|---------------------------------|----------------------------------------------|
| *(empty)*                       | `./script/clippy`                             |
| `-p <crate>` or `--fix`        | `./script/clippy $ARGUMENTS` (pass through)   |
| anything else                   | `./script/clippy $ARGUMENTS` (pass through)   |

## Execution

- Run via the Bash tool from the repo root.
- A full clippy pass touches many crates and can take a few minutes on first run. Use `run_in_background: true` and Monitor for full-project runs. Scoped runs (`-p single_crate`) can run in the foreground.

## After the run

- **Clean**: report "No warnings" and the elapsed time.
- **Warnings/errors**: surface the first few diagnostics verbatim. Offer to fix them but don't start unless asked.

User input: $ARGUMENTS
