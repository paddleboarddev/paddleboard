# Fork hygiene

PaddleBoard is a fork of [Zed](https://github.com/zed-industries/zed). Some drift from upstream is the point — that's the differentiation. Unintended drift, on the other hand, is what makes long-lived forks die. This document is the playbook for keeping the *unintended* drift small and the merge tax low.

## Mental model

There are two kinds of drift:

- **Intentional drift** — features that don't exist upstream (sandbox, browser panel, sandboxed MCP, orchestration panel, the Vertex AI LLM provider), or upstream behavior we deliberately disable (telemetry, Zed Pro upsells). This is the value we're adding; protect it. The Vertex provider lives mostly in the `paddleboard_vertex` crate, but registers via small tagged edits in `language_models` + a `vertex` field in `settings_content`; if an upstream merge adds its own Vertex provider, reconcile rather than duplicate.
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

## Watch for dropped files in renamed crates

A second silent class, and a nastier one — it produces no diff to review, because the file simply isn't there.

PaddleBoard renamed four upstream crate directories:

| upstream | PaddleBoard |
| --- | --- |
| `crates/zed` | `crates/paddleboard` |
| `crates/zed_actions` | `crates/paddleboard_actions` |
| `crates/zed_credentials_provider` | `crates/paddleboard_credentials_provider` |
| `crates/zed_env_vars` | `crates/paddleboard_env_vars` |

When upstream **adds** a file inside one of these, git has no PaddleBoard-side counterpart to conflict against. The merge completes cleanly, the file is dropped (or resurrects a stray `crates/zed/` tree), and nothing fails to compile — our renamed crate never referenced it. On 2026-07-20 four `reliability/hang_detection` files vanished exactly this way, with no conflict and no error.

`script/check-dropped-upstream-files` catches it:

```bash
./script/check-dropped-upstream-files            # exit 1 if anything was dropped
./script/check-dropped-upstream-files --list     # show every mapping and its resolution
```

It compares the **merge base** — the newest upstream commit we've actually merged — against `HEAD`, so files upstream added but we haven't merged yet are correctly ignored. Deliberate non-ports go in `script/dropped-upstream-files-allowlist.txt` with a reason; entries that stop matching are reported as stale so the list can't rot.

The weekly merge workflow runs this automatically and surfaces any hits in the PR body. It does **not** fail the job — a dropped file should be reviewed by a human, not silently block the merge PR from opening.

**If you rename another upstream directory, add it to `RENAME_MAP` in the script.** Additions inside an unmapped directory are invisible to the check — which is the same blind spot the script exists to close.

## Watch for zombie files in deleted crates

The same git behavior runs in the other direction. PaddleBoard **deleted** five upstream directories outright:

`crates/call` · `crates/collab` · `crates/collab_ui` · `crates/livekit_api` · `crates/livekit_client`

When upstream *modifies* a file we deleted, the merge raises a modify/delete conflict and a human sees it. When upstream *adds a new file* there, nothing opposes the add — it merges cleanly and the file **resurrects** in a directory we killed. It never compiles (the crate isn't a workspace member), so nothing goes red. By July 2026, 30 such files had quietly accumulated; they were removed in #89.

`script/check-zombie-upstream-files` guards this:

```bash
./script/check-zombie-upstream-files          # exit 1 if zombies exist
./script/check-zombie-upstream-files --fix    # git rm them (staged; you commit)
```

It's just `git ls-files` over the deleted directories — no upstream fetch — so it runs on **every PR** via `public-embeds.yml`, which also covers hand-resolved merges. The weekly merge workflow additionally surfaces zombies in the PR body.

Two maintenance rules: if the fork deletes another upstream directory, add it to `DELETED_DIRS` in the script. And if you ever want to *adopt* a file upstream added in one of these directories, that's a decision to un-delete the directory — remove it from `DELETED_DIRS` and restore it properly; there's deliberately no per-file allowlist.

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

`.github/workflows/` contains **only PaddleBoard-authored workflows — nothing inherited from Zed remains**. That invariant is the durable claim; the current set (as of 2026-07) is:

| workflow | purpose |
| --- | --- |
| `merge_upstream_zed.yml` | weekly upstream merge (+ dropped-file and zombie reports in the PR body) |
| `compile-check.yml` | compile gate: Linux full workspace every PR; macOS platform-crate list on `main` pushes only (10x cost) |
| `test.yml` | test gate: runs the `paddleboard*` crates' tests on Linux |
| `public-embeds.yml` | per-PR hygiene: embeds, zed-isms, llama pins, zombie files |
| `upstream-drift.yml` | drift report, sticky PR comment + weekly |
| `upstream-drift-baseline.yml` | re-saves the drift baseline in a bot PR once an upstream merge PR lands |
| `release.yml` | tag-triggered signed/notarized DMG + Linux tarball |
| `watch-llama-cpp-advisories.yml` | llama.cpp security watcher |
| `watch-scion-releases.yml` | Scion release watcher |

The original ~40 inherited workflows were Zed-org-specific (collab deploys, community labelers, signing/notarization for Zed's release pipeline, Slack-bound stale-PR reminders, Factory.ai docs-suggestion bots, etc.) and either failed silently or did nothing useful in this fork. The cleanup happened in three passes: PR #13 (initial sweep of org-only assign-reviewers), PR #14 (Zed-org-specific workflows), PR #15 (`run_tests.yml`), PR #16 (the final batch of seven).

One of those removals left a hole worth naming: dropping upstream's `run_tests.yml` (commit `272c536d38`, 2026-05-13) was correct on its own terms — it ran Zed's full suite against Zed's infrastructure — but nothing replaced it, so for **~11 weeks** no workflow in this repo ran a single test, until `test.yml` was added on 2026-07-27. When you delete an inherited workflow, check whether it was the only thing covering a category.

Three of the removed workflows — `autofix_pr.yml`, `compare_perf.yml`, `run_bundling.yml` — are generated by `cargo xtask workflows`. If anyone runs that command they reappear. Followup pass needed in `crates/xtask/` to drop the generators.

When the weekly upstream merge brings in a new workflow, decide at PR review time whether it belongs in the fork. Default to deleting on the merge branch before marking the PR ready.

## When in doubt

Ask: "would I want this change to survive an upstream merge in 6 months?"

- **Yes** — make sure it's tagged with `// PaddleBoard:` (or lives in a `paddleboard_*` crate where upstream can't reach it).
- **No** — don't make the change. Configure upstream's existing knob, file an upstream issue, or do it in a separate consumer crate.
