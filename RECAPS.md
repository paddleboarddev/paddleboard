# Recaps

Running log of completed work sessions, newest first. Each entry summarizes a coherent block of work — features landed, bugs fixed, infrastructure added. Not every chat reply lives here; only completed tasks.

---

## 2026-05-13

### Recap enforcement via Stop hook
- Added `.claude/hooks/recap-enforcer.sh` and `.claude/settings.json` wiring it as a Stop hook. The hook scans the session transcript for Edit/Write/NotebookEdit tool calls and blocks stopping if non-RECAPS files were edited *after* the last RECAPS.md update (or RECAPS was never touched).
- Pipe-tested against six scenarios — all behaved as expected.
- **Exemption added** after first real-world fire: edits under `~/.claude/projects/*/memory/`, `/tmp/`, and `/var/folders/*/T/` are now ignored. Agent bookkeeping (memory writes) and temp scratch shouldn't trigger the recap requirement; real code/config edits still do.
- First activation requires opening `/hooks` once or restarting Claude Code so the settings watcher picks up the new file. `.claude/` is currently untracked — committing the hook script + `.claude/settings.json` makes the enforcement apply for anyone who works on the repo.

### Fork-hygiene infrastructure + weekly upstream merge
- Added `.github/workflows/merge_upstream_zed.yml` — Mondays 16:00 UTC (≈ 09:00 PDT). Fetches `zed/main`, attempts merge, opens a PR (draft if conflicts). PR body lists every `// PaddleBoard`-tagged file touched by the merge.
- Wrote `FORK_HYGIENE.md` (human-facing playbook covering intentional-vs-unintended drift, the `paddleboard_*` crate pattern, `// PaddleBoard: <reason>` tagging convention, asset-drift hazards, merge cadence rationale).
- Added a `# Fork hygiene` section to `.rules` (= `CLAUDE.md`) with six agent-actionable bullets mirroring the playbook.
- Open follow-ups: prune the ~40 inherited Zed workflows in `.github/workflows/` (most are Zed-org-specific and likely fail silently); flip the GitHub repo setting *Settings → Actions → General → Workflow permissions → "Allow GitHub Actions to create and approve pull requests"* before the first scheduled run.

### Telemetry hard-disabled
- `TelemetrySettingsContent::default()` now returns `(diagnostics: false, metrics: false)`.
- `Telemetry::report_event` in `crates/client/src/telemetry.rs` is a no-op — events never queue, log, or reach HTTP. Stale fields/const annotated `#[allow(dead_code)]` to preserve upstream shape.
- Removed two obsolete queueing tests and the `is_empty_state` helper.
- Removed the telemetry toggle section from the onboarding flow (`render_telemetry_section` deleted, divider + call site removed from `render_basics_page`).
- Intentionally preserved: `TelemetrySettings` struct, the `telemetry` / `telemetry_events` crates, and ~59 `telemetry::event!` call sites — keeps upstream merges sane.

### Default terminal shell fix
- `assets/settings/default.json` had `"shell": { "program": "gemini-cli" }` from the 2026-04-18 "Cleanup" commit, causing *failed to spawn terminal* errors. Reverted to `"shell": "system"`.

### Welcome tour synced to WELCOME.md
- Updated `crates/workspace/src/tour.md` to match `WELCOME.md` section ordering. Added Embedded Browser Panel (with Unsloth Studio), Forwarded Ports, Step-Through Mode, Agent Orchestration Panel, and LLM Provider Picker sections. Preserved the tour's compact voice and trailing CTA.
