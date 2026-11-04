# Contributing to PaddleBoard

PaddleBoard is a small, beta-stage fork of [Zed](https://github.com/zed-industries/zed). Issues and PRs are welcome, but please scope-check anything bigger than a small fix before you spend a lot of time on it.

## Before you start

A few conventions encode most of how the project works:

- **Code style.** Prioritize correctness and clarity. Avoid panics (`unwrap()`, out-of-bounds indexing) — propagate errors with `?`, and never silently discard them with `let _ =`. Use full words for names, and don't add comments that just restate the code.
- **Fork hygiene.** Net-new features go in their own `paddleboard_*` crate so upstream can't conflict on a file it doesn't have. When you must edit a shared/upstream-shaped file, keep the diff minimal and tag the change with a `// PaddleBoard: <reason>` comment so future weekly merges stay mechanical. Watch for silent drift in `assets/` (settings, keymaps, icons). The full playbook is [`FORK_HYGIENE.md`](./FORK_HYGIENE.md).
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

- Fork [`paddleboarddev/paddleboard`](https://github.com/paddleboarddev/paddleboard), branch off `main` in your fork, and open a PR against `paddleboarddev/paddleboard`'s `main`. Maintainers work the same way, from their own forks. Title is a clear imperative sentence — no `feat:` / `fix:` prefixes, no trailing punctuation.
- Include a `Release Notes:` section at the bottom of the PR body. Either one bullet (`- Added …` / `- Fixed …` / `- Improved …`) for user-facing changes, or `- N/A` for everything else.
- For UI changes, attach a screenshot or short screen recording.
- Sign off every commit (see below).
- Workflows on PRs from first-time and outside contributors wait for a maintainer to approve them before they run, so the checks may sit at "waiting" for a while.

## Agent-written changes

Using a coding agent is fine, as long as you've read and understood what it wrote. The project's agent instructions (`.rules`, which `CLAUDE.md` and `AGENTS.md` point to) make an agent put this banner at the top of `README.md` whenever it changes source files:

```
> [!IMPORTANT]
> Remove this line to confirm you've reviewed this PR before submitting.
```

Removing it is your confirmation that you've reviewed the changes, so agents are told never to remove it themselves. The Fork guards check fails while it's there.

To remove it, delete those two lines and commit, or run `script/clear-review-banner`, which does it safely even if you run it twice. GitHub's web editor on your branch works too. Maintainers can clear it on a PR with `script/clear-review-banner <PR number>`, which commits with a sign-off and pushes to the PR's branch.

## Sign off your commits

PaddleBoard uses the [Developer Certificate of Origin](https://developercertificate.org/) (DCO) instead of a contributor license agreement. By adding a `Signed-off-by` line to a commit, you certify that you wrote the change, or otherwise have the right to submit it, under the project's license (GPL-3.0-or-later). You keep the copyright to your work; nothing is assigned.

Add the line with `git commit -s`, which uses your `user.name` and `user.email`:

```
Signed-off-by: Your Name <you@example.com>
```

Use your real name or a name you're known by. To sign off commits you've already made, run `git rebase --signoff main` and force-push the branch.

## Working on the upstream surface area

The bulk of the codebase — editor, LSP, debugger, GPUI, terminal, vim mode, language tooling, extensions — is inherited from Zed and merged in weekly. For anything in that surface area, [Zed's documentation](https://zed.dev/docs) and the [upstream `CONTRIBUTING.md`](https://github.com/zed-industries/zed/blob/main/CONTRIBUTING.md) are the canonical references. PaddleBoard inherits Zed's UI/UX standards by default; the upstream UI/UX checklist applies here too.

When you edit a shared file, keep the diff minimal and tag the change with a `// PaddleBoard:` comment so future merge resolution stays mechanical.

## Keeping your own files

Notes, to-do lists and agent instructions you keep next to the code belong in `.private/` at the root of your clone. The repo ignores that folder, so nothing in it can be committed by accident, and the Fork guards check fails a PR that forces one in with `git add -f`. `.claude/` is ignored the same way: the project ships no Claude config, so yours stays yours.

- **To keep a history of them,** run `git init` inside `.private/` and add a private remote if you want a backup. It's a separate repository, so pushing it never touches PaddleBoard.
- **For a file that has to live somewhere else in the tree,** add its path to `.git/info/exclude`, git's per-clone ignore list. It's never committed or shared.
- **For ignores you want in every repository** (editor files, OS clutter), use your global gitignore (`git config --global core.excludesFile ~/.gitignore_global`) rather than this project's `.gitignore`.
