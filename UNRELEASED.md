# Unreleased

Staging area for the next release's notes. Add a bullet here as user-facing
work lands on `main`; at release time, cut a version and publish with:

```
RELEASE_NOTES_FILE=UNRELEASED.md bash script/publish-public.sh v0.1.2 "…"
```

Then reset this file back to an empty `## Next (v0.1.2)` section for the cycle
after. Use the `- Added` / `- Fixed` / `- Improved` convention from CLAUDE.md.

## Next (v0.1.2)

- Fixed the `paddleboard` CLI command silently breaking after a local install — `script/bundle-mac -i` now refreshes the `/usr/local/bin/paddleboard` symlink to point at the freshly-installed app on every install.
- Fixed the CLI still saying "Zed" in user-facing output — the first-run open-behavior prompt and `paddleboard --help` text now say PaddleBoard and show `paddleboard …` example commands.
- Improved the Google enterprise LLM provider's branding to match Google's current naming — shown as "Gemini Enterprise (Vertex AI)" (was "Vertex AI (Gemini Enterprise)"), and the cosmetic UI now says "Gemini Enterprise" (the underlying API/auth messages still say Vertex, which is correct).
