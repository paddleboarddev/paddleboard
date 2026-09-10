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
gone stale **three** times: v0.1.8.1 → v0.2.6, v0.2.6 → v0.2.7 (reconstructed
2026-08-14 from PRs #161–#174), and v0.2.7 → v0.3.0 (reconstructed 2026-09-09
from PRs #187–#227). Reconstruction is slower and silently drops anything whose
PR body was thin — the v0.2.7→v0.3.0 pass found four upstream-merge PRs marked
`- N/A` that had in fact shipped user-visible features.

Twice was called a process problem. Three times said the process needed a check,
so `script/danger/dangerfile.ts` now warns on any PR that carries a user-facing
release note without touching this file. The bullet belongs in the PR that lands
the change, not recovered at cut time. See "When to cut a release" in
`RELEASING.md`.

## Next

Reconstructed 2026-09-09 from PRs #187–#227. Each bullet was taken from the PR's
own `Release Notes` section where it had one; the upstream-merge entries were
verified against the shipped assets rather than inferred from the merge titles.

- Added a keybinding and a View menu entry for the AI Dock (`cmd-k cmd-a` on
  macOS, `ctrl-k ctrl-a` on Linux and Windows).
- Added a configurable window title, via the `window_title_format` and
  `window_title_separator` settings.
- Added a pending-keystroke indicator to the status bar, with `which_key`
  settings to control it.
- Added a call-hierarchy picker.
- Fixed the AI Dock sometimes doing nothing when opened from the command
  palette.
- Fixed macOS releases not being verifiable offline, which could block first
  launch for someone who downloaded while online and then went offline.
- Improved the LLM provider list ordering, with Zed's optional hosted service
  listed last rather than first.
- Improved project search-as-you-type, which now uses upstream's implementation;
  its debounce is 250ms where PaddleBoard's was 200ms.

Not listed, and deliberately: the four upstream merges themselves (#192, #198,
#210, #221) beyond the user-visible features above, the release-pipeline and
CI fixes, dependency bumps, and the README/publish guards. None of those change
what a user sees.
