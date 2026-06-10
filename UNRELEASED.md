# Unreleased

Staging area for the next release's notes. Add a bullet here as user-facing
work lands on `main`; at release time, build clean release notes from these
bullets and publish with:

```
RELEASE_NOTES_FILE=<clean-notes.md> bash script/publish-public.sh v0.1.3 "…"
```

Then reset this file back to an empty `## Next` section for the cycle after. Use
the `- Added` / `- Fixed` / `- Improved` convention from CLAUDE.md.

## Next (v0.1.3)

- Fixed building PaddleBoard from the public repository failing with `couldn't read .claude/commands/update-tour.md` (and friends) — the AI Dock's bundled skill bodies are now embedded from `assets/ai_dock/skills/`, which ships in the public source snapshot.
