---
name: update-tour
description: Sync `crates/workspace/src/tour.md` (the in-app first-launch tour) with the current PaddleBoard feature set. Use after landing a user-facing change — reads `WELCOME.md` and the current branch's diff, then writes matching sections into `tour.md`. Edits the source file only; the runtime copy at `~/.config/paddleboard/PaddleBoard_Tour.md` is materialized at first launch.
---

# Update PaddleBoard tour

The in-app tour shown on first launch lives at `crates/workspace/src/tour.md`. It's embedded into the binary via `include_str!` and written to `~/.config/paddleboard/PaddleBoard_Tour.md` on first launch. **Never edit the runtime copy** — only the source.

## What this skill does

Bring `crates/workspace/src/tour.md` in line with the canonical feature list in `WELCOME.md` (and any new features added on the current branch).

## Procedure

1. **Read both files** — `WELCOME.md` and `crates/workspace/src/tour.md`. The repo root has WELCOME; the tour is under `crates/workspace/src/`.
2. **Check the branch diff** with `git log main..HEAD --oneline` and `git diff --stat main..HEAD WELCOME.md` to see what was added on this branch. If `WELCOME.md` was updated on this branch, those new sections are top priority.
3. **Identify gaps** — diff the conceptual feature lists between WELCOME.md and tour.md. The tour is more compressed and emoji-friendly than WELCOME; missing features show up as `### N. Feature Name` headers present in WELCOME but absent in tour.
4. **Write the new sections** into `tour.md`, matching its existing style:
   - Numbered headers under `## 🚀 Key Features`, e.g. `### 5. Forwarded Ports`
   - 2–4 short bullets per feature — the tour is *visual and skimmable*, not exhaustive
   - Use an emoji where it adds value, sparingly
   - Renumber subsequent headers if you insert in the middle
5. **Don't touch the runtime file.** `~/.config/paddleboard/PaddleBoard_Tour.md` is materialized from the source.
6. **Flag the sticky-file behavior** if the user might ship this to existing users: `workspace.rs:785` and `paddleboard/src/main.rs:1494` write the tour file only when it doesn't exist, so existing users won't see updates until they delete the file or the gate is fixed.
7. **Report back** what was added in 1–3 lines.

## When NOT to run

- The change is internal / refactoring only (no user-visible feature)
- `WELCOME.md` is already out of sync with the codebase — fix WELCOME first, then run this
- The new feature is opt-in for power users (e.g. a settings flag) and not worth surfacing on first launch

## Style anchors

The existing tour uses:
- Imperative, second-person voice ("Open the assistant panel, give it a goal...")
- Numbered top-level features under `## 🚀 Key Features`
- Bullets prefixed with action verbs
- Short — most sections are 4–7 lines

Match that. The tour is the first thing a new user sees; if a section is longer than half a screen, trim it.
