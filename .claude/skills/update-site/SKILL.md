---
name: update-site
description: Sync the paddleboard.dev marketing site with the README. Use after a README change that alters how PaddleBoard is described or which features it advertises. Reads this repo's `README.md`, updates the site's `hugo.toml` params (tagline/subtitle/description) and feature cards in the SEPARATE `paddleboarddev/site` repo, builds, and opens a PR there. Translates dev-detailed README prose into short marketing copy — it does not copy verbatim.
---

# Update the PaddleBoard website from the README

The website at [paddleboard.dev](https://paddleboard.dev) lives in a **separate repo** (`paddleboarddev/site`), not in this one. Its homepage copy is data-driven from `hugo.toml` `[params]` — that's the sync target. Pushing to the site repo's `main` triggers the GitHub Pages deploy (`.github/workflows/hugo.yml`).

This is the website analogue of `/update-tour` (which syncs the in-app `tour.md` from `WELCOME.md`). The README is the source of truth; the site is a **compressed, marketing-toned translation** of it — never a verbatim copy.

## What this skill does

Bring the site's homepage copy in line with the current `README.md`: the one-line pitch, the intro paragraph, and the list of headline features.

## Procedure

1. **Locate the site checkout.** Look for `~/Projects/Tools/paddleboard-site`. If it's missing, clone it: `gh repo clone paddleboarddev/site ~/Projects/Tools/paddleboard-site`. Then `git -C <site> checkout main && git -C <site> pull --ff-only`. Create a working branch (e.g. `sync-readme`) — **never edit the site's `main` directly.**
2. **Read the source.** In this repo, read `README.md` — specifically the intro paragraph (the pitch) and the "What's different from Zed" bullets (the feature list). Optionally `git log -1 --format=%s README.md` / the recent diff to see what changed.
3. **Read the current site copy.** In the site repo, read `hugo.toml` — the `[params]` block (`tagline`, `subtitle`, `description`) and the `[[params.features]]` entries (each has `icon`, `title`, `body`).
4. **Reconcile, don't dump.** Update params where the README's framing has drifted:
   - `tagline` — the short hero headline (≤ ~8 words).
   - `subtitle` / `description` — 1-2 sentences distilled from the README intro.
   - `[[params.features]]` — one card per headline differentiator. Keep ~4-6 cards; each is an emoji `icon`, a short `title`, and a 1-2 sentence benefit-oriented `body`. Add cards for new README features, revise drifted ones, drop ones the README no longer claims. **Rewrite into marketing voice** — short, benefit-first — not the README's dev detail.
   - Leave layout/CSS (`layouts/`, `static/css/`) alone unless the structure itself must change.
5. **Build to verify.** From the site repo: `hugo --gc --minify`. It must finish with **no warnings** (the config uses `[languages.en] locale/label` + `disableKinds` for taxonomies — keep it warning-clean).
6. **Open a PR on the site repo.** Commit on the working branch, push, and `gh pr create --repo paddleboarddev/site`. Mention it deploys to paddleboard.dev on merge. Don't merge unless asked.
7. **Report** what changed in 1-3 lines.

## When NOT to run

- The README change is internal / non-user-facing (build notes, contributor docs, license text).
- The site intentionally diverges from the README (e.g. seasonal copy, a campaign).
- The README edit only touched the "not Apple-notarized" / status disclaimers — those have their own spot on the site (the CTA band); update that line directly rather than regenerating feature cards.

## Style anchors

- The site is **shorter and warmer** than the README. Features are a glanceable icon + title + one benefit sentence, not a spec.
- Match the existing nautical/PaddleBoard tone; keep the Catppuccin-derived palette and layout intact.
- If a feature needs more than ~2 sentences to explain, it probably belongs in the docs, not a homepage card.
