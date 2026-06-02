# Releasing PaddleBoard (internal)

Internal process doc. Excluded from the public snapshot via `script/publish-public.sh`.

PaddleBoard develops in the **private** repo (`jasonsmithio/paddleboard`, remote `origin`)
and publishes clean squashed snapshots to the **public** repo
(`paddleboarddev/paddleboard`, remote `public`). The public repo shares no history with
private — every release is authored fresh by `script/publish-public.sh`, so internal
files and history never leak.

## Versioning

PaddleBoard uses its **own `0.x` version line**, decoupled from Zed's number.

**The only hard rule: the version must only ever increase.** Auto-update (and users)
treat versions as monotonic. Never number downward.

- Zed's version (`0.232`, `0.233`, …) is a *sequence counter* — `0.232` is much larger
  than `0.3`. That's why we set our own line **now**, before there's an installed base;
  switching to a lower number after shipping + enabling auto-update is a one-way door
  (auto-update would refuse the "older" number).
- The number lives in `crates/paddleboard/Cargo.toml` (`version = "..."`). It's what the
  app reports and what auto-update compares against. The git tag must be `v<that version>`
  or `script/determine-release-channel` (run by `release.yml`) fails.
- **Merge friction:** the weekly upstream merge will keep trying to reset this line to
  Zed's number. Resolve it **keep-ours** every merge — it's one line. `git grep version
  crates/paddleboard/Cargo.toml` to check after a merge.

## Cutting a release

1. Land the changes on private `main` (merge upstream and/or your own work).
2. Bump `crates/paddleboard/Cargo.toml` `version` to the next number; commit on `main`.
3. Publish the source snapshot to the public repo (note: **no** `FIRST_PUBLISH` — this
   *appends* a new commit onto the public history rather than replacing it):

   ```bash
   bash script/publish-public.sh v0.X.0 "Short release notes"
   ```

   This builds a clean full-tree snapshot as a child of the current `public/main`, stamps
   it with the noreply identity, force-free-pushes it, pushes the `v0.X.0` tag, **and
   creates/updates the GitHub Release** for that tag in one go. The public repo grows a
   linear release history (`v0.3.0 → v0.4.0 → …`).

   Release notes default to your message + an auto Full-Changelog compare link to the
   previous tag. For richer notes: `RELEASE_NOTES_FILE=notes.md bash script/publish-public.sh …`.
   To push source + tag only (no Release): `SKIP_RELEASE=1 …`. Releases are marked
   `--prerelease` until the signed-DMG pipeline lands.
4. *(Once the signed-DMG pipeline exists — see below)* the tag triggers the build, which
   uploads the notarized DMG to that same GitHub Release.

## First publish / resetting public history

`FIRST_PUBLISH=1` makes the publish a single orphan root commit and **force-replaces** all
public history. Use it only to (re)initialise the public repo or to renumber the version
line before there's an installed base. It prompts for `replace public history` to confirm.

```bash
FIRST_PUBLISH=1 bash script/publish-public.sh v0.3.0 "PaddleBoard v0.3.0"
```

## Not yet wired up: the binary

Publishing the **source** snapshot is solved. Shipping the **signed/notarized DMG** per
release is not — that's the deferred cross-repo pipeline (build in private → upload the
DMG to the public Release), gated on Apple signing secrets and the runner choice
(GitHub-hosted vs Blacksmith). Until then, "a release" = source + tag, not a downloadable
app. See `~/.claude/plans/auto-update-phase-2-github-releases.md` for how in-app
auto-update consumes these releases.

## Identity / privacy

`publish-public.sh` stamps public commits with `PUBLIC_AUTHOR_NAME` /
`PUBLIC_AUTHOR_EMAIL` (default: name + GitHub noreply address) so the personal email never
lands on a public commit while attribution is preserved. Override via those env vars.
