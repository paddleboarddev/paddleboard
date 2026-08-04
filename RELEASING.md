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
- **Three components only. A four-component version cannot exist here.** `0.2.4.1` is not
  a hotfix number we've chosen not to use — it is unrepresentable. Cargo rejects it at
  parse (`unexpected character '.' after patch version number`), so the crate won't
  build; and `script/determine-release-channel` validates the tag against the crate
  version, so the workflow could neither build nor tag it. Worse, `paddleboard_releases::
  select_release` parses every tag with `Version::parse` and `filter_map`s away whatever
  fails — so a `v0.2.4.1` release would publish successfully, look right on the releases
  page, and be **silently skipped by every installed copy**. When the urge for "just a
  small patch number" arrives, the answer is the next patch (`0.2.6`), carrying the batch.

## When to cut a release

**Fixes land on `main` continuously. Releases are cut deliberately, and the version bump
happens at cut time carrying everything accumulated since the last one.**

The distinction matters because the two costs are wildly different. A version number is
free. A release costs a ~2-hour signed-and-notarized pipeline, a manual publish (the
classifier blocks agents from running `publish-public.sh`), and a promotion step. Cutting
one release per fix spends that repeatedly for no user-visible benefit — on 2026-08-03
v0.2.4 and v0.2.5 each carried a single fix, and the following day produced four more
fixes that would have been four more releases at that rate.

`UNRELEASED.md` is the staging mechanism: add a user-facing bullet there as work lands,
and build the release notes from it at cut time.

**Two triggers:**

1. **Severity — cut now, don't wait.** A crash, data loss, or anything that makes the app
   unusable. Both of these cleared that bar and sat in the published build for days: the
   main window could not be moved by its title area, and clicking the Placid status-bar
   button killed the app on every click.
2. **Cadence — everything else waits.** Batch it into the next scheduled cut. A docs link
   pointing at the wrong site, a cosmetic overlap, a dependency bump: none of these are
   worth a pipeline run of their own.

If you find yourself wanting to release because a fix *feels* finished, that's the cadence
case. "Finished" and "released" are deliberately decoupled.

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
   previous tag. For richer notes: `RELEASE_NOTES_FILE=notes.md bash script/publish-public.sh …`
   — build them from `UNRELEASED.md`. To push source + tag only (no Release):
   `SKIP_RELEASE=1 …`.
4. The tag triggers `release.yml`, which builds, signs, notarizes and attaches the DMG and
   Linux tarball to that same Release (see [The binary](#the-binary)).
5. **Promote it.** Releases are created as prereleases; until promoted, the updater skips
   them and users are offered nothing. Verify afterwards that `isPrerelease` is `false`.
6. Reset `UNRELEASED.md` to an empty `## Next` section for the following cycle.

**Run the publish from `main`, with a clean checkout.** `publish-public.sh` snapshots
`git archive HEAD` — whatever branch is checked out, not `origin/main`. On both v0.2.4 and
v0.2.5 it ran while the working copy still sat on the release branch, and the public repo
briefly carried a version `main` had not recorded. Merge the bump first, then publish.

## First publish / resetting public history

`FIRST_PUBLISH=1` makes the publish a single orphan root commit and **force-replaces** all
public history. Use it only to (re)initialise the public repo or to renumber the version
line before there's an installed base. It prompts for `replace public history` to confirm.

```bash
FIRST_PUBLISH=1 bash script/publish-public.sh v0.3.0 "PaddleBoard v0.3.0"
```

## The binary

**Live since v0.1.14.** The tag triggers `release.yml`, which builds, signs, notarizes and
staples on a `macos-15` runner (Team `H7YGQM6U9W`) and attaches the assets to the GitHub
Release: `PaddleBoard-aarch64.dmg` and `paddleboard-linux-x86_64.tar.gz`. A release is a
downloadable app, not just source + tag.

Two things the updater cares about, both worth checking after a cut:

- **`prerelease` must end up `false`.** `release.yml` creates releases as prereleases and
  they are promoted afterwards. `paddleboard_releases::select_release` filters prereleases
  out unless the user opted in, so an unpromoted release reaches nobody. Verify with
  `gh release view v0.X.Y --repo paddleboarddev/paddleboard --json isPrerelease`.
- **Asset names must match** `paddleboard_releases::asset_name` exactly, or the updater
  finds a release it cannot install and skips it.

Expect the build to take roughly 110–190 minutes; the long runs track CI contention, not
trouble. Failures arrive fast, so a *long* run is the healthy signal. See
`~/.claude/plans/auto-update-phase-2-github-releases.md` for how in-app auto-update
consumes these releases.

## Identity / privacy

`publish-public.sh` stamps public commits with `PUBLIC_AUTHOR_NAME` /
`PUBLIC_AUTHOR_EMAIL` (default: name + GitHub noreply address) so the personal email never
lands on a public commit while attribution is preserved. Override via those env vars.
