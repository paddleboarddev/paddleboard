# Unreleased

Staging area for the next release's notes. Add a bullet here as user-facing
work lands on `main`; at release time, build clean release notes from these
bullets and publish with:

```
RELEASE_NOTES_FILE=<clean-notes.md> bash script/publish-public.sh v0.X.Y "…"
```

Then reset this file back to an empty `## Next` section.

## Next

- Changed: PaddleBoard now needs macOS 12 Monterey or later (was 10.15), following Zed.
