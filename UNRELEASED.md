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

- Fixed Linux builds failing to link the static musl `remote_server` with GCC ≥ 14 (`undefined reference to __isoc23_sscanf` from aws-lc) — rustls now uses the `ring` crypto provider, removing aws-lc from that build entirely.
- Fixed macOS bundles not using the git binary they ship — the build exported `ZED_BUNDLE` while the app checks `PADDLEBOARD_BUNDLE` at compile time, so PaddleBoard silently fell back to the system git instead of the bundled one.
- Fixed Linux source installs not getting the "you built from source — rebuild to update" message baked in (same env-var mismatch, `ZED_UPDATE_EXPLANATION` vs `PADDLEBOARD_UPDATE_EXPLANATION`).
- Added Git Login v2: the Manage modal lists each provider with live sign-in status and one-click removal; git's password prompt gains a "Remember on this device" checkbox; saved GitHub tokens also authenticate GitHub API requests (blame avatars on private repos, higher rate limits); and builds configured with an OAuth client id offer "Sign in with GitHub (browser)" via the device flow.
- Added a local-only agent context gauge to the status bar — see how much of the model's context window the active thread has used, with a token breakdown on hover. Computed entirely on your machine; telemetry remains hard-disabled.
- Improved fork hygiene: all packaging scripts now use `PADDLEBOARD_*` environment variables, and CI fails if a `ZED_*` variable sneaks back in.
- Improved the README's Linux build instructions: the prerequisites callout now appears in the quick path (run `./script/linux` first).
