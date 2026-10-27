# Unreleased

Staging area for the next release's notes. Add a bullet here as user-facing
work lands on `main`; at release time, build clean release notes from these
bullets and publish with:

```
RELEASE_NOTES_FILE=<clean-notes.md> bash script/publish-public.sh v0.X.Y "…"
```

Then reset this file back to an empty `## Next` section.

## Next

- Fixed the bundled fallback certificate store still trusting the emSign ECC Root CA - G3,
  whose intermediate CAs had their private keys compromised. It is only used when the
  system's certificate store cannot be loaded.
