# MODEL TIERS — run routine work on a local model, escalate on measured failure

> **Status:** phase 1 built 2026-10-08 (see [As built](#as-built-2026-10-08)); designed 2026-10-07. Jay's call the same day: this is a differentiator
> and ships **in the v0.3.0 beta**, even if the launch moves a week to get it right.
> **Ships as Spotter** (named 2026-10-12): the settings key is `"paddleboard_spotter"`
> and user-facing text says Spotter. "Model tiers" stays as the internal name of the
> crates, modules and this doc.

## The gap

The pitch is simple to say and nobody ships it well: **let a free local model do
the routine work, and bring in Claude or Gemini only when the work actually needs
them.** Most of a session is single-file edits, docstrings and "make the test
pass". Paying frontier prices for all of it is waste. Running all of it on a 12B
local model fails on the hard 20%, and today the user only finds out by watching.

PaddleBoard already holds every model a user could want in one registry. What it
lacks is the loop around them: nothing notices that the local model is failing,
and nothing moves the task up to a stronger one.

## What already exists

A read-only audit of `main` (2026-10-07, every claim re-checked) found that most
of the plumbing is shipped. Nothing below needs a new provider adapter.

| Need | Status | Where |
|---|---|---|
| Ollama, llama.cpp, LM Studio, OpenAI-compatible (vLLM), Gemini and Anthropic configured together | ✅ | one registry, `crates/language_models/src/language_models.rs`; settings in `assets/settings/default.json` `language_models` |
| A different model per feature | ✅ | `agent.default_model`, `subagent_model`, `inline_assistant_model`, `commit_message_model`, `thread_summary_model`, `compaction_model` (`crates/settings_content/src/agent.rs`) |
| Inline completions on a local model | ✅ | `edit_predictions.provider: "ollama"` (`EditPredictionProvider::Ollama`) |
| Switch model by hand, with shortcuts | ✅ | `agent::ToggleModelSelector` (`cmd-alt-/`), `agent::CycleFavoriteModels` (`alt-tab`), `agent::ToggleProfileSelector` (`cmd-i`) |
| A profile that pins a model and a tool set | ✅ | `AgentProfileContent.default_model`, applied by `Thread::set_profile` |
| Delegate a subtask to another model | ✅ | `spawn_agent` tool takes `model` and `persona` (`crates/agent/src/tools/spawn_agent_tool.rs`) |
| `PERSONA.md` in the system prompt | ✅ | `crates/paddleboard_personas` (root `PERSONA.md`, `.claude/personas/*.persona.md`) |
| Anthropic prompt caching | ✅ | `AnthropicPromptCacheMode::Automatic` by default (`crates/anthropic/src/completion.rs`) |
| Per-model token usage on disk | ✅ | `crates/paddleboard_usage` (daily JSON, includes cache read/write tokens) |
| **An automatic model switch, in-thread** | ⚠️ one narrow case | the refusal fallback in `Thread::run_turn_internal` (`thread.rs`, `had_refusal` branch): finds a model in the registry, calls `set_model`, sends a retry notice, continues the turn |
| **A signal that the work failed** | ❌ | tests run only if the model chooses to call the terminal; no fix-attempt counter, no stuck-loop detection |
| **Escalation on failure** | ❌ | nothing; the refusal fallback's target is a hardcoded id on Fable models only |
| Prices, so cost can be shown or budgeted | ❌ | tokens only |
| Gemini context caching | ❌ | reads `cached_content_token_count`, never creates a cache |

The refusal fallback matters most here. It proves the in-turn switch works: same
thread, same history, new model, with a notice the UI already renders
(`acp_thread::meta_with_refusal_fallback`, shown in `thread_view.rs`). Escalation
is that mechanism with a different trigger and a user-chosen target.

## Principles

1. **Escalate on evidence, not on a guess.** A model that classifies "is this
   task hard?" is itself a model call, and when it misroutes nobody sees it. A
   failed check is a fact. Tiers are chosen by the user; movement between them is
   triggered by a check failing.
2. **The user stays in charge of spend.** The default is *ask before escalating*.
   Automatic escalation is a setting, off by default, because it moves work from
   free to paid.
3. **No new provider code.** Tiers are named lists of models the registry already
   knows. Every provider that works today works in a tier.
4. **Fork-owned crate, tiny hooks.** The logic lives in new `paddleboard_tiers*`
   crates. Upstream-shaped files (`thread.rs`, `thread_view.rs`) get small,
   tagged call sites, so the weekly merge stays mechanical.
5. **Small models get small jobs.** A 12B local model will skip steps if it is
   handed thirty tools and room to improvise (the same lesson the ADK work hit with
   Gemma on Mementos). The local tier pairs with a narrow profile.
6. **Nothing leaves the machine that didn't before.** Escalating to a paid
   provider is the one moment code goes off-device, so it is always visible and,
   by default, always confirmed.

## The design

### 1. Tiers (settings)

A new top-level setting, `paddleboard_tiers`, in user or project settings:

```jsonc
"paddleboard_spotter": {
  "enabled": true,
  // Ordered, cheapest first. Each tier is a model the registry already has,
  // optionally paired with an agent profile (tools and instructions).
  "tiers": [
    { "name": "Local",    "model": { "provider": "ollama",    "model": "gemma3:12b" },        "profile": "local-narrow" },
    { "name": "Fast",     "model": { "provider": "google",    "model": "gemini-3.5-flash" } },
    { "name": "Precise",  "model": { "provider": "anthropic", "model": "claude-sonnet-5" } },
    { "name": "Deep",     "model": { "provider": "anthropic", "model": "claude-fable-5-1" } }
  ],
  "start_tier": "Local",
  // The failure signal. Runs in the agent sandbox at the end of every turn.
  "check": { "command": "cargo test -p my_crate", "timeout_seconds": 300 },
  // Fix attempts on the current tier before stopping. 1 = the "two-strike" rule.
  "retries_per_tier": 1,
  // "ask" (default): stop and offer the next tier. "auto": move up without asking,
  // up to "auto_ceiling". "off": stop and report, never offer.
  "escalation": "ask",
  "auto_ceiling": "Fast"
}
```

`model` reuses `LanguageModelSelection` (provider, model, thinking, effort), so a
tier can be "Sonnet with extended thinking" as easily as "Sonnet". `check` is a
project setting by nature; a project's `.paddleboard/settings.json` (or the future
`harness.toml`, see [Open questions](#open-questions)) supplies it.

The four tiers in the example are the brief's own. Nothing requires four: two
tiers (local, then Sonnet) is the expected common case.

### 2. The check (the missing failure signal)

After a turn ends with `StopReason::EndTurn` the thread runs `check.command`,
whether or not the turn edited a file (see [As built](#as-built-2026-10-08)):

- **Same sandbox as the terminal tool.** The command runs under the thread's
  `ThreadSandbox` and the persistent sandbox policy (`crates/agent/src/sandboxing.rs`),
  with the worktree writable, so a check can never do what the agent could not.
  If the sandbox is unavailable and the policy is `block`, the check is reported
  as *unavailable*, not as passed.
- **Visible.** It renders as a tool-call card in the thread ("Check: `cargo test
  -p my_crate`"), with exit code and the last 200 lines, exactly like a terminal
  tool call. The user can always see what decided a pass or a fail.
- **Pass** ends the run. **Fail** feeds the next step.
- **No edits, no check.** A turn that only answered a question costs nothing.

### 3. Two-strike escalation (the state machine)

Per thread, `paddleboard_tiers` keeps `{ tier, attempt }`:

```
            turn edits files
                  │
                  ▼
             run check ──pass──▶ DONE
                  │fail
                  ▼
      attempt < retries_per_tier ──yes──▶ attempt += 1
                  │no                      send check output as the next user
                  ▼                        message ("The check failed. Fix it.")
            BLOCKED on this tier           and continue on the same model
                  │
     ┌────────────┼──────────────────┐
  "ask"         "auto" (≤ ceiling)   "off" / top tier
     │            │                   │
 Blocker card   escalate              Blocker card,
 [Escalate to   (tier += 1,           no button
  <next>]       attempt = 0)
 [Stop]           │
     │            ▼
     └──────▶ set_model(next tier), send the Blocker Summary as the next
              user message, continue the turn
```

- **The Blocker Summary is deterministic, not generated.** The check command,
  exit code, the last 60 lines of output, the files changed this run, the tiers
  tried and attempts on each. Asking the failing model to summarise its own
  failure costs a call and invites the same mistake; the facts are enough, and
  they are what the stronger model needs.
- **Escalation reuses the refusal-fallback path:** `set_model`, a retry notice
  in the existing UI slot ("Escalated to Precise after 2 failed checks"), and
  `continue` inside `run_turn_internal`. The hook is one call into
  `paddleboard_tiers` at the point the turn would otherwise stop.
- **Manual moves still work.** Picking a model by hand in the selector resets
  `attempt` and pins that model until the user changes it again; tiers never fight
  the user.
- **One thread, one history.** Escalation does not spawn a subagent. The stronger
  model sees everything the weaker one tried, which is the point.

### 4. Seeing where the work went

- The thread header shows the current tier next to the model name ("Local ·
  gemma3:12b").
- `paddleboard_usage` already records tokens per provider and model. It gains a
  `tier` field per record, so a day's file answers "how much ran local?".
- The status-bar context gauge is unchanged.

### 5. Personas and subagents (phase 2)

Persona frontmatter gains an optional `tier:` (a tier *name*, never a raw model
id, so a persona stays portable between machines). `spawn_agent` with a persona
that names a tier starts the subagent on that tier. This reverses one line of
`AGENT_SCAFFOLDING.md` ("Model, keys and sandbox settings are not fields"), which
is why it is phase 2 and an open question, not phase 1.

### 6. Cost (phase 2)

An optional `price` per tier (USD per million input, output, cache-read tokens),
user-supplied, never fetched. `paddleboard_usage` multiplies; the thread header
shows the run's cost; `harness.toml`'s planned budget gets something to compare
against. Prices are user-entered because they change, and because a wrong
built-in price is worse than none.

## What it looks like

1. Jay asks the agent, on the Local tier, to add a field to a struct and update
   the three call sites.
2. Gemma edits two of the three call sites. The check runs: `cargo test` fails to
   compile at the third.
3. Strike one. The compiler output goes back to Gemma as the next message. It
   edits the wrong file. The check fails again.
4. Strike two. A Blocker card: *Check failed twice on Local (gemma3:12b). 2 files
   changed. `error[E0063]: missing field` at `src/zed/foo.rs:154`.* Buttons:
   **Escalate to Fast**, **Escalate to Precise**, **Stop**.
5. Jay picks Precise. Sonnet reads the history and the summary, fixes the third
   call site, the check passes. The usage file for the day shows 41k tokens on
   Local, 9k on Precise.

## Phases

### Phase 1 — the beta (in v0.3.0)

1. `paddleboard_tiers_settings`: the settings above, with schema and defaults
   (`enabled: false`, so nothing changes for anyone who doesn't opt in).
2. `paddleboard_tiers`: the check runner (via the thread sandbox) and the
   two-strike state machine as plain data, unit-tested without GPUI.
3. Hooks: one call at turn end in `thread.rs`, one card variant in
   `thread_view.rs` (Blocker card with escalate buttons), the tier label in the
   thread header. All tagged `// PaddleBoard:`.
4. `paddleboard_usage`: `tier` on each record.
5. Docs page (`docs.paddleboard.dev/model-tiers.html`), a `/beta` entry, a README
   line, a WELCOME section, and a starter `local-narrow` profile.
6. Tests: state-machine transitions (pass, fail→retry, fail→blocked, ask, auto
   up to ceiling, top tier, manual override resets), settings parsing, Blocker
   Summary formatting. One end-to-end test with a fake model and a check command
   that fails then passes.

#### As built (2026-10-08)

Phase 1 shipped with these deliberate differences from the design above:

- **No `start_tier`.** The thread's current model decides which tier it is on, so
  the user starts on the local tier with the normal model picker or default model.
  A setting that silently replaced the chosen model on every new thread would fight
  the user. A model that is not a tier is offered every tier.
- **The check is the agent's own terminal tool**, called by the thread with a
  synthetic tool-call card (`sandboxed_terminal` when the sandbox is on, else
  `terminal`). That gives it the exact sandbox policy, permission prompt and
  rendering of any agent command for free. Its result goes to the model as a plain
  user message, not a tool result, because a tool result without a matching model
  tool call is rejected by providers.
- **The check runs after every top-level turn, edits or not** (changed 2026-10-08,
  after the first real-model run). The design said "after a turn that edited a
  file". On a real run `gemma4:e4b` described an implementation, called no tool
  and ended its turn twice; with an edit trigger the check never fired and the run
  ended "completed" with every test failing. A model that claims work it did not do
  is exactly the failure tiers exist to catch. The cost is a check after a pure
  question turn too, which is why the check should be fast. Subagent threads still
  skip it; their parent checks.
- **Pass/fail is read from the terminal tool's text**, keyed on the exact command
  and matched after any sandbox notes. A test runs the real formatter so a wording
  change upstream fails the build instead of turning failures into passes.
- **Deferred:** the tier label in the thread header (the model picker already shows
  the model) and the `tier` field in usage records (records are already per model,
  which identifies the tier). A starter `local-narrow` profile is documented rather
  than shipped as a default, since a new default profile would change every user's
  profile list.
- **The `paddleboard_tiers` key lives in the project settings struct**, which is
  flattened into user settings, so one key serves both scopes and merges field by
  field (tested).

### Phase 2 — after launch

Persona `tier:`, prices and the run cost, Gemini context caching, subagent tier
inheritance, `harness.toml` as the project home for `check` and the budget.

### Not planned

- **Automatic task-complexity routing.** Revisit only with data from phase 1's
  usage records showing which tasks escalate.
- **Racing models in parallel** on the same task. Doubles spend for a benefit the
  check already provides more cheaply.

## Schedule

Phase 1 is roughly **five working days**: two for the crates and state machine
with tests, one for the thread and UI hooks, one for a full CI pass plus a real
run on macOS against Ollama and Anthropic, one for docs, `/beta` and the launch
copy facts. The hooks touch `thread.rs`, the most upstream-active file in the
agent, so it gets a careful review rather than a fast one.

Against the current dates (cut Tue 13 Oct, launch Wed 14), that does not fit with
any margin. **Recommendation: move the launch one week, to Wednesday 21 October,
cut Tuesday 20.** The upstream merge freeze extends with it; the scheduled close of
Monday's merge-bot PR still applies, and the 19 October merge-bot PR is closed the
same way. Moving the date touches `hugo.toml` `[params.beta]`, the launch plan, the
release notes and Jay's launch copy doc; the Reddit days move to Thu 22, Fri 23,
Mon 26.

## Open questions

1. ~~**The name.**~~ **Decided 2026-10-12: Spotter** (the gym spotter steps in only
   when the rep fails). Chosen over Tow-In and Tag In; Lifeguard read as a
   security feature. The settings key was renamed before the v0.3.0 cut, while
   no release had shipped it; crate and module names stay `paddleboard_tiers`.
2. **Default `escalation`.** `ask` is proposed. `auto` with a ceiling of a cheap
   tier is the more impressive demo; `ask` is the one that never surprises a bill.
3. **Where `check` lives long-term.** Project settings now; `harness.toml` once
   Agent Scaffolding ships it. Both should keep working.
4. **Persona `tier:`** reverses a recorded scaffolding decision. Phase 2 either
   way; yes or no decides whether the persona format changes.
5. ~~**Does the check run after every edited turn, or only when the agent says it
   is done?**~~ **Decided 2026-10-08: after every turn**, edited or not. A real
   `gemma4:e4b` run claimed edits it never made; see [As built](#as-built-2026-10-08).

## Reference

- Refusal fallback (the in-turn switch this reuses): `crates/agent/src/thread.rs`,
  `run_turn_internal`, `had_refusal` branch; UI notice
  `acp_thread::meta_with_refusal_fallback`, rendered in
  `crates/agent_ui/src/conversation_view/thread_view.rs`.
- Model selection type: `LanguageModelSelection`,
  `crates/settings_content/src/agent.rs`.
- Sandbox for the check: `crates/agent/src/sandboxing.rs`.
- Usage records: `crates/paddleboard_usage/src/paddleboard_usage.rs`.
- Personas: `crates/paddleboard_personas/src/paddleboard_personas.rs`.
- Related design: [`AGENT_SCAFFOLDING.md`](./AGENT_SCAFFOLDING.md) (`harness.toml`,
  persona conventions).
