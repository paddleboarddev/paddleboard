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

_Empty — v0.1.7 shipped the working Build an MCP install: the host-side `install_mcp_server` tool plus forcing the flow onto the native agent so the install always lands (correct data-dir path, plain Stdio entry, no leftover sandbox-only fields) regardless of the selected agent. New bullets accumulate here as user-facing work lands._
