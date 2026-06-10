---
description: Sync the in-app tour (crates/workspace/src/tour.md) with the canonical WELCOME.md after a user-facing feature lands
allowed-tools: ["Read", "Edit", "Write", "Bash"]
---

# /update-tour — Sync the welcome tour with WELCOME.md

`WELCOME.md` is the canonical, long-form PaddleBoard welcome doc that lives at the repo root.

`crates/workspace/src/tour.md` is a shorter, in-app tour that gets compiled into the binary and shown to users in the welcome tab. It mirrors `WELCOME.md` but in a more compact, command-palette-focused, emoji-friendly voice.

This command reconciles them after a user-facing feature has been added (typically you also just edited `WELCOME.md`).

## Steps

1. **Read both files** in parallel:
   - `WELCOME.md`
   - `crates/workspace/src/tour.md`

2. **Diff the feature coverage.** For each feature/section in `WELCOME.md`, check whether it's represented in `tour.md`. Look especially for:
   - New sections added recently to `WELCOME.md` but missing from `tour.md`.
   - Sections present in both but materially out of date in `tour.md` (e.g. command names, settings keys, panel names that have changed).
   - Sections in `tour.md` that no longer match `WELCOME.md` (renamed feature, removed flag, etc.).

3. **Update `tour.md`** to bring it in line:
   - Preserve `tour.md`'s existing voice: shorter sentences, sparing emoji, lots of `**command palette name**` and keyboard shortcuts, fewer external links.
   - Match the section ordering of `WELCOME.md` unless `tour.md`'s order is clearly intentional.
   - Don't bloat `tour.md` — it's a tour, not a manual. Each feature gets 2–4 bullets at most.
   - Keep the trailing closing/CTA section (it usually says something like "Happy paddling!" — preserve that flavor).

4. **Report what changed.** After editing, give the user a one-paragraph summary of which sections you added, updated, or removed and why.

## Things to be careful about

- Do not invent features. Only sync what's already documented in `WELCOME.md`.
- Do not turn `tour.md` into a copy of `WELCOME.md` — they have different jobs.
- If `tour.md` and `WELCOME.md` disagree on a fact (e.g. an action name), prefer `WELCOME.md` but flag the discrepancy in your report so the user can double-check.
- If nothing has drifted, say so and don't make a cosmetic edit.

User input (optional focus area): $ARGUMENTS
