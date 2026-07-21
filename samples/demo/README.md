# PaddleBoard demo — presenter script 🏄

A small FastAPI service (**Paddle Log**) plus an ordered walkthrough that
exercises every headline PaddleBoard feature.

The app is deliberately boring — the point is the editor, not the code. It's
dependency-light, runs offline, has a passing test suite, one deliberately
failing test for the agent to fix, and a `Dockerfile` so it can be sandboxed and
deployed.

**Total run time:** ~20 minutes for the full script, or pick sections à la carte.
Each step lists what you should *see*, so you know if it worked.

---

## Setup (before the audience arrives)

```bash
cd samples/demo
python3 -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt
pytest          # expect: 11 passed, 1 xfailed
```

Open `samples/demo` as its own PaddleBoard window (`File → Open…`) so the file
tree and git state are scoped to the sample.

> **Tip:** if you're demoing first-launch (§1), do it *before* opening the
> project, and reset state first:
> `rm ~/.config/paddleboard/.tour_seen` and clear the `first_open` key.

---

## 1. First launch — theme, provider setup, tour

**Feature:** onboarding, PaddleBoard theme, guided tour

1. Launch PaddleBoard with fresh state. → Onboarding page appears.
2. Point out the **Theme** row: **PaddleBoard** is first and preselected.
   → The UI is the navy/lavender PaddleBoard palette, matching paddleboard.dev.
3. Scroll to **AI Providers**. → A **Local Models — no API key needed** hero card
   with a model picker, and **Or bring your own key** rows below it.
4. Click **Finish Setup**. → Welcome page, with a **Take the Tour** button.
5. Click it. → The tour opens as a **rendered markdown preview** (styled
   headings and links, not raw text), showing six curated stops.

*Talking point: a new user finishes onboarding with a working model — no dead end.*

## 2. The agent edits real code

**Feature:** agent panel, step-through mode, context gauge

1. Open the agent panel (right dock).
2. Ask: *"What does `longest_consecutive_run` do, and where is it used?"*
   → It reads `app/storage.py` and explains the streak logic.
3. Turn on **step-through mode** (⏭ in the thread toolbar; it turns accent-colored).
4. Ask: *"Add a `GET /spots` endpoint returning each distinct spot with a session count."*
   → Each tool call now pauses for **Step** / **Skip**.
5. Watch the **context gauge** in the status bar as the thread grows.
   → Percentage climbs; hover for the token breakdown.

## 3. Personas — tell the agent who to be

**Feature:** personas

1. Show `PERSONA.md` in the project root.
2. Start a **new** agent thread. → It auto-adopts *Demo Reviewer*.
3. Ask the same question as §2.2. → Note the changed voice: states intent before
   changing anything, refuses speculative abstraction.
4. Mid-thread, say: *"be my QA tester instead."* → It switches via `adopt_persona`.
5. Browse **AI Dock → Personas** for the starter library.

## 4. Sandboxed test runs

**Feature:** secure agent sandbox, backend tiers

1. Click the **shield** in the status bar. → Backend picker; note which tier is
   active on *this* machine (Native / libkrun microVM / Podman + gVisor).
2. Ask the agent: *"Run the test suite."*
   → Runs in the sandbox; project mounted at `/workspace`; permission prompts
   still gate each command.
3. → Expect **11 passed, 1 xfailed**.

*Talking point: your choice of tier is honored — Native is never silently swapped.*

## 5. Fix the failing test

**Feature:** agent editing + sandbox verification loop

1. Open `tests/test_api.py`, scroll to `test_stats_ignores_duplicate_days`
   (marked `xfail`). It asserts a `unique_days` field that doesn't exist yet.
2. Ask: *"Make `test_stats_ignores_duplicate_days` pass. Remove the xfail marker
   once it does."*
   → The agent adds `unique_days` to `PaddlerStats`, populates it in
   `stats_for`, drops the marker, and re-runs the tests.
3. → Expect **12 passed**.

*Talking point: this is the whole loop — read, edit, verify in a sandbox.*

## 6. Semantic search — find code by meaning

**Feature:** built-in agentic RAG

1. Enable it if needed: `"paddleboard_rag": { "enabled": true }`.
   (First run downloads the ~0.33 GB EmbeddingGemma model.)
2. Ask: *"Use semantic_search to find where we decide if two sessions are on
   back-to-back days."*
   → Ranked hits pointing at `longest_consecutive_run` — **without** the word
   "streak" or "consecutive" appearing in the query.
3. Contrast with a plain text search for "streak" to show the difference.

## 7. Forwarded ports + embedded browser

**Feature:** sandbox services, browser panel

1. Ask: *"Start the API server on port 8000."*
2. → A **Forwarded Ports** row appears above the browser viewport (`http :8000`).
3. Click it. → The embedded browser opens `http://localhost:8000/docs` —
   FastAPI's Swagger UI, inside the editor.
4. POST a session through Swagger, then GET `/paddlers/Jay/stats`.
5. Click the **×** on the port row. → Service stops, entry disappears.

## 8. Manifest + Git Graph

**Feature:** Manifest panel, git surfaces

1. Open **Manifest** (tree icon, or `manifest: Toggle Focus`).
   → Repositories / Branches (ahead-behind) / Commits / Stashes / Contributors.
2. Click a commit. → Full diff.
3. Open **Git Graph** for deep history; right-click a commit →
   **Copy SHA**, **Copy Tag**, **Open Commit View**.

*Talking point: Manifest is the overview, Git Graph is the deep history.*

## 9. AI Dock

**Feature:** AI Dock — agents, skills, MCP, personas, usage

1. `Cmd-Shift-P` → **`ai_dock: Open`**.
2. Walk the tabs: **Agents** (catalog + install), **Skills**, **Personas**,
   **MCP Servers**, **Usage**.
3. **Usage** tab: today / 7-day / all-time token totals, split per provider and
   model — all local, stored as daily JSON files.

## 10. MCP servers (and building one)

**Feature:** sandboxed MCP, Build an MCP

1. AI Dock → **MCP Servers**. Filter All / Running / Stopped / Error.
2. Install one from the catalog. → It runs inside the same sandbox.
3. Click **Build an MCP**, name it (e.g. *Tide Times*) and describe the API.
   → An agent researches it, writes the server, tests it in the sandbox,
   and installs it.

## 11. Orchestration + Scion

**Feature:** orchestration panel, container-isolated agents

1. Open the **Orchestration** panel. → Live tree of every agent session,
   subagents nested under their parent.
2. Ask for something with subagents: *"Review this codebase for bugs using
   subagents."* → Watch them appear and report status.
3. If Scion is enabled, start a container-isolated agent
   (`scion: Start Agent`) and show **View Logs** streaming live.

## 12. Set Sail — deploy to serverless ⛵

**Feature:** Set Sail

1. `Cmd-Shift-P` → **`set sail: Deploy`**.
2. Pick a platform (Cloud Run is the quickest), a service name, public access.
3. → PaddleBoard installs the platform's [s8sskills](https://s8sskills.com)
   pack into `.agents/skills/` and the agent follows that playbook, using the
   `Dockerfile` in this folder.
4. Interactive auth (`gcloud auth login`) is handed to **you** in the terminal —
   the agent never runs auth flows itself.
5. → A live URL; hit `/healthz` in the embedded browser.
6. Show **Rig the Pipeline** mode for CI/CD groundwork instead of a direct deploy.

## 13. Local Models — no API key

**Feature:** managed llama.cpp

1. AI provider settings → **Local Models**.
2. Flip **Run locally, managed by PaddleBoard**, pick **Gemma 3 4B**.
3. → Live progress bar: *downloading → starting → ready*. The model then appears
   in the agent's picker like any other.
4. Ask it something small to prove it answers with no network provider.

*Talking point: signed llama.cpp, bound to 127.0.0.1, Metal-accelerated.*

## 14. Git Login

**Feature:** Git Login + OAuth

1. `Cmd-Shift-P` → **`git login: Manage`**.
2. Show GitHub / GitLab **browser OAuth** (device page opens pre-filled) and the
   PAT path for self-managed hosts.
3. Credentials land in the **OS keychain**; HTTPS `fetch`/`push` stop prompting.

## 15. Odds and ends

Quick hits if you have time:

- **Multi-workspace** — `git: Worktree` to open a second project in one window;
  the orchestration panel shows threads from both.
- **Search as you type** — project search updates as you type, no Enter.
- **Language support** — `Manage Languages` for the install-on-demand tier.
- **Chrome toggles** — every PaddleBoard status item and dock panel can be
  hidden (Glowup Wave 1); right-click the status bar to show it off.

---

## Reset between runs

```bash
git checkout -- samples/demo          # undo agent edits
rm -rf samples/demo/.venv
rm ~/.config/paddleboard/.tour_seen   # to re-show the tour
```

## Keeping this current

This script should track the shipped feature set. When a headline feature lands
or changes, update the matching section here as well as `WELCOME.md`. If a step's
"expected result" no longer matches reality, fix it before the next demo — a
stale presenter script is worse than none.
