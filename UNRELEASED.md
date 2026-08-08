# Unreleased

Staging area for the next release's notes. Add a bullet here as user-facing
work lands on `main`; at release time, build clean release notes from these
bullets and publish with:

```
RELEASE_NOTES_FILE=<clean-notes.md> bash script/publish-public.sh v0.X.Y "…"
```

Then reset this file back to an empty `## Next` section for the cycle after. Use
the `- Added` / `- Fixed` / `- Improved` convention from CLAUDE.md.

⚠️ This file only works if bullets actually get added as work lands. It sat
stale from v0.1.8.1 through v0.2.6, so v0.2.6's notes had to be reconstructed
from the commit log — which is slower and easier to get wrong than writing one
line at the time. See "When to cut a release" in `RELEASING.md`.

## Next

_Empty — v0.2.6 shipped the Placid-mode crash fix, the window-drag fix, the
left-dock traffic-light fix, PaddleBoard-branded user-facing text, and made the
default model an explicit choice rather than Zed's hosted models. New bullets
accumulate here as user-facing work lands._
