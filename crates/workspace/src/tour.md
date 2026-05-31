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
- **More frameworks**, same Scaffold/Run/Stop pattern, each auto-detected with a toast: **LangGraph** (`langgraph dev`), **CrewAI** (`crewai run`), and **AutoGen** (AutoGen Studio web UI). All appear in the AI Dock with a **Set Up** install button.

### 4. Sandboxed MCP Servers
PaddleBoard runs your **MCP servers** inside the same Podman + gVisor sandbox as the Sandbox Tool.
- Manage them in the AI Dock: `Cmd-Shift-P` → **`paddleboard: Mcp Servers`** opens the dock on the MCP tab (filter All / Running / Stopped / Error, add servers, browse the catalog of common ones).
- Or use `"source": "sandboxed_stdio"` in `settings.json` directly.
- Forward only the host env vars you need by name — values stay out of the agent's context.
- The worktree is mounted at `/workspace` so filesystem-touching servers (git, fs, etc.) still work.

### 5. AI Dock
One place to browse and install everything the agent talks to — the marina where every external collaborator ties up.
- Open it: `Cmd-Shift-P` → **`ai_dock: Open`**, or hit **Open the AI Dock** on the Welcome screen. The Welcome screen also surfaces a **Featured** strip (Claude / Codex / Copilot / Cursor / Gemini pills) so first-run users have recognizable names to click.
- Three tabs: **Agents** (Zed, Claude, Codex, Copilot, Cursor, Google ADK), **Skills** (slash commands), **MCP Servers** (catalog + absorbed management UI).
- Installed items show a green badge; missing ones get a one-click **Install / Sign In / Set Up / Learn More** that does the category-appropriate thing. CLI-based agents (like Google ADK) show a **Set Up** button that opens a terminal with the install command. Bundled skills (`/build`, `/update-tour`, `/clippy`, `/test`, `/check-drift`) install with **Add to project** / **Add to user** buttons that drop a markdown file into the right `.claude/commands/` directory.
- **Add Agent** (Agents tab header): opens a modal to register a custom agent server by its registry ID — for agents not in the catalog.
- **Create Skill** (Skills tab header): opens a modal with a name field, prompt editor, and project/user scope toggle. Creates a new `.claude/commands/{name}.md` file.
- The catalog is `assets/ai_dock/catalog.json` in-repo — adds are PRs, not fetches.

### 6. Step-Through Mode
Approve every tool call before the agent executes it.
- Click the **⏭** icon in the agent thread toolbar to enable (it turns accent-colored).
- Each tool call pauses with **Step** (run it) or **Skip** (return empty and move on).
- Only the root thread is gated — subagents run without interruption.

### 7. Agent Orchestration Panel
A live tree view of every active agent session, including subagents.
- Open it: panel bar `ListTree` icon, or `Cmd-Shift-P` → **`orchestration_panel: Toggle Focus`**.
- Subagents nest under the thread that spawned them.
- Status dot shows generating vs. idle; click any row to jump to that thread.

### 8. Scion — Container-Isolated Parallel Agents
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

### 9. LLM Provider Picker Panel
A dedicated panel for switching the active language model provider without opening settings.
- Dock it wherever is convenient and change providers as you work.
- **ChatGPT Subscription auth**: sign in with your ChatGPT Plus or Pro account via OAuth — no API key needed. The flow opens in the embedded browser panel; tokens persist in PB's credential store.
- **Vertex AI (Gemini Enterprise)**: run Gemini through your own GCP project. Configure it right in the agent settings — fill in a Project ID and Save. Recommended auth stores no key (`gcloud auth login` + borrow short-lived tokens); a service-account key file or Vertex Express API key also work.

### 10. Multi-Workspace
Keep multiple projects in one window, each as its own workspace with its own pane tree and its own agent threads.
- Open the worktree picker: `Cmd-Shift-P` → **`git: Worktree`**.
- **Switch** between existing worktrees, **create** a new worktree-backed workspace (accept the auto-generated branch name like `dusty-pelican` or supply your own), or **open in new window**.
- The orchestration panel shows agent threads from every workspace at once — perfect for parallel agent sessions against different projects.

### 11. Language Support — Two Tiers
PaddleBoard keeps the default install lean and lets you add the rest with one click.
- **Ready to use**: Rust, TypeScript, JavaScript, Python, Go, JSON, YAML, HTML/CSS attach automatically — open a file and the server downloads on first use.
- **Install support** (run **`Manage Languages`** from the Command Palette): **Java**, **Kotlin** (JDK 17+), **PHP** (Node), **C#** (.NET), and **C++** (clangd) ship a built-in server — click Install and PaddleBoard downloads the binary. Each shows its prerequisite up front.
- **Install support** (opt-in via `Manage Languages`): Java, Kotlin, PHP, C#, C++, and **Swift** (SourceKit-LSP from your toolchain, resolved from PATH). **Ruby** and **Dart** come from extensions.
- **Build tool context**: Java and Kotlin auto-detect Gradle/Maven projects and expose `JAVA_BUILD_TOOL` and `JAVA_PROJECT_ROOT` task variables.

---

*You can always revisit this tour by opening the Command Palette (`Cmd-Shift-P`) and selecting **`workspace: Open Paddle Board Tour`**.*
