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
- Click the × → the container stops and the entry disappears.
- Bindings stay on `127.0.0.1` only — never your LAN.

### 4. Sandboxed MCP Servers
PaddleBoard runs your **MCP servers** inside the same Podman + gVisor sandbox as the Sandbox Tool.
- Manage them in the AI Dock: `Cmd-Shift-P` → **`paddleboard: Mcp Servers`** opens the dock on the MCP tab (filter All / Running / Stopped / Error, add servers, browse the catalog of common ones).
- Or use `"source": "sandboxed_stdio"` in `settings.json` directly.
- Forward only the host env vars you need by name — values stay out of the agent's context.
- The worktree is mounted at `/workspace` so filesystem-touching servers (git, fs, etc.) still work.

### 5. AI Dock
One place to browse and install everything the agent talks to — the marina where every external collaborator ties up.
- Open it: `Cmd-Shift-P` → **`ai_dock: Open`**, or hit **Open the AI Dock** on the Welcome screen. The Welcome screen also surfaces a **Featured** strip (Claude / Codex / Copilot / Cursor / Gemini pills) so first-run users have recognizable names to click.
- Three tabs: **Agents** (Zed, Claude, Codex, Copilot, Cursor), **Skills** (slash commands), **MCP Servers** (catalog + absorbed management UI).
- Installed items show a green badge; missing ones get a one-click **Install / Sign In / Learn More** that does the category-appropriate thing. Bundled skills (currently `/build` and `/update-tour`) install with **Add to project** / **Add to user** buttons that drop a markdown file into the right `.claude/commands/` directory.
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
- Install: `go install github.com/GoogleCloudPlatform/scion/cmd/scion@latest`, then `scion init --machine` and `scion init` in your project.
- Start an agent: `Cmd-Shift-P` → **`scion: Start Agent`** opens a modal where you set a task description, agent name, and template.
- The **Orchestration Panel** shows a **Scion Agents** section below native threads. Each row shows the agent's phase (provisioning → running → stopped) and activity (working, thinking, waiting, etc.).
- Right-click any agent row for **View Logs** (opens last 200 lines in a read-only editor tab), **Sync Changes** (pulls the agent's worktree changes into your local copy), or **Stop Agent**.
- Status colors: accent = running, warning = needs attention, error = errored, muted = stopped.

### 9. LLM Provider Picker Panel
A dedicated panel for switching the active language model provider without opening settings.
- Dock it wherever is convenient and change providers as you work.
- **ChatGPT Subscription auth**: sign in with your ChatGPT Plus or Pro account via OAuth — no API key needed. The flow opens in the embedded browser panel; tokens persist in PB's credential store.

### 10. Multi-Workspace
Keep multiple projects in one window, each as its own workspace with its own pane tree and its own agent threads.
- Open the worktree picker: `Cmd-Shift-P` → **`git: Worktree`**.
- **Switch** between existing worktrees, **create** a new worktree-backed workspace (accept the auto-generated branch name like `dusty-pelican` or supply your own), or **open in new window**.
- The orchestration panel shows agent threads from every workspace at once — perfect for parallel agent sessions against different projects.

### 11. Built-in Language Servers
PaddleBoard ships built-in LSP support for four languages that Zed historically punts to extensions — **no extension installation required**.
- **Java** via [jdtls](https://github.com/eclipse/eclipse.jdt.ls)
- **Kotlin** via [kotlin-language-server](https://github.com/fwcd/kotlin-language-server)
- **PHP** via [intelephense](https://intelephense.com/)
- **Swift** via [SourceKit-LSP](https://github.com/swiftlang/sourcekit-lsp)
- Open a `.java`/`.kt`/`.php`/`.swift` file; PB downloads the language server on first use and caches it.

---

*You can always revisit this tour by opening the Command Palette (`Cmd-Shift-P`) and selecting **`workspace: Open Paddle Board Tour`**.*
