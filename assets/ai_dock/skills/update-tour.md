---
description: Review the in-app tour (crates/workspace/src/tour.md) after a user-facing feature lands — a curated ~6-stop guide, NOT a mirror of WELCOME.md
allowed-tools: ["Read", "Edit", "Write", "Bash"]
---

# /update-tour — Review the curated welcome tour

`crates/workspace/src/tour.md` is a short, in-app **curated getting-started tour** compiled into the binary and shown on first launch. As of the Wave 4 Glowup it is **decoupled from `WELCOME.md`** — it is NOT a mirror. `WELCOME.md` (repo root) and [docs.paddleboard.dev](https://docs.paddleboard.dev) are the full feature reference; the tour is the ~6-stop on-ramp for a new user's first 15 minutes.

The six stops: (1) Agent + provider, (2) Manifest/git, (3) Sandboxing, (4) Set Sail, (5) Personas, (6) AI Dock.

## Steps

1. **Read** `crates/workspace/src/tour.md` and check the branch diff (`git log main..HEAD --oneline`).
2. **Decide whether the tour should change at all.** Most features do NOT earn a slot. Edit the tour only if the change materially affects one of the six stops, or is a genuine first-15-minutes headline capability that should *replace* a weaker stop (keep it at ~6 — add one, cut one). Otherwise the feature belongs in `WELCOME.md`/docs, and you should say so and stop.
3. **If it qualifies**, edit the relevant `## N. …` stop in place. Preserve the tour's voice: one framing sentence + 2–4 skimmable, action-verb bullets; keep each stop under ~half a screen. Keep the intro ("six stops") and the closing (points to WELCOME/docs + how to reopen) intact; if the stop count changes, fix the intro wording.
4. **Report** in 1–3 lines what changed and why it earned a tour edit (or why nothing did).

## Be careful

- Do not auto-expand the tour to cover every feature — a longer tour is a worse tour.
- Do not turn `tour.md` back into a copy of `WELCOME.md`; they have different jobs.
- Don't touch the runtime copy at `~/.config/paddleboard/PaddleBoard_Tour.md` — `crates/paddleboard/src/tour.rs` re-materializes it on launch (content-hash based; users get a "tour has new sections" toast automatically).
- If nothing has drifted, say so and don't make a cosmetic edit.

User input (optional focus area): $ARGUMENTS
