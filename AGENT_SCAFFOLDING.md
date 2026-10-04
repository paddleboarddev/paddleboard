# AGENT SCAFFOLDING — define, run, and deploy an agent from one brief

Design doc for making agent scaffolding a headline PaddleBoard feature: a single
brief that produces a runnable agent *and* the Markdown that defines it
(`AGENTS.md`, `PERSONA.md`, `SKILL.md`), an A2A agent card, an eval loop, and an
optional deploy hand-off. Produced 2026-10-04 from a read of the shipped
`paddleboard_agent_frameworks`, `paddleboard_personas`, `paddleboard_set_sail`
and `paddleboard_ai_dock` crates, at Jay's request.

**Scheduled after the v0.3.0 beta**, as the v0.4 headline. Not a launch item —
see [Why this waits](#why-this-waits).

---

## The gap

PaddleBoard already manages every part of an agent and already deploys one.
Defining one is a blank directory.

| Stage | What PaddleBoard has today | Where |
|---|---|---|
| **Define** | Personas (`PERSONA.md`, a library with `extends:`), skills in `.agents/skills/<name>/SKILL.md`, MCP servers, the Build-an-MCP modal | AI Dock |
| **Scaffold** | `adk: Scaffold Agent`, `langgraph: Scaffold Agent`, `crewai: Scaffold Agent` — a name field that runs the framework's own `create` command in a terminal | `scaffold_modal.rs` |
| **Run** | Per-framework `Run Agent` actions, port forwarding, the sandbox service tool, the Agent Threads tree, the usage tab | `paddleboard_agent_frameworks`, AI Dock |
| **Deploy** | Set Sail: quick deploy or "rig the pipeline" to six platforms, driven by s8sskills packs | `paddleboard_set_sail` |

The scaffold step is the thin one. It writes the framework's code skeleton and
nothing else: no `AGENTS.md`, no persona, no skills, no agent card, no evals,
no link to deploy. Every convention that the agentic-design, loop-engineering
and harness-engineering practice now treats as standard — *write the operating
instructions, write who the agent is, write what it can do, give it a loop to
improve against, make it addressable* — is left to the user to assemble by
hand from pieces that live three tabs apart.

That is the feature: close define → run → deploy into one surface, with the
Markdown layer as the thing no framework CLI gives you.

## What exists to build on

Nothing here starts from zero. The modal pattern, the hand-off to the agent,
the catalog mechanism and every file convention are already shipped.

- **The modal → agent hand-off is Build MCP's.** `build_mcp_modal.rs` collects
  a short brief, builds a self-contained prompt, and seeds a new thread with it
  (`seed_prompt_thread`). The agent does the work; the modal does not. The
  scaffolder is the same shape with a larger brief.
- **The catalog mechanism is Set Sail's.** `ensure_skill_installed` fetches
  `https://raw.githubusercontent.com/s8sskills/<repo>/main/skills/<skill>/SKILL.md`
  into `.agents/skills/<skill>/`, and the prompt tells the agent to follow it.
  Platform knowledge lives in a versioned catalog, not in Rust. The scaffolder
  should use the identical path with its own packs.
- **Persona conventions are fixed.** `PERSONA.md` at the project root is the
  zero-config entry; library personas are `.claude/personas/*.persona.md`
  (project) or `~/.claude/personas/` (user); frontmatter carries `name`,
  `description`, `type`, `voice` and `extends`. Three starter roles ship in
  `assets/ai_dock/personas/`.
- **Framework detection and run already exist** for ADK, LangGraph, CrewAI,
  AutoGen and A2A, including the A2A agent-card landing path
  (`/.well-known/agent-card.json`) that `a2a: Run Agent` opens.
- **The existing `ScaffoldConfig` table** (`adk create`, `langgraph new`,
  `crewai create crew`) stays as the code-skeleton step inside the new flow.
  It is not replaced; it is step one of several.

## Principles

1. **One brief, not a form.** Name, purpose in a paragraph, and a handful of
   pickers. Every field maps to a file the user would otherwise write by hand.
   If a field does not produce a file, it does not belong in the modal.
2. **The agent writes; the editor orchestrates.** Same division as Build MCP
   and Set Sail. The modal installs the pack, seeds the thread, and gets out of
   the way. Generated content is never hardcoded in Rust.
3. **Conventions live in a catalog.** `AGENTS.md`, `SKILL.md` and agent-card
   conventions are still moving. A catalog tracks them without a PaddleBoard
   release; a template baked into the binary is stale on the day it ships.
   Same argument that put Set Sail on s8sskills.
4. **ADK by default, A2A always.** A scaffolded agent gets an A2A endpoint and
   agent card whatever the framework, so it is addressable from its first run.
   Two agents that could be two services are two services; a "team" brief
   produces N agents talking A2A, never one process with roles inside it.
5. **The loop is part of the definition.** An agent without evals is a prompt.
   The scaffold ships `evals/` with starter cases and a run → judge → record
   task the in-editor agent can iterate against in the sandbox. This is the
   part no framework CLI provides and the part that makes the feature a
   harness rather than a file generator.
6. **Nothing phones home.** The catalog is fetched the way s8sskills already is
   (raw GitHub, user-initiated, cached in the project). No registry account, no
   telemetry about what was scaffolded.

## The brief

One modal, reached from the AI Dock's Agents tab (**New Agent…**) and the
command palette (`agent: Scaffold`). The existing per-framework
`<fw>: Scaffold Agent` actions open the same modal with the framework
preselected, so nothing a user already knows stops working.

| Field | Produces | Source of options |
|---|---|---|
| **Name** | directory, package, agent-card `name` | free text |
| **Purpose** (one paragraph) | `AGENTS.md` summary, agent-card `description`, the first eval cases | free text |
| **Framework** | code skeleton via the existing `ScaffoldConfig`; A2A server wiring | ADK (default), LangGraph, CrewAI, AutoGen, plain A2A |
| **Persona** | `PERSONA.md` with `extends: <picked>` | library picker: project, user, and the three starters |
| **Skills** | `.agents/skills/<name>/SKILL.md` per pick, stubs for any typed inline | installed skills + s8sskills search |
| **MCP servers** | the agent's MCP config stanza | Dock's installed servers |
| **Team** (toggle) | N sibling agents + a coordinator, all A2A | off by default |
| **Evals** (toggle, default on) | `evals/` + the loop task | — |
| **Deploy target** (optional) | Set Sail "rig the pipeline" hand-off after scaffold | Set Sail's platform list |

Model, keys and sandbox settings are not fields: the scaffolded agent inherits
whatever the workspace already has, exactly as a Build-MCP server does.

## What gets generated

For a single ADK agent named `triage`, with the QA Engineer persona, two skills
and evals on:

```
triage/
├── AGENTS.md                    # operating contract: purpose, boundaries, tools, loop
├── PERSONA.md                   # extends: qa-engineer, with the brief's specifics
├── agent.py                     # from `adk create`, then wired for A2A
├── pyproject.toml               # uv; a2a-sdk added
├── .well-known/
│   └── agent-card.json          # name, description, skills, endpoint
├── .agents/
│   └── skills/
│       ├── triage-issues/SKILL.md
│       └── write-repro/SKILL.md
├── .mcp.json                    # the servers picked in the brief
├── harness.toml                 # tools allowed, approval policy, token budget
└── evals/
    ├── cases.jsonl              # 5–10 cases derived from the purpose paragraph
    ├── judge.md                 # the rubric, as a skill the judge follows
    └── loop.toml                # the run → judge → record task
```

Three files are new conventions and need a decision (see
[Open questions](#open-questions)): `harness.toml`, `evals/loop.toml`, and the
MCP config file name. Everything else is a convention PaddleBoard or the
framework already uses.

**`AGENTS.md` is the contract, `PERSONA.md` is the voice, `SKILL.md` is the
capability.** The scaffold keeps them separate on purpose. Collapsing them into
one file is the mistake every hand-rolled agent repo makes, and the reason the
three-file split exists in the product already.

## The loop

`evals/loop.toml` defines one task: run every case in `cases.jsonl` against the
agent, score each with `judge.md`, append a row to `evals/results.jsonl`, and
print the delta from the last run. It runs in the sandbox like any other
long-running task, and it is registered as a PaddleBoard task so it appears in
the task picker and the Agent Threads tree.

What the in-editor agent is told, at the end of the scaffold prompt:

> The agent is scaffolded. Run the eval loop once to establish a baseline, then
> read the lowest-scoring case and propose one change to `AGENTS.md` or a
> `SKILL.md` that would fix it. Do not change code for a prompt problem.

That last sentence is the loop-engineering discipline in one line, and it is
what the harness is for: the agent iterates on its own definition against
evidence, in the editor, with the user watching the tree.

## The catalog

A new s8sskills-style repo, `agent-scaffold`, one pack per framework plus one
shared pack:

| Pack | Carries |
|---|---|
| `agent-scaffold-core` | the `AGENTS.md` shape, the `PERSONA.md` hand-off, `harness.toml`, the evals layout, the agent-card schema |
| `agent-scaffold-adk` | `adk create` + A2A wiring for ADK, `adk web` dev loop |
| `agent-scaffold-langgraph` / `-crewai` / `-autogen` / `-a2a` | the same for each framework |
| `agent-scaffold-team` | the coordinator pattern: N agents, one card each, A2A between them |

The modal installs `core` plus the framework pack into `.agents/skills/` and
the prompt names them, exactly as Set Sail names `cloud-run-deploy`. Adding a
framework later is a pack and a row in `ScaffoldConfig`, not a Rust feature.

## Phases

### Phase 1 — single agent, ADK, evals on (the recommended scope)

- The modal, the brief, the `core` and `adk` packs, the generated tree above.
- `langgraph`, `crewai` and `a2a` packs follow in the same release if they are
  small; AutoGen (no scaffold CLI, Studio-only) can wait.
- The loop task registered and runnable from the Agent Threads tree.
- The existing `<fw>: Scaffold Agent` actions re-pointed at the new modal.
- Deploy target field present but it only hands off to Set Sail; no new
  deploy logic.

### Phase 2 — teams and the Dock (⏭)

- The Team toggle: coordinator + N agents, A2A between them, one tree node
  per agent in Agent Threads (this is also where the deferred Scion/ADK
  hierarchy work lands, since the scaffold now knows the parent).
- A scaffolded agent appears in the AI Dock's Agents tab with its card, its
  last eval score and a Run button.
- Eval results surfaced in the usage tab next to token spend, so cost per
  improvement is visible.

### Not in scope

- A hosted registry of agents or evals. The catalog is files in a repo.
- Running evals against models the user has not configured.
- Any change to how personas, skills or MCP servers are installed today.

## Why this waits

- **The v0.3.0 cut is Tue 13 Oct and the freeze holds to it.** This touches the
  Dock, the frameworks crate, actions and keymaps. None of it belongs in the
  tree a week before a cut that has moved four times.
- **The beta should pick the first eval conventions, not this doc.** Testers
  will arrive with their own `AGENTS.md` and evals layouts. Two weeks of
  Beta Feedback is cheap evidence about which conventions to generate and
  which to detect and respect.
- **It is a milestone, not a fix.** Minor numbers are milestones on this line
  (v0.1.13 sandbox, v0.1.18 RAG, v0.2.0 Glowup). This is the v0.4 headline.

## Open questions

1. **Which files are canon.** `AGENTS.md`, `PERSONA.md`, `SKILL.md` are
   settled. Whether the scaffold also writes per-tool files (`CLAUDE.md`,
   `GEMINI.md`) or leaves them to the user changes the scope and the catalog.
   Recommendation: `AGENTS.md` only, with a one-line `CLAUDE.md` that imports
   it, since that is the pattern this repo itself uses.
2. **`harness.toml`.** A new file. The alternative is folding tool permissions
   and budget into `AGENTS.md` frontmatter. A separate file is machine-readable
   and diffable; frontmatter is one file fewer. Jay's call.
3. **The MCP config file name.** Frameworks disagree (`.mcp.json`,
   `mcp.json`, settings keys). The pack can write whichever the framework
   reads; the question is whether PaddleBoard also mirrors it into the Dock.
4. **Catalog ownership.** A second s8sskills-style repo under the s8sskills
   org, or a `paddleboarddev/agent-scaffold` repo. The former keeps one
   catalog story; the latter keeps agent conventions out of a deploy catalog.
5. **Evals in Phase 1 or Phase 2.** The recommendation above is Phase 1,
   because it is the distinctive part. It is also the most work, and a
   file-only scaffold without the loop would still be a real improvement.
6. **The name.** "Agent Scaffolding" describes it. If it wants the product's
   nautical voice alongside Set Sail and Scion, that is a naming session, not
   a decision this doc should make.

## Reference

- `crates/paddleboard_agent_frameworks/src/scaffold_modal.rs` — the current
  `ScaffoldConfig` table and modal, to be widened rather than replaced.
- `crates/paddleboard_ai_dock/src/ai_dock/build_mcp_modal.rs` — the
  brief → prompt → `seed_prompt_thread` pattern.
- `crates/paddleboard_set_sail/src/paddleboard_set_sail.rs` —
  `ensure_skill_installed`, the catalog fetch and the prompt that names packs.
- `crates/paddleboard_personas/src/paddleboard_personas.rs` — persona file
  locations and the `extends:` resolution.
- `crates/paddleboard_agent_frameworks/src/a2a.rs` — A2A run and the
  agent-card landing path.
- `SCION_HUB.md` — the Scion Hub design, whose Phase 2 tree work this
  feature's Team toggle would share.
- `WELCOME.md` §"Recipe — build & run an ADK agent" — the end-to-end ADK
  flow the scaffold should leave the user inside.
