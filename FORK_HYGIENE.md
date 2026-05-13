# Fork hygiene

PaddleBoard is a fork of [Zed](https://github.com/zed-industries/zed). Some drift from upstream is the point — that's the differentiation. Unintended drift, on the other hand, is what makes long-lived forks die. This document is the playbook for keeping the *unintended* drift small and the merge tax low.

## Mental model

There are two kinds of drift:

- **Intentional drift** — features that don't exist upstream (sandbox, browser panel, sandboxed MCP, orchestration panel), or upstream behavior we deliberately disable (telemetry, Zed Pro upsells). This is the value we're adding; protect it.
- **Unintended drift** — assets we forgot we edited, refactors that touched files we didn't need to touch, defaults that diverged silently. This is the rot; eliminate it.

The practices below all reduce one or the other.

## Where new code goes

**Default: new features go in new crates.** Upstream Zed cannot conflict on a file it has never seen. The fork already does this: `paddleboard`, `paddleboard_actions`, `paddleboard_credentials_provider`, `paddleboard_env_vars`. New features should follow the same pattern — pick a `paddleboard_*` name and live there.

**When you must edit an upstream-shaped file**, three rules:

1. **Keep the diff minimal.** A 1-line `#[allow(dead_code)]` is dramatically cheaper to merge than a 50-line refactor. Prefer narrow surgical changes over "while I'm in here…" cleanup.
2. **Tag the divergence with a `// PaddleBoard:` comment** explaining *why* this is fork-specific. Future merge resolutions become mechanical when the comment tells you which side to keep.
3. **Prefer extension points over rewrites.** If upstream exposes a trait, an option struct, or a settings field that lets you change behavior without forking the function body, use it.

### The `// PaddleBoard:` convention

Every intentional divergence in an upstream-shaped file should be greppable with `git grep "// PaddleBoard:"`. Examples already in the tree:

```rust
// PaddleBoard: Zed Pro trial upsell is disabled.
fn should_render_trial_end_upsell(&self, _cx: &mut Context<Self>) -> bool {
    false
}
```

```rust
// PaddleBoard: telemetry is hard-disabled — events drop here and never queue,
// log, or reach HTTP. Keeping the function shape preserves the call-site
// contract so upstream merges don't churn this file.
fn report_event(self: &Arc<Self>, _event: Event) {}
```

Section headers without an explanation (`// PaddleBoard Webview Integration`) are fine for net-new code regions inside a shared file, but anywhere we're *replacing* or *disabling* upstream behavior, prefer the `// PaddleBoard: <reason>` form.

## Watch the non-Rust drift

Compile-time drift gets caught by `cargo check` or by warnings. **Asset drift is silent**: `assets/settings/default.json`, `assets/keymaps/`, icons, license files, the welcome doc. We've already eaten one production incident this way (the `gemini-cli` shell default-bug, May 2026).

After every upstream merge, run:

```bash
git diff <merge-base> HEAD -- assets/ docs/
```

and verify every divergence is intentional. If you find one that isn't, revert it on the merge branch before marking the PR ready.

## Merge cadence

A scheduled workflow (`.github/workflows/merge_upstream_zed.yml`) runs every Monday at 16:00 UTC. It:

1. Fetches `zed/main`.
2. Creates a branch `chore/merge-upstream-zed-YYYY-MM-DD`.
3. Attempts the merge.
4. If clean: opens a normal PR for review.
5. If conflicts: commits the conflict markers, opens a **draft** PR with a list of conflicted files in the body.

The PR body also lists every `// PaddleBoard`-tagged file that received upstream changes — that's the manual-review hotspot list.

The job can be triggered on demand via the **Actions** tab → *Merge upstream Zed* → *Run workflow*.

**Cadence rule:** don't skip a merge if a clean one is sitting there. The cost of resolving conflicts is roughly quadratic in time-since-last-merge — three skipped weeks is much more than 3× the work of three handled weeks.

## Inherited Zed workflows

`.github/workflows/` currently contains ~40 workflows inherited from upstream Zed. Most are Zed-org-specific (collab deploys, community labelers, weekly Linear digests) and either fail silently or do nothing useful in this fork. They are not actively maintained here and should be pruned opportunistically — but pruning the wrong one can break something subtle (e.g. `run_tests.yml`, `release.yml`), so do it deliberately, not in a sweep.

When upstream adds a new workflow that lands during the weekly merge, decide at review time whether it belongs in the fork.

## When in doubt

Ask: "would I want this change to survive an upstream merge in 6 months?"

- **Yes** — make sure it's tagged with `// PaddleBoard:` (or lives in a `paddleboard_*` crate where upstream can't reach it).
- **No** — don't make the change. Configure upstream's existing knob, file an upstream issue, or do it in a separate consumer crate.
