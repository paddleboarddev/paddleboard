# Unreleased

Staging area for the next release's notes. Add a bullet here as user-facing
work lands on `main`; at release time, build clean release notes from these
bullets and publish with:

```
RELEASE_NOTES_FILE=<clean-notes.md> bash script/publish-public.sh v0.X.Y "…"
```

Then reset this file back to an empty `## Next

- Added a keybinding and a View menu entry for the Agent Threads panel, which shows
  multi-agent sessions as a tree (`cmd-k cmd-g` on macOS, `ctrl-k ctrl-g` on Linux and
  Windows).
- Improved the Agent Threads tree: each row now shows its agent's own icon and name, flags
  a thread that is waiting for your confirmation, and marks threads that are running tools.
