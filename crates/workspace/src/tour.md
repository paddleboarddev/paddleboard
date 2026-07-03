# Welcome to PaddleBoard! 🏄‍♂️

PaddleBoard is your new agentic, highly-performant IDE fork designed specifically for AI-driven software development. You can code natively, let the AI act on your workspace, browse the web, and run tests in secure sandboxes all from one window!

## 🚀 Key Features

### 1. Embedded Browser Panel
A native Chromium/WebKit browser lives inside the editor as a dockable panel.
- Open it: `Cmd-Shift-P` → **`workspace: Open Browser`**.
- Type a URL or search query in the address bar and press Enter.
- Quick-access bookmarks (Google, GitHub, Hacker News) are one click away.
- **Unsloth Studio**: `Cmd-Shift-P` → **`workspace: Open Unsloth`** spins up a containerised Jupyter server and points the browser at it once it's ready.

### 2. Secure Agent Sandboxing (Podman + gVisor)
When the AI needs to run untrusted code, compile new binaries, or run tests, it uses the integrated **Sandbox Tool**.
- All executions happen in an ephemeral `ubuntu:latest` container.
- It uses the `runsc` (gVisor) runtime for deep isolation.
- Your project directory is safely mounted so builds succeed without host contamination.
- Permission prompts still gate every command — approve, deny, or set always-allow rules.
- Watch the **shield icon** in the status bar: green = ready, yellow = degraded, red = missing. Click it for live status + copy-paste install commands.
- Missing prereqs are enforced, not silently swallowed. `paddleboard_sandbox.on_missing_runtime` chooses what happens: `block` (default, surface install modal), `fall_back_to_host` (run unsandboxed), or `warn_once`.

### 3. Forwarded Ports — Sandbox Services
Long-lived processes (dev servers, demo apps, `adk web`) use the **Sandbox Service Tool**. Each running service appears in a **Forwarded Ports** row above the browser viewport.
- Click the label (e.g. `http :54321`) → the browser panel navigates to `http://localhost:54321`.
- Click the × → the service stops and the entry disappears.
- Bindings stay on `127.0.0.1` only — never your LAN.
- Non-container ports appear here too — `adk: Run Agent` registers port 8000 from the terminal so you get the same one-click navigation.
- **ADK quick start**: `Cmd-Shift-P` → **`adk: Scaffold Agent`** to create a new Google ADK agent project, then **`adk: Run Agent`** to launch `adk web` — port 8000 appears in Forwarded Ports automatically. Open a folder with `agent.py` or `agent.yaml` and PaddleBoard shows a toast with a **Run Agent** button.
- **More frameworks**, same Run/Stop pattern, each auto-detected with a toast: **LangGraph** (`langgraph dev`), **CrewAI** (`crewai run`), **AutoGen** (AutoGen Studio web UI), and **A2A** (`uv run .`, runs your local [A2A](https://a2a-protocol.org/) agent server). All appear in the AI Dock with a **Set Up** install button.

### 4. Sandboxed MCP Servers
PaddleBoard runs your **MCP servers** inside the same Podman + gVisor sandbox as the Sandbox Tool.
- Manage them in the AI Dock: `Cmd-Shift-P` → **`paddleboard: Mcp Servers`** opens the dock on the MCP tab (filter All / Running / Stopped / Error, add servers, browse the catalog of common ones).
- 🛠️ **Build an MCP** — no server for a service? Click **Build an MCP**, name it (e.g. Substack) + describe what you want, and an agent researches the API, writes the server, tests it in the sandbox, and installs it.
- Or use `"source": "sandboxed_stdio"` in `settings.json` directly.
- Forward only the host env vars you need by name — values stay out of the agent's context.
- The worktree is mounted at `/workspace` so filesystem-touching servers (git, fs, etc.) still work.

### 5. AI Dock
One place to browse and install everything the agent talks to — the marina where every external collaborator ties up.
- Open it: `Cmd-Shift-P` → **`ai_dock: Open`**, or hit **Open the AI Dock** on the Welcome screen. The Welcome screen also surfaces a **Featured** strip (Claude / Codex / Copilot / Cursor / Antigravity pills) so first-run users have recognizable names to click.
- Five tabs: **Agents** (Zed, Claude, Codex, Copilot, Cursor, Google ADK), **Skills** (slash commands), **Personas** (who the agent should be — see #6), **MCP Servers** (catalog + absorbed management UI), and **Usage** (local per-provider token stats — see #17).
- Installed items show a green badge; missing ones get a one-click **Install / Sign In / Set Up / Learn More** that does the category-appropriate thing. CLI-based agents (like Google ADK) show a **Set Up** button that opens a terminal with the install command. Bundled skills (`/build`, `/update-tour`, `/clippy`, `/test`, `/check-drift`) install with **Add to project** / **Add to user** buttons that drop a markdown file into the right `.claude/commands/` directory.
- **Add Agent** (Agents tab header): opens a modal to register a custom agent server by its registry ID — for agents not in the catalog.
- **Create Skill** (Skills tab header): opens a modal with a name field, prompt editor, and project/user scope toggle. Creates a new `.claude/commands/{name}.md` file.
- The catalog is `assets/ai_dock/catalog.json` in-repo — adds are PRs, not fetches.

### 6. Personas — Tell the Agent Who to Be
A **persona** describes who the agent should mimic — a Senior Developer, an SRE, a QA Engineer. 🎭
- Drop a `PERSONA.md` at your project root → new agent threads adopt it automatically. Plain prose works; frontmatter optional.
- Keep a library as `.claude/personas/*.persona.md` files (project or `~/.claude/personas/` for user-wide), then switch per thread with the **persona picker** next to the profile selector in the agent panel.
- Grab starter roles in **AI Dock → Personas** — Senior Developer, Site Reliability Engineer, QA Engineer — with one-click **Add to project / Add to user**.
- Or just ask mid-conversation — "be my QA tester" — and the agent switches itself via its `adopt_persona` tool.
- Works identically with every LLM provider; the persona is saved with the thread. Opt out with `"paddleboard_personas": { "enabled": false }`.

### 7. Set Sail — Deploy to Serverless
Quick-deploy the current project to **Cloud Run, AWS Lambda, or Vercel**, no YAML safari. ⛵
- Run it: `Cmd-Shift-P` → **`set sail: Deploy`** — pick a platform, service name, and public/private, then the agent takes the helm.
- Powered by the open-source [s8sskills](https://s8sskills.com) catalog: PaddleBoard installs the platform's skill pack into `.agents/skills/` and the agent follows that playbook.
- Interactive steps (`gcloud auth login`, `aws configure`, `vercel login`) are handed to you in the terminal — the agent never runs auth flows itself.
- More platforms (Azure, Cloudflare, Netlify…) arrive as skill packs; a future **Rig the pipeline** mode will set up git-push CD.

### 8. Step-Through Mode
Approve every tool call before the agent executes it.
- Click the **⏭** icon in the agent thread toolbar to enable (it turns accent-colored).
- Each tool call pauses with **Step** (run it) or **Skip** (return empty and move on).
- Only the root thread is gated — subagents run without interruption.

### 9. Agent Orchestration Panel
A live tree view of every active agent session, including subagents.
- Open it: panel bar `ListTree` icon, or `Cmd-Shift-P` → **`orchestration_panel: Toggle Focus`**.
- Subagents nest under the thread that spawned them.
- Status dot shows generating vs. idle; click any row to jump to that thread.

### 10. Scion — Container-Isolated Parallel Agents
Run multiple deep agents in parallel, each in its own container and git worktree, via [Scion](https://github.com/GoogleCloudPlatform/scion).
- **Opt-in:** enable with `"paddleboard_scion": { "enabled": true }` in settings (installing the CLI alone won't turn it on).
- Install: `go install github.com/GoogleCloudPlatform/scion/cmd/scion@latest`, then `scion init --machine` and `scion init` in your project.
- Start an agent: `Cmd-Shift-P` → **`scion: Start Agent`** opens a modal where you set a task description, agent name, and template.
- The **Orchestration Panel** shows a **Scion Agents** section below native threads. Each row shows the agent's phase (provisioning → running → stopped) and activity (working, thinking, waiting, etc.).
- Right-click any agent row for **View Logs** (opens a live-streaming log tab that tails output in real time), **Sync Changes** (pulls changes into your local copy and shows a toast), or **Stop Agent**.
- **Agents can delegate to Scion themselves:** with the CLI installed, the agent gains a `spawn_scion_agent` tool — it hands a subtask to a container + worktree-isolated agent (instead of an in-process sub-agent that shares your workspace), waits for it, and returns the result.
- Activity badges show what the agent is doing: "executing · Edit", "thinking", "waiting", etc.
- Status colors: accent = running, warning = needs attention, error = errored, muted = stopped.
- **OpenTelemetry tracing:** Enable `"paddleboard_otel": { "enabled": true }` in settings (or `PADDLEBOARD_OTEL_ENABLED=1`) to export agent lifecycle telemetry via OTLP. Poll cycles, CLI commands, and phase/activity transitions appear as spans and events in Jaeger, Tempo, or any OTEL-compatible collector.

### 11. LLM Provider Picker Panel
A dedicated panel for switching the active language model provider without opening settings.
- Dock it wherever is convenient and change providers as you work.
- **ChatGPT Subscription auth**: sign in with your ChatGPT Plus or Pro account via OAuth — no API key needed. The flow opens in the embedded browser panel; tokens persist in PB's credential store.
- **Vertex AI (Gemini Enterprise)**: run Gemini through your own GCP project. Configure it right in the agent settings — fill in a Project ID and Save. Recommended auth stores no key (`gcloud auth login` + borrow short-lived tokens); a service-account key file or Vertex Express API key also work.

### 12. Multi-Workspace
Keep multiple projects in one window, each as its own workspace with its own pane tree and its own agent threads.
- Open the worktree picker: `Cmd-Shift-P` → **`git: Worktree`**.
- **Switch** between existing worktrees, **create** a new worktree-backed workspace (accept the auto-generated branch name like `dusty-pelican` or supply your own), or **open in new window**.
- The orchestration panel shows agent threads from every workspace at once — perfect for parallel agent sessions against different projects.

### 13. Language Support — Two Tiers
PaddleBoard keeps the default install lean and lets you add the rest with one click.
- **Ready to use**: Rust, TypeScript, JavaScript, Python, Go, JSON, YAML, HTML/CSS, and **Dockerfile** attach automatically — open a file and the server downloads on first use. Dockerfiles get highlighting + `docker-langserver` out of the box.
- **Install support** (run **`Manage Languages`**): **Java**, **Kotlin** (JDK 17+), **PHP** (Node), **C#** (.NET), **C++** (clangd), and **Swift** (SourceKit-LSP, PATH-resolved from your toolchain) ship a built-in server — click Install, prerequisite shown up front. **Ruby** and **Dart** come from extensions.
- **Build tool context**: Java and Kotlin auto-detect Gradle/Maven projects and expose `JAVA_BUILD_TOOL` and `JAVA_PROJECT_ROOT` task variables.
- **Prose checking**: Markdown and git commit messages get offline spelling + grammar squiggles via [Harper](https://writewithharper.com) — private, no text leaves your machine. Keep a deliberate word with `cmd-.` → **Add to dictionary**.

### 14. Git Login
Save your git host credentials once so HTTPS git operations stop prompting.
- Run **`git login: Manage`** → pick GitHub, GitLab, BitBucket (or a custom host), paste a Personal Access Token. Stored in your **OS keychain**.
- `clone`/`fetch`/`push` over HTTPS then authenticate silently; the prompt only returns if there's no saved login.
- When the prompt does appear, tick **"Remember on this device"** as you submit — saved to the keychain, no prompt next time.
- `GITHUB_TOKEN` / `GITLAB_TOKEN` / `BITBUCKET_TOKEN` work as a fallback; SSH is untouched.
- On GitHub, builds with an OAuth client id offer **Sign in with GitHub (browser)** — approve a short code on github.com and you're done.

### 15. Search As You Type
Project search runs as you type — results update a beat after you pause, no Enter needed. 🔍
- Prefer the classic behavior? Set `"search": { "search_on_type": false }` in settings.

### 16. Agent Context Gauge
Watch the status bar while an agent thread runs — a percentage shows how much of the model's context window you've used. 🌊
- Hover for the token breakdown (used / total, input vs. output); click to jump to the agent panel.
- Goes yellow near the limit, red past it; hidden when no thread is active.
- Purely local — reads counts the thread already tracks. Telemetry stays off.

### 17. Local Usage Stats
Mix multiple providers? PaddleBoard tracks **how your token usage splits across them** over time — all on your machine. 📊
- Open the **AI Dock → Usage** tab: today / 7-day / all-time totals, then a per-provider, per-model breakdown.
- Stored as a **text JSON file per day** (`<data_dir>/usage/`) — point `paddleboard_usage.directory` at your own private git repo to back it up; clean diffs guaranteed.
- Counts every provider equally (Anthropic, OpenAI, Gemini, Vertex, Bedrock, Ollama, …) — recorded at the one spot every billed token flows through.
- Opt-out via `"paddleboard_usage": { "enabled": false }`; optional `auto_commit` commits the files for you.

---

*You can always revisit this tour by opening the Command Palette (`Cmd-Shift-P`) and selecting **`workspace: Open Paddle Board Tour`**.*
