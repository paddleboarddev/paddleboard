# Unreleased

Staging area for the next release's notes. Add a bullet here as user-facing
work lands on `main`; at release time, build clean release notes from these
bullets and publish with:

```
RELEASE_NOTES_FILE=<clean-notes.md> bash script/publish-public.sh v0.1.5 "…"
```

Then reset this file back to an empty `## Next` section for the cycle after. Use
the `- Added` / `- Fixed` / `- Improved` convention from CLAUDE.md.

## Next

_Empty — v0.1.8 shipped the weekly upstream Zed sync (~236 commits) with all PaddleBoard features preserved, plus the wasmtime 36.0.10 WASI sandbox-escape security fix. New bullets accumulate here as user-facing work lands._
