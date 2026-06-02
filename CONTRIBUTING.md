# Contributing to PaddleBoard

PaddleBoard is a small, alpha-stage fork of [Zed](https://github.com/zed-industries/zed). Issues and PRs are welcome, but please scope-check anything bigger than a small fix before you spend a lot of time on it.

## Before you start

A few conventions encode most of how the project works:

- **Code style.** Prioritize correctness and clarity. Avoid panics (`unwrap()`, out-of-bounds indexing) — propagate errors with `?`, and never silently discard them with `let _ =`. Use full words for names, and don't add comments that just restate the code.
- **Fork hygiene.** Net-new features go in their own `paddleboard_*` crate so upstream can't conflict on a file it doesn't have. When you must edit a shared/upstream-shaped file, keep the diff minimal and tag the change with a `// PaddleBoard: <reason>` comment so future weekly merges stay mechanical. Watch for silent drift in `assets/` (settings, keymaps, icons).
- [`WELCOME.md`](./WELCOME.md) — what features PaddleBoard adds on top of Zed, and which subsystems are fork-specific vs. inherited from upstream.

## What's likely to land

- Bug fixes in PaddleBoard-specific code (any `paddleboard_*` crate, or any shared file with a `// PaddleBoard:` tag).
- Small enhancements to the sandbox, browser panel, orchestration panel, sandboxed MCP transport, step-through mode, or LLM picker.
- Docs improvements — especially anything that makes the fork hygiene rules clearer.
- Tightening the upstream merge process (`.github/workflows/merge_upstream_zed.yml`).

## What probably won't land

- Anything that would belong upstream. File the PR against [`zed-industries/zed`](https://github.com/zed-industries/zed/pulls) instead — we'd just inherit it on the next weekly merge.
- New themes, language extensions, or icon themes — those go through Zed's extension system.
- Telemetry, sponsorship, or upsell code, even if upstream re-introduces it — those are deliberately inert.
- AI-generated code where the author can't explain what it does.

## Sending a PR

- Branch off `main`, push to a topic branch, open a PR. Title is a clear imperative sentence — no `feat:` / `fix:` prefixes, no trailing punctuation.
- Include a `Release Notes:` section at the bottom of the PR body. Either one bullet (`- Added …` / `- Fixed …` / `- Improved …`) for user-facing changes, or `- N/A` for everything else.
- For UI changes, attach a screenshot or short screen recording.

## Working on the upstream surface area

The bulk of the codebase — editor, LSP, debugger, GPUI, terminal, vim mode, language tooling, extensions — is inherited from Zed and merged in weekly. For anything in that surface area, [Zed's documentation](https://zed.dev/docs) and the [upstream `CONTRIBUTING.md`](https://github.com/zed-industries/zed/blob/main/CONTRIBUTING.md) are the canonical references. PaddleBoard inherits Zed's UI/UX standards by default; the upstream UI/UX checklist applies here too.

When you edit a shared file, keep the diff minimal and tag the change with a `// PaddleBoard:` comment so future merge resolution stays mechanical.
