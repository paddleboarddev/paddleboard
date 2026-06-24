# Unreleased

Staging area for the next release's notes. Add a bullet here as user-facing
work lands on `main`; at release time, build clean release notes from these
bullets and publish with:

```
RELEASE_NOTES_FILE=<clean-notes.md> bash script/publish-public.sh v0.1.5 "…"
```

Then reset this file back to an empty `## Next` section for the cycle after. Use
the `- Added` / `- Fixed` / `- Improved` convention from CLAUDE.md.

## Next (v0.1.5)

- Fixed `script/install.sh` failing with `SHELL: parameter not set` in environments without a login shell (Docker builds, CI).
- Added "Build an MCP" — for a service with no MCP server, the AI Dock's MCP tab can generate one: name the service and describe what you want, and an agent researches the API, writes a Python server, tests it in the sandbox, and installs it.
