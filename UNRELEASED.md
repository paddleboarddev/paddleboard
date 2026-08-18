# Unreleased

Staging area for the next release's notes. Add a bullet here as user-facing
work lands on `main`; at release time, build clean release notes from these
bullets and publish with:

```
RELEASE_NOTES_FILE=<clean-notes.md> bash script/publish-public.sh v0.X.Y "…"
```

Then reset this file back to an empty `## Next` section for the cycle after. Use
the `- Added` / `- Fixed` / `- Improved` convention from CLAUDE.md.

⚠️ This file only works if bullets actually get added as work lands. It has now
gone stale twice: once from v0.1.8.1 through v0.2.6, and again from v0.2.6
through v0.2.7, so both releases had their notes reconstructed from the commit
log and merged PR bodies. That is slower, and it silently drops anything whose
PR body was thin. See "When to cut a release" in `RELEASING.md`.

## Next

_Empty — v0.2.7 shipped the bullets below. New bullets accumulate here as
user-facing work lands._

## v0.2.7 (reconstructed from merged PRs #160–#174)

- Fixed filled buttons being invisible in the PaddleBoard themes, the MCP tab
  clipping its empty state, and installed personas appearing twice in the AI Dock
- Fixed the sidebar header and tab bar not lining up, and not scaling together at
  larger UI font sizes
- Fixed `--user-data-dir` not redirecting logs on macOS, where a second instance
  would write into and rotate the primary profile's log
- Fixed user-facing text that still said "Zed" in the Copilot sign-in window,
  remote-development errors, the debug-adapter error, the CLI install failure,
  `paddleboard --help`, and the run-as-root guard
- Fixed the bundled debug tasks and the new-worktree task template referencing
  task variables that never resolved
- Improved the first-run setup: local models are now clearly the no-API-key
  option, common providers are listed first, and the page scrolls from the keyboard
- Improved the AI Dock Skills tab so every listed skill works in any project,
  instead of some assuming the PaddleBoard repository
- Improved the AI Dock usage view to show cached tokens, so the breakdown adds up
  to the total, and to show a shortened path
- Improved the AI Dock MCP tab to label its installed-servers list
