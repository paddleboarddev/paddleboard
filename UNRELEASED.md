# Unreleased

Staging area for the next release's notes. Add a bullet here as user-facing
work lands on `main`; at release time, build clean release notes from these
bullets and publish with:

```
RELEASE_NOTES_FILE=<clean-notes.md> bash script/publish-public.sh v0.1.4 "…"
```

Then reset this file back to an empty `## Next` section for the cycle after. Use
the `- Added` / `- Fixed` / `- Improved` convention from CLAUDE.md.

## Next (v0.1.4)

- Fixed macOS bundles not using the git binary they ship — the build exported `ZED_BUNDLE` while the app checks `PADDLEBOARD_BUNDLE` at compile time, so PaddleBoard silently fell back to the system git instead of the bundled one.
- Fixed Linux source installs not getting the "you built from source — rebuild to update" message baked in (same env-var mismatch, `ZED_UPDATE_EXPLANATION` vs `PADDLEBOARD_UPDATE_EXPLANATION`).
- Improved fork hygiene: all packaging scripts now use `PADDLEBOARD_*` environment variables, and CI fails if a `ZED_*` variable sneaks back in.
