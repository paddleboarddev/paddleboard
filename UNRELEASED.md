# Unreleased

Staging area for the next release's notes. Add a bullet here as user-facing
work lands on `main`; at release time, build clean release notes from these
bullets and publish with:

```
RELEASE_NOTES_FILE=<clean-notes.md> bash script/publish-public.sh v0.X.Y "…"
```

Then reset this file back to an empty `## Next` section.

## Next

- Fixed four security issues in the TLS library (rustls 0.23.46), including TLS session
  secrets that could appear in debug output.
- Fixed the macOS window buttons overlapping the tab bar's back and forward arrows on some
  macOS versions, and the tab bar's bottom line stopping short under the window buttons.
- Fixed the bundled fallback certificate store still trusting the emSign ECC Root CA - G3,
  whose intermediate CAs had their private keys compromised. It is only used when the
  system's certificate store cannot be loaded.
- Fixed a crash when an extension receives a component-model stream with a different
  payload type than it expects (wasmtime 48.0.7, GHSA-9pvw-hwfg-92h5).
- Fixed Vim and Helix mode loading none of their key bindings, because one binding pointed
  at an action that no longer exists.
- Added Spotter to the first-launch tour. Existing users see a "tour has new sections" notice.
