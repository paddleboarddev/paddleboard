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
gone stale twice: from v0.1.8.1 through v0.2.6, and again from v0.2.6 through
v0.2.7 — whose notes were reconstructed from the log on 2026-08-14 covering PRs
#161–#174. Reconstruction is slower and silently drops anything whose PR body was
thin. Twice in a row is a process problem, not an oversight: the bullet has to be
written in the PR that lands the change, not recovered at cut time. See "When to
cut a release" in `RELEASING.md`.

## Next

_Empty — v0.2.7 shipped the onboarding first-run fixes, the sidebar/tab-bar
seam, cached tokens in the Usage view, the bundled-skills curation, and one
tagline across every surface. New bullets accumulate here as user-facing work
lands._
