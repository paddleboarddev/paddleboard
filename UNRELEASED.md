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

_Empty — v0.1.8.1 patched two agent-default bugs (new drafts now use the native agent instead of external Gemini; the new-Gemini-thread action no longer opens a native thread). v0.1.8 before it shipped the weekly upstream Zed sync (~236 commits) + the wasmtime 36.0.10 WASI sandbox-escape fix. New bullets accumulate here as user-facing work lands._
