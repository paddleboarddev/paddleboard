# Recaps

Running log of completed work sessions, newest first. Each entry summarizes a coherent block of work — features landed, bugs fixed, infrastructure added. Not every chat reply lives here; only completed tasks.

---

## 2026-05-14

### Rebase modal-UX PR onto post-enforcement `main`
- PR #26 went red shortly after open because enforcement PR #25 landed on `main` in the interval. Both touched the same files: UI crate `Cargo.toml` (`gpui_tokio` removed / `paddleboard_sandbox_prereqs_state` added by enforcement vs. `log` + `which` added by modal), UI crate `.rs` (state-crate extraction by enforcement vs. modal-UX additions), `Cargo.lock`, and `RECAPS.md`.
- Resolution: `git rebase origin/main` on the modal branch and resolved all four conflicts in favour of "keep the enforcement-side change, layer modal-UX on top." Net effect on the UI crate: dropped the temporary `gpui_tokio` import (no longer needed now that the global lives in the state crate), kept `paddleboard_sandbox_prereqs_state::{OpenSandboxPrereqs, SandboxPrereqs}`, added `CommandKind` to the prereqs import, kept the modal `flex_1`/`min_w_0`/`w_full` wrap fixes and the Run-in-Terminal IconButton + `open_in_terminal`/`spawn_linux_terminal` helpers verbatim. `Cargo.lock` hand-resolved (auto-merge had misaligned the package blocks for `_state` and `_ui`); validated by re-running `cargo check`.
- RECAPS ordering: modal entry stays at the top of `## 2026-05-14` (newest-first), enforcement entry slots in just below.
- Verified post-rebase: `cargo check -p paddleboard_sandbox_prereqs_ui` clean; full `cargo check -p paddleboard` clean (~19s); 3/3 prereqs unit tests still pass. Force-pushed with `--force-with-lease`; `gh pr view 26` now reports `mergeable: MERGEABLE, mergeStateStatus: CLEAN`.
- Preserved: the rebased commit is still a single commit (`ab8c40529a` → `120ace08bb`) sitting one ahead of `main`. Did not squash-into or split. The PR body / title are unchanged.

### Sandbox prereqs modal: text wrap, prescriptive copy, Run-in-Terminal (macOS + Linux)
- Three user-facing fixes to the Sandbox Prerequisites modal (the install-instructions dialog that opens from the shield status-bar item): step descriptions weren't wrapping inside the 36-rem dialog, the copy was too terse to actually be prescriptive, and there was no affordance to actually run the commands without alt-tabbing.
- Wrap fix: descriptions were clipping mid-word at the modal's right edge (visible in `paddleboard-4.png`). Root cause was a flex-sizing gotcha — `flex: 1 1 0` alone doesn't let a child shrink below its content's intrinsic min-width because of CSS's default `min-width: auto`. Added `.min_w_0()` to the description's `flex_1` div (the command block already had it) and `.w_full()` at each level (per-step `v_flex`, description `h_flex`, command-row `h_flex`) as belt-and-braces so the flex chain has definite cross-axis sizes top to bottom. Verified in `crates/ui/src/components/label/label_like.rs:243-249` that `Label` defaults to wrap; only `single_line` or truncate modes set `whitespace_nowrap`.
- Prescriptive copy: rewrote every (PodmanStatus, Os) branch in `install_instructions`. Each branch now leads with a context `note` (what Podman/gVisor is, why it's needed), follows with concrete commands, and ends with an explicit "Click Refresh below to verify" step. macOS path gained an upfront Homebrew prereq note; Linux paths gained a `podman info` / `systemctl --user enable --now podman.socket` verify step.
- `paddleboard_sandbox_prereqs`: added `CommandKind { Shell, Snippet }` + a `command_kind` field on `InstallStep`, plus three constructor helpers (`InstallStep::note/shell/snippet`). Snippet kind is for TOML/config fragments (like `[engine.runtimes]\nrunsc = ["/usr/local/bin/runsc"]`) that the user pastes into a config file — these suppress the Run-in-Terminal button so the user doesn't try to execute config text as a shell command. The macOS-gVisor branch now marks inner-VM commands as `Snippet` since they run inside the SSH session, not on the host Mac.
- Run-in-Terminal (macOS): new `IconButton::new(..., IconName::Terminal)` next to Copy, gated on `command_kind == Shell && matches!(host_os, Os::MacOs | Os::Linux)`. Click calls `write_wrapper_script(command)` which writes a unique-named `paddleboard-sandbox-<pid>-<nanos>.sh` to the temp dir (chmod 0755). The wrapper runs the command, prints the exit code, waits on Enter, then `rm -- "$0"` self-removes. macOS arm shells out to `open -a Terminal <path>`. After spawn, the click handler also triggers `SandboxPrereqs::refresh(cx)` so the modal re-probes when the user returns.
- Run-in-Terminal (Linux): `spawn_linux_terminal(&path)` walks a priority list of well-known terminal emulators and uses the first one found on `$PATH` via the `which` crate (added as a Linux-only `[target.'cfg(target_os = "linux")'.dependencies]` entry). Each candidate carries its own argv-prefix because invocation conventions vary — `xdg-terminal-exec` (XDG spec, first), `x-terminal-emulator -e` (Debian alternatives), `gnome-terminal --`, `konsole -e`, `xfce4-terminal -e`, `tilix -e`, `terminator -e`, `alacritty -e`, `kitty <script>` (positional), `foot <script>`, `wezterm start --`, `ghostty -e`, `xterm -e`. Falls back to `io::ErrorKind::NotFound` with a search-set message if nothing matches; the click handler logs the error via `log::warn!` and the Copy button is always available as a fallback.
- Preserved: macOS `open -a Terminal` behaviour. The Snippet/Shell distinction means TOML-fragment steps only render Copy on both platforms. CLI `--check-sandbox` output is unchanged — `paddleboard/src/main.rs` destructure now uses `..` to ignore the new `command_kind` field rather than differentiating Shell vs Snippet in the plain-text path (the description text already carries the distinction in prose).
- Verified: `cargo check -p paddleboard` clean (~30s); `cargo check -p paddleboard_sandbox_prereqs_ui` clean; 3/3 prereqs unit tests still pass; full debug build + launch of `cargo run -p paddleboard` exercised the macOS Run-in-Terminal path against the gVisor-not-configured branch.
- Not verified locally: the Linux arm is `#[cfg(target_os = "linux")]` so it's dropped from the AST on a macOS build. `cargo check --target x86_64-unknown-linux-musl` fails on missing `x86_64-linux-musl-gcc` (cc-rs toolchain not on this Mac). Linux CI is the type-check authority for this arm.
- Follow-ups: no UI feedback on Linux spawn failure (silent `log::warn!`) — a future pass could surface a toast via the workspace notification system. `$TERMINAL` env-var override isn't honoured, but `xdg-terminal-exec` is preferred in the candidate list so users who configure their desktop's default terminal get it for free. Snippet-kind steps could grow a "Paste into config file" affordance (open the path in the editor, append the snippet) — out of scope for this PR.

### Sandbox prerequisites enforcement (PR-C: gate three tool sites)
- Closed the loop on the sandbox-prereqs feature: PR-A (data) and PR-B (UI) were visibility only; this is the enforcement half that actually refuses to run podman when prereqs are missing.
- Two new crates: `paddleboard_sandbox_prereqs_state` extracts the `SandboxPrereqs` GPUI global out of the UI crate so non-UI callers (`agent`, `project`) can read the cached probe status without a `workspace` cycle. `paddleboard_sandbox_settings` owns the policy model: `OnMissingRuntime` enum (Block / FallBackToHost / WarnOnce), `SandboxSettings` struct (with `RegisterSetting` derive), a pure `decide_gate(prereqs, settings) -> SandboxGateDecision`, and a `claim_warn_once_slot()` AtomicBool so `warn_once` is genuinely once-per-session. 7 unit tests covering the decision matrix.
- Settings wiring: new tagged file `crates/settings_content/src/paddleboard_sandbox.rs` defines `PaddleboardSandboxContent` + `PaddleboardOnMissingRuntimeContent`; three tagged additions to `settings_content.rs` add the module + re-export + field. Defaults wired into `assets/settings/default.json` with a `// PaddleBoard:` comment block. Settings registration triggered via a force-link `init(cx)` from `paddleboard/src/main.rs`.
- Three gate sites:
  - `crates/agent/src/tools/sandbox_tool.rs` — gate in `cx.update` block; `Block` returns clear error pointing at the status bar; `FallBackToHost` strips the podman wrapper and runs `bash -c <user_command>` in the working directory; `WarnOnce` logs once + proceeds sandboxed.
  - `crates/agent/src/tools/sandbox_service_tool.rs` — same gate; for `FallBackToHost` the service spawns on host with `host_port == container_port`, still registers a Forwarded Ports entry so the user gets a clickable link.
  - `crates/project/src/context_server_store.rs` — gated at the `ContextServerConfiguration::Sandboxed` arm (caller of `ContextServer::sandboxed_stdio`), keeping the upstream-shaped `sandboxed_stdio_transport.rs` untouched per fork hygiene. `FallBackToHost` falls through to plain `ContextServer::stdio`.
- Fork-hygiene cost: 1 field in `SettingsContent`, 1 mod decl + 1 use re-export in `settings_content.rs`, 1 field in `VsCodeImporter::settings_content()`, all tagged `// PaddleBoard:`. Everything else lives in PaddleBoard crates.
- Verified: full `cargo build -p paddleboard` clean; `cargo clippy --no-deps` over the changed crates clean; all unit tests pass (7 in settings crate, 3 in sandbox_tool, 7 in sandbox_service_tool, 6 in sandboxed_stdio_transport).
- Docs: `WELCOME.md` Sandbox section expanded with the policy values and the shield-icon UX. `crates/workspace/src/tour.md` section 2 mirrors that. `README.md`'s "Secure agent sandbox" bullet under `## What's different from Zed` got the same shield-icon + `on_missing_runtime` note tacked on. Existing users won't see the tour update until the materialization gate at `workspace.rs:785` / `paddleboard/src/main.rs:1494` is loosened — flagged for a future session, not in this PR.
- Workflow rule extended: the WELCOME/tour feedback memory now also covers `README.md`. User asked for README to be kept in sync on the same turn as WELCOME + tour after I'd updated the latter two but left README stale on this PR. See `feedback-update-welcome-and-tour`.
- Follow-ups: nothing committed yet on `feat/sandbox-prereqs-enforcement` — the user will decide on commit + PR after review. The duplicate "Sandboxed MCP Servers" block at the bottom of `tour.md` (a pre-existing bug) was intentionally left alone to keep this PR scoped.

### Sandbox prerequisites UI (PR-B: visibility unit)
- New crate `paddleboard_sandbox_prereqs_ui` (~350 lines) layered on PR-A's data layer. Three pieces in one file: `SandboxPrereqs` (a `gpui::Global` holding the latest `SandboxStatus` + a `refreshing` flag), `SandboxStatusItem` (status-bar entry — colored shield icon), `SandboxPrereqsModal` (full install-guidance UI with per-step Copy buttons + Refresh).
- Async-to-GPUI bridge uses `gpui_tokio::Tokio::spawn(cx, async { check().await })`. The probe runs on tokio's pool (needed for `tokio::process::Command`); the result is written back to the global via `cx.update_global` on the foreground thread, which automatically notifies observers so the status bar + modal re-render.
- Severity model: `Unknown` while initial probe is in flight, `Ok` when podman + gVisor are both satisfied (or gVisor inapplicable on Windows), `Warning` when podman is ready but gVisor isn't configured, `Error` when podman is missing or unreachable.
- Wire-up: `paddleboard_sandbox_prereqs_ui::init(cx)` in `paddleboard/src/main.rs` right after `gpui_tokio::init` — registers the global, kicks off the first probe, and `cx.observe_new` wires the `paddleboard::OpenSandboxPrereqs` action into every workspace as it opens. Status item is registered in `paddleboard/src/zed.rs`'s `initialize_workspace`, tagged with two `// PaddleBoard:` markers (declaration site + status_bar.add_right_item).
- v0.1 followup (PR-C, future session): tool gating + settings (`sandbox.on_missing_runtime`: block / fall-back-to-host / warn-once) — the enforcement unit, requires editing three tool entry points (sandbox_tool, sandbox_service_tool, sandboxed_stdio_transport).
- Verified: `cargo check -p paddleboard` clean; `./script/clippy -p paddleboard_sandbox_prereqs_ui -p paddleboard_sandbox_prereqs` clean. The pre-existing `llm_picker` clippy failure on `main` is unrelated.

### Sandbox prerequisites detection (PR-A of two)
- New crate `paddleboard_sandbox_prereqs` with `check() -> SandboxStatus`. Probes `podman --version`, `podman info --format json`, and parses the JSON for `host.ociRuntimes.runsc`. Each probe is timeout-bounded (2s) so a stuck `podman machine` cannot stall startup.
- Three variants per dimension: `PodmanStatus` (Missing / InstalledNotRunning / Ready) and `GvisorStatus` (Available / NotConfigured / NotApplicable / Unknown). On macOS the InstalledNotRunning state is the common case when `podman machine` is stopped.
- Hand-curated `install_instructions(status, os)` produces ordered `InstallStep`s with copy-paste commands per OS: brew on macOS, distro-detected (`/etc/os-release` ID lookup) on Linux, Podman Desktop pointer on Windows. gVisor section only appears once Podman is `Ready` so missing-Podman users don't see runtime instructions before they have a runtime.
- CLI: `--check-sandbox` flag on the `paddleboard` binary. Spins up a current-thread tokio runtime, runs the check, prints a status block + install steps, exits 0 if satisfied / 1 if not. Useful for OSS users diagnosing in a terminal before launching the editor.
- Locally verified end-to-end: `./target/debug/paddleboard --check-sandbox` correctly reports `Podman ✓` + `gVisor ✗`, prints macOS `podman machine ssh` runsc-install path, exits 1. 3/3 unit tests pass; clippy clean.
- v0.1 followup (PR-B, separate session): background check on startup + cached `Entity<SandboxPrereqs>` + status indicator UI + modal + tool gating + settings. Detection logic is in place; UI surfaces aren't.

### Fixed shell-interpolation injection pattern in merge_upstream_zed.yml
- `cargo xtask check-workflows` (exposed after PR #21) flagged 8 instances of `${{ steps.*.outputs.* }}` being interpolated directly into shell `run:` blocks across 5 steps. That's GitHub's documented script-injection pattern — values from `${{ }}` expressions are substituted *before* the shell parses the script, so a value containing shell metacharacters would be executed.
- In this specific workflow the values come from `git rev-parse` / `git merge-base` / `date -u` and are safe in practice, but the validator is right that the pattern is dangerous and worth fixing on its own merits.
- Moved each output into a step-level `env:` block, switched `run:` blocks to read `$VAR` instead of `${{ }}`. `if:` conditions on step outputs stay as-is (those are GitHub Actions expressions evaluated before any shell runs — safe and correct).
- Validator now exits 0 against the workflow.

### Dropped xtask workflow-generator subsystem
- The xtask `Workflows` subcommand generated 18 different `.github/workflows/*.yml` files from `gh-workflow`-based generators in `tooling/xtask/src/tasks/workflows/`. **Every** target workflow has now been deleted across PRs #13/#14/#15/#16, so the entire subsystem was producing nothing but zombies waiting to be resurrected by `cargo xtask workflows`.
- Removed: the `Workflows` CLI subcommand, `tasks/workflows.rs` dispatcher, 26 generator files under `tasks/workflows/`, and the workspace `gh-workflow` git dependency (used only here).
- Kept: the `CheckWorkflows` subcommand and `workflow_checks.rs` validator. Refactored it to drop the `WorkflowType` enum dependency and iterate `.github/workflows/` directly — we don't have Zed's `extensions/workflows/` folders.
- Sanity-checked: `./script/clippy -p xtask` passes; `cargo xtask check-workflows` runs the validator successfully against `merge_upstream_zed.yml`. The validator surfaces a real pre-existing issue — direct `${{ steps.* }}` interpolation in shell `run:` blocks should be passed via env vars instead. Not a regression from this PR; flagged as a followup.

### CONTRIBUTING.md rewritten, FORK_HYGIENE.md workflows section updated
- `CONTRIBUTING.md` was still Zed's 156-line version (Zed CLA, Zed staff confirmation gates, Zed forums, Let's Git Together community program, packaging-Zed link, bird's-eye view of Zed crates). Replaced with a ~35-line PaddleBoard version: pointers to CLAUDE.md / FORK_HYGIENE.md / WELCOME.md, explicit "this belongs upstream → file against zed-industries/zed" rule, PR title/release-notes conventions, and an explicit "upstream surface area → see Zed's docs" note that defers UI/UX standards to upstream.
- `FORK_HYGIENE.md`'s "Inherited Zed workflows" section said "~40 workflows … prune opportunistically" — outdated. Rewrote to reflect the new state: exactly one workflow remains (`merge_upstream_zed.yml`), cleanup happened across PRs #13/#14/#15/#16, three of the removed files are still generated by `cargo xtask workflows` so the followup is xtask, and from now on the rule is "delete on the merge branch before marking the upstream PR ready."

### Rewrote README.md for PaddleBoard
- The README was still Zed's — broken CI badge (pointing at the deleted `run_tests.yml`), Zed Industries sponsorship section, package-manager install links to a release pipeline we don't have, "we're hiring" link to Zed jobs.
- New README leads with the PaddleBoard pitch (AI-driven dev environment forked from Zed), lists the seven feature differentiators (sandbox, forwarded ports, browser panel, sandboxed MCP, step-through, orchestration panel, LLM picker) sourced from WELCOME.md, points to FORK_HYGIENE.md for the fork model and CLAUDE.md for agent contributor rules.
- Build section points at the still-valid upstream build docs (`docs/src/development/*.md`) with the `cargo run -p paddleboard` substitution. License section calls out the inherited tri-license.
- Followups noted: `FORK_HYGIENE.md` says "~40 inherited workflows" but only `merge_upstream_zed.yml` remains after today's cleanup. `CONTRIBUTING.md` is still Zed-flavored (mentions the Zed CLA, etc.) — needs a separate pass.

### Pruned inherited Zed workflows
- Removed 7 of the 8 remaining `.github/workflows/` files. Only `merge_upstream_zed.yml` (PaddleBoard's weekly upstream merge) stays.
- Three were already hard-gated `if: github.repository_owner == 'zed-industries'` and would silently no-op forever in this fork: `pr_labeler.yml`, `randomized_tests.yml`, `stale-pr-reminder.yml`.
- Four depended on zed-org secrets/infra that we don't (and shouldn't) have: `autofix_pr.yml` (ZED_ZIPPY app + namespace.so runners), `run_bundling.yml` (Apple notarization + Azure signing + ZED_CLIENT_CHECKSUM_SEED), `docs_suggestions.yml` (Factory.ai FACTORY_API_KEY — Factory is a separate AI-tools company we don't use), `compare_perf.yml` (namespace.so 16x32 perf runners — perf testing on shared `ubuntu-latest` would be too noisy to be useful).
- Three of these (`autofix_pr`, `compare_perf`, `run_bundling`) are generated by `cargo xtask workflows`. If someone runs that command they'll come back — followup pass needed in the xtask to drop the generators.

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
