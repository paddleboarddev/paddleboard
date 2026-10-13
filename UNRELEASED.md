# Unreleased

Staging area for the next release's notes. Add a bullet here as user-facing
work lands on `main`; at release time, build clean release notes from these
bullets and publish with:

```
RELEASE_NOTES_FILE=<clean-notes.md> bash script/publish-public.sh v0.X.Y "…"
```

Then reset this file back to an empty `## Next` section.

## Next

<!-- Check before building v0.3.0's notes: #227 "Open the AI Dock instead of toggling it"
     merged 2026-09-09T16:01Z, twelve hours AFTER v0.2.8 was tagged (03:27Z), so by timing
     it is unreleased. But v0.2.8's published notes already list "The AI Dock sometimes
     doing nothing when opened from the command palette" under Fixed, and this file's own
     post-release placeholder (2026-09-10) recorded the AI Dock palette fix as shipped.
     Either it was included and needs no bullet, or it was missed and needs one. -->

- Added Spotter: run routine agent work on a local model and move it to a stronger
  one when the project's check fails twice. Off by default; enable with
  `"paddleboard_spotter"` in settings and a check command per project. Escalation asks
  first unless set to `"auto"`.
- Added a keybinding and a View menu entry for the Agent Threads panel, which shows
  multi-agent sessions as a tree (`cmd-k cmd-g` on macOS, `ctrl-k ctrl-g` on Linux and
  Windows).
- Improved the Agent Threads tree: each row now shows its agent's own icon and name, flags
  a thread that is waiting for your confirmation, and marks threads that are running tools.
