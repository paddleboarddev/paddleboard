# Welcome to PaddleBoard

PaddleBoard is a fork of the [Zed editor](https://zed.dev) purpose-built for AI-driven development. It keeps everything you love about Zed — speed, native GPU rendering, LSP, debugging, git — and layers on features that turn the editor into a full agent workbench.

---

## Features unique to PaddleBoard

### Embedded browser panel

A native Chromium/WebKit browser lives inside the editor as a dockable panel.

- Open it from the command palette: `workspace: Open Browser`
- Type a URL or search query in the address bar and press Enter
- Quick-access bookmarks (Google, GitHub, Hacker News) are one click away
- Dock it to the left or right side of your workspace like any other panel

The browser stays in sync with your layout — it moves and resizes as you rearrange panels.

**Unsloth Studio** — launch an Unsloth fine-tuning environment in one step via `workspace: Open Unsloth`. PaddleBoard starts a containerised Jupyter server, waits for it to be ready, then navigates the browser panel directly to it.

---

### Secure agent sandbox (Podman + gVisor)

When the agent needs to run untrusted code, compile binaries, or execute tests, it uses the built-in Sandbox Tool instead of your host shell.

- Commands run inside an ephemeral `ubuntu:latest` container via **Podman**
- The `runsc` (gVisor) runtime adds a second layer of kernel-level isolation
- Your project directory is mounted inside the container so builds have full access to your source
- Supports a configurable timeout and can be cancelled mid-run
- The agent still goes through the normal permission layer — you can approve, deny, or set always-allow rules per command pattern

This means an agent mistake cannot touch anything outside the container. Once the command finishes, the container is discarded.

**Prerequisites are enforced.** PaddleBoard probes for Podman and gVisor on startup; the result lives in the status-bar shield icon (green = ready, yellow = degraded, red = missing). When prereqs are missing the agent can't silently fall through to a `podman: command not found` shell error — the policy in `paddleboard_sandbox.on_missing_runtime` decides:

- `"block"` (default) — refuse to launch and surface the install modal. The agent gets a clear "sandbox prerequisites missing" error.
- `"fall_back_to_host"` — run the command on the host without a container. Escape hatch for Windows or environments where the sandbox stack is genuinely unavailable.
- `"warn_once"` — emit a one-shot toast with install guidance, then proceed sandboxed.

Click the shield icon any time to see the live status and copy-paste install commands for your OS.

---

### Forwarded ports — sandbox services in the browser

When you want the agent to run something long-lived — a dev server, a demo app, an `adk web` UI — it uses the **Sandbox Service Tool**. PaddleBoard starts a detached Podman container (still under gVisor), publishes the service's port to a host port chosen by Podman, and surfaces it as a one-click link in the browser panel.

- The agent picks the container port (e.g. `python3 -m http.server 8000` → port `8000`); PaddleBoard handles the host-side mapping and binds it to `127.0.0.1` only — never your LAN
- Each running service shows up in a **Forwarded Ports** row above the browser viewport, labeled like `http :54321`
- Click the label → the browser panel navigates to `http://localhost:54321`
- Click the × → the service is stopped and the entry disappears
- The agent can wait for a readiness substring in the container logs before reporting success, so the URL it gives you is actually live
- Non-container ports also appear here — for example, `adk: Run Agent` registers port 8000 directly from the terminal so you get the same one-click navigation

For one-shot commands (builds, tests, scripts) the agent still uses the regular Sandbox Tool. The Service Tool is for processes that should outlive the tool call.

#### Recipe — build & run an ADK agent

Google's [Agent Development Kit](https://google.github.io/adk-docs/) ships a `adk web` UI that's a natural fit for PaddleBoard. Two command palette actions make it quick:

- **`adk: Scaffold Agent`** — opens a modal where you name your agent, then runs `adk create <name>` in a terminal tab. Writes `agent.py`, config, and dependencies into your workspace.
- **`adk: Run Agent`** — spawns `adk web` in a terminal tab and registers port 8000 in the Forwarded Ports row so you can click through to the dev server in the browser panel.
- **Project detection** — when you open a workspace that contains `agent.py` or `agent.yaml`, PaddleBoard shows a toast notification with a **Run Agent** button so you can launch the dev server in one click.
- **AI Dock entry** — Google ADK appears in the Agents tab of the AI Dock. If the `adk` CLI isn't on your PATH, a **Set Up** button opens a terminal with `pip install google-adk`.

**Beyond ADK**, the same command-palette pattern works for several agent frameworks, each auto-detected from your project and surfaced with a toast:

- **LangGraph** — `langgraph: Run Agent` launches `langgraph dev` (LangGraph Studio); detected from `langgraph.json`.
- **CrewAI** — `crewai: Scaffold Agent` runs `crewai create crew <name>`; `crewai: Run Agent` runs `crewai run` (one-shot, streamed to a tab — no server); detected when `crewai` is in your `pyproject.toml`/`requirements.txt`.
- **AutoGen** — `autogen: Run Agent` launches AutoGen Studio (`autogenstudio ui`), a local web UI on port 8081 surfaced in Forwarded Ports; detected when `autogen` is a project dependency.
- **A2A** — `a2a: Run Agent` launches a local [A2A](https://a2a-protocol.org/) agent server with `uv run .` (the a2a-samples convention), surfacing its port (9999 for the helloworld sample) in Forwarded Ports; detected when `a2a-sdk` is a project dependency. Requires `uv` on your PATH. This runs *your* A2A server locally — PaddleBoard doesn't yet speak the A2A protocol itself.

Each also appears in the AI Dock's Agents tab with a **Set Up** button that installs the CLI (`pip install …`).

For sandboxed execution via the agent, you can also ask in a chat thread: **"Run `adk web` in the sandbox, forwarding GOOGLE_API_KEY."** The agent will call `sandbox_service_tool` and the URL lands in the Forwarded Ports row.

The `forward_env` field accepts a list of host env var **names** only — values are read by the tool at run time and passed to the container via `podman run -e`, never serialized into the conversation.

---

### Sandboxed MCP servers

Most editors run **MCP (Model Context Protocol) servers** directly on your host. That means an MCP server has the same filesystem reach, network access, and credentials as you do — which doesn't match PaddleBoard's "everything the agent touches goes through Podman + gVisor" pitch.

PaddleBoard adds a fourth context-server transport, `sandboxed_stdio`, that runs the MCP server inside a `podman run -i --rm --runtime=runsc` container. Stdin and stdout are proxied transparently, so the JSON-RPC framing keeps working without any change on the agent side.

**Manage servers in the AI Dock** — `paddleboard: Mcp Servers` (or `ai_dock: Open` then the **MCP Servers** tab) opens the PaddleBoard AI Dock with the absorbed server view. You get the full add/filter (All / Running / Stopped / Error) / inspect surface plus a side-by-side **Available** catalog of well-known servers without hand-editing JSON.

**Build an MCP for any service** — when a service has no MCP server, the **Build an MCP** button at the top of the MCP tab generates one. Give it a service (e.g. `Substack`), an optional API-docs URL, an optional auth env-var name (e.g. `SUBSTACK_API_KEY`), and a sentence on what you want — PaddleBoard seeds an agent thread that researches the API, writes a Python (FastMCP) server, tests it in the sandbox, and installs it into the AI Dock so the agent can use it. The thread is visible, so you can watch and course-correct. The install is a host-side step (the `install_mcp_server` tool) that persists the server under PaddleBoard's data dir and registers it; the server runs on the host via `uv run` and reads its API key from the environment PaddleBoard was launched with, so no secret is ever written to settings. (`uv` must be on your PATH.)

You can still configure servers by hand in `settings.json` if you prefer:

```json
"context_servers": {
  "github": {
    "source": "sandboxed_stdio",
    "command": "github-mcp-server",
    "args": [],
    "image": "ghcr.io/github/github-mcp-server:latest",
    "forward_env": ["GITHUB_PERSONAL_ACCESS_TOKEN"],
    "mount_worktree": true
  }
}
```

- `image` is required and selects the container the server runs inside.
- `forward_env` is a list of host env var **names**; values are read by PaddleBoard at run time and passed via `podman run -e`, never serialized into the agent's context (same shape as the Sandbox Service Tool).
- `mount_worktree: true` (default) binds the active worktree at `/workspace` so filesystem-touching MCP servers work the way users expect; set to `false` for servers that shouldn't see your code.

The original `stdio` transport (which runs the binary directly on your host) is unchanged — sandboxing is opt-in per server.

---

### AI Dock

One place to browse and install everything the agent talks to. Think of it as the marina where every external collaborator your PaddleBoard talks to ties up.

- Open it from the command palette (`ai_dock: Open`) or the **Open the AI Dock** button on the Welcome screen. The Welcome screen also shows a small **Featured** strip of well-known agents (Claude, Codex, Copilot, Cursor, Antigravity) — clicking any pill opens the Dock so first-run users have something concrete to recognize.
- Five tabs: **Agents** (Zed, Claude, Codex, Copilot, Cursor, …), **Skills** (slash commands shipped with the project or installed in `~/.claude/commands/`), **Personas** (who the agent should *be* — see below), **MCP Servers** (the absorbed management page plus a catalog of common servers), and **Usage** (local, per-provider token stats — see below).
- Installed items show a green badge; missing ones show an **Install / Sign In / Set Up / Learn More** action that does the right thing for the category — registry agent installs are a one-click settings write, CLI-based agents (like Google ADK) show a **Set Up** button that opens a terminal with the install command, sign-in flows route to your existing identity, MCP server adds delegate to the existing setup machinery, and bundled skills (`/build`, `/update-tour`, `/clippy`, `/test`, `/check-drift`) install with **Add to project** / **Add to user** buttons that drop a markdown file into the right `.claude/commands/` directory.
- The catalog itself is `assets/ai_dock/catalog.json` in this repo — adding an entry is a PR, not a network fetch, so what shows up in the Dock is exactly what the team has reviewed.

The AI Dock replaces the old hardcoded 5-card "Agent Setup" row on the Welcome screen and the standalone MCP Servers pane — both routes now land here.

---

### Set Sail — deploy to serverless ⛵

Vibe-coded something worth showing off? **`set sail: Deploy`** (command palette, or click the **⛵ sailboat in the status bar**) gives you serverless-first deploys of the current project to **[Cloud Run](https://cloud.run), [AWS Lambda](https://aws.amazon.com/pm/lambda), or [Vercel](https://vercel.com)** — no YAML safari required.

- Pick a platform, a service name (pre-filled from your project), a region where it applies, and whether the URL should be public. Then the agent takes the helm.
- **Powered by [s8sskills](https://s8sskills.com):** PaddleBoard installs the platform's community skill pack (e.g. `gcloud-project-setup` + `cloud-run-deploy`, `aws-project-setup` + `lambda-deploy`, `vercel-project-setup` + `vercel-deploy`) into `.agents/skills/` and the agent follows that playbook — platform knowledge lives in the open-source catalog, not hardcoded in the editor.
- The agent checks your CLI setup first and hands interactive steps (`gcloud auth login`, `aws configure`, `vercel login`) to you in the terminal rather than running auth flows itself, then deploys and reports your live URL.
- More platforms (Azure, Cloudflare, Netlify, …) arrive as s8sskills packs — and a future "Rig the pipeline" mode will set up git-push-driven CD so quick deploys graduate into real infrastructure.

---

### Personas — tell the agent who to be

A **persona** is a markdown file describing who the agent should mimic — a Senior Developer, an SRE, a QA Engineer — and PaddleBoard injects it into the native agent's system prompt for the whole thread. Skills say what the agent can *do*; a persona says who it should *be*: its voice, values, and what it pushes back on.

- **Zero-config:** drop a `PERSONA.md` at your project root and new agent threads adopt it automatically. Plain prose works — frontmatter is optional.
- **Persona library:** keep several as `<name>.persona.md` files in `.claude/personas/` (per-project) or `~/.claude/personas/` (per-user), with `name:` and `description:` frontmatter.
- **Pick per thread:** a persona picker sits next to the profile selector in the agent panel. Switch or clear mid-thread; the change applies from the next message. The persona is saved with the thread, so it survives restarts.
- **Or just ask:** the agent knows your persona catalog and has an `adopt_persona` tool — say "be my QA tester" mid-conversation and it switches itself (and can drop the persona when you ask). It only changes personas at your request.
- **Compose with `extends:`** — a persona can inherit another's rules (`extends: house-base` in the frontmatter), so every role can build on a shared house style. Chains are followed; the child's own rules always read last and win.
- **Personas for sub-agents:** delegated work can wear its own identity — the agent can pass `persona` to its `spawn_agent` tool (or you can ask it to: "have a QA-persona sub-agent review this"), so a review pass genuinely thinks like a reviewer, not like the implementer grading their own work.
- **Starter personas:** the AI Dock's **Personas** tab ships three ready-made roles (Senior Developer, Site Reliability Engineer, QA Engineer) with one-click **Add to project / Add to user** install, and lists every persona discovered in your project.
- **Provider-agnostic:** the persona rides the system prompt, so it works identically with every configured LLM provider and stays byte-stable per thread for prompt-cache friendliness.
- Personas apply to the native PaddleBoard Agent (external agents like Claude Code own their system prompts). On external-agent threads, a muted persona icon links to the AI Dock's Personas tab so the feature stays discoverable.
- Turn the whole system off with `"paddleboard_personas": { "enabled": false }` in settings.

Writing tip: short, imperative behavioral rules ("ask for repro steps before proposing a fix") hold up far better over a long conversation than paragraphs of backstory.

---

### Usage tracking

If you mix multiple LLM/SLM providers, PaddleBoard can keep **local stats on how your token usage is distributed** across them — Anthropic, OpenAI, Gemini, Vertex, Bedrock, local SLMs (Ollama, LM Studio), and anything else you have configured. It complements the live status-bar context gauge: the gauge shows the *current* thread, this shows usage *over time*.

- **All local, all yours.** Nothing is reported anywhere. PaddleBoard writes one small **JSON file per day** (`<data_dir>/usage/YYYY-MM-DD.json`) — a text format on purpose, so the directory can live inside **your own private git repository** and produce clean diffs.
- **See it in the AI Dock → Usage tab.** Today / last-7-days / all-time totals up top, then a per-provider, per-model breakdown. An **Open Folder** button reveals the flatfiles; a refresh button re-reads them.
- **Accurate by construction.** Usage is recorded at the single point in the agent where every provider's billed token delta flows through (normal completions *and* context compaction), so providers are counted equally and nothing is double-counted.
- **Configure it** under `"paddleboard_usage"` in settings:
  - `enabled` (default `true`) — turn tracking on/off.
  - `granularity` — `"daily"` (one total per provider/model per day) or `"session"` (also broken down by agent session).
  - `directory` — where the files go; point it inside your backup repo (supports a leading `~`). Defaults to `<data_dir>/usage`.
  - `auto_commit` (default `false`) — when on, PaddleBoard runs `git add` + `git commit` in that directory after each flush. Off by default; when off, you commit and push the files yourself.

---

### Step-through mode

Step-through mode lets you approve every tool call before the agent executes it — useful when you want to watch exactly what the agent is doing or sanity-check a risky operation.

**How to enable it:** Click the step-over icon (⏭) in the agent thread toolbar. The button turns accent-colored when active.

When step mode is on, each tool call the agent wants to make pauses and shows two buttons in the normal permission UI:

- **Step** — execute this tool call and continue
- **Skip** — skip this tool call (the agent sees an empty result and moves on)

Step mode only applies to the root thread. Subagents spawned from that thread run without interruption.

---

### Agent orchestration panel

When you have multiple agents running — root threads, subagents spawned mid-task, background threads — it can be hard to track what's active. The Agent Threads panel gives you a live tree view of everything.

**How to open it:** Click the `ListTree` icon in the panel bar, or search for `orchestration_panel: Toggle Focus` in the command palette.

The panel shows:

- Every active agent session across all conversation views
- Subagents indented under the thread that spawned them
- A status indicator: accent-colored when generating, muted when idle
- Click any row to jump directly to that thread in the Agent Panel

The tree updates in real time as threads start, finish, or spawn subagents.

#### Scion agents

The Scion integration is **opt-in** — enable it with `"paddleboard_scion": { "enabled": true }` in your settings (installing the CLI alone won't activate it). Once enabled and [Scion](https://github.com/GoogleCloudPlatform/scion) is installed, a **Scion Agents** section appears below the native threads. Each agent shows a color-coded status icon and live activity badge (e.g., "executing · Edit", "thinking"). Right-click an agent row for:

- **View Logs** — opens a live-streaming log tab that tails `scion logs -f`. New lines appear in real time; the stream stops when you close the tab.
- **Sync Changes** — pulls the agent's worktree changes into your local project. Shows a toast on success and refreshes the agent list.
- **Stop Agent** — stops the selected agent.

Install Scion with `go install github.com/GoogleCloudPlatform/scion/cmd/scion@latest`, then use `scion: Start Agent` from the command palette to launch your first container-isolated agent.

**Delegating to Scion from an agent.** When the `scion` CLI is installed, agents gain a `spawn_scion_agent` tool. Instead of spawning an in-process sub-agent that shares your workspace, an agent can hand a well-scoped subtask to a Scion agent running in its own container and git worktree — true isolation for parallel work that writes to disk. The tool starts the agent, optionally waits for it to finish, returns the final phase plus a tail of its logs, and can `sync` the result back into your project when it completes.

#### OpenTelemetry tracing

PaddleBoard can export Scion agent lifecycle telemetry via **OpenTelemetry** to an external collector (Jaeger, Tempo, Grafana, etc.). When enabled, every poll cycle, CLI command, and agent state transition is captured as a trace span or event — useful for debugging multi-agent sessions or measuring agent efficiency.

**How to enable it:** Add `"paddleboard_otel": { "enabled": true }` to your `settings.json`, or set `PADDLEBOARD_OTEL_ENABLED=1` in your environment. Traces export via OTLP gRPC to `localhost:4317` by default.

Configuration options in `settings.json`:

```json
"paddleboard_otel": {
  "enabled": true,
  "endpoint": "http://localhost:4317",
  "protocol": "grpc",
  "service_name": "paddleboard"
}
```

- The `OTEL_EXPORTER_OTLP_ENDPOINT` and `OTEL_SERVICE_NAME` environment variables override their settings equivalents (standard OTEL convention).
- When OTEL is disabled (the default), the tracing instrumentation compiles to near-zero-cost checks — no overhead.

**What gets traced:**

- `scion.poll_cycle` — one span per 5-second poll interval
- `scion.list_agents`, `scion.start_agent`, `scion.stop_agent`, `scion.sync_from` — spans per CLI command with agent names, counts, and timing
- Phase transitions (e.g., `provisioning` → `running`) and activity transitions (e.g., `thinking` → `executing`) as trace events
- Agent discovery and disappearance events

**Quick start with Jaeger:**

```
docker run -d -p 16686:16686 -p 4317:4317 jaegertracing/all-in-one
```

Then launch PaddleBoard with `PADDLEBOARD_OTEL_ENABLED=1` and open `http://localhost:16686` to see traces.

---

### Multi-workspace

PaddleBoard lets you keep multiple projects in one window, each as its own workspace with its own pane tree and its own agent threads. Designed for running parallel agent sessions against different projects without window-juggling.

**How to use it:** Open the worktree picker via `git: Worktree`. From there you can:

- **Switch** to an existing worktree — PB persists which workspaces are open and reopens them on relaunch.
- **Create a new worktree** — provide a branch name (or accept the auto-generated one, like `dusty-pelican`) and PB sets up a git worktree alongside your repo, then opens it as a new workspace in the same window.
- **Open in new window** — for when you want the new worktree to live in its own window.

The worktree picker integrates with the agent orchestration panel: every workspace's agent threads show up in the live tree, so you can see what's running across all your projects at once.

---

### LLM provider picker panel

A dedicated panel for configuring and switching your active language model provider without opening settings. Dock it wherever is convenient and change providers as you work.

**ChatGPT Subscription auth.** Alongside the usual API-key providers, PaddleBoard includes a ChatGPT Subscription provider that authenticates via OAuth — sign in once with your ChatGPT Plus or Pro account and PB uses your subscription's OpenAI access, no API key needed. The OAuth flow opens in the embedded browser panel; tokens persist in PB's credential store.

**Vertex AI (Gemini Enterprise).** PaddleBoard adds a Google Vertex AI provider so you can run Gemini models through your own GCP project — the enterprise path alongside the consumer Gemini API. It reuses the Gemini request format, so it's a thin addition with no extra cloud SDK. **Configure it in the agent settings** (no `settings.json` editing required): open the Vertex provider, fill in **Project ID** and (optionally) **Location**, and **Save**. Three ways to authenticate, in precedence order:

- **gcloud (recommended — nothing stored):** run `gcloud auth login`, set a Project ID, leave the key fields empty. PaddleBoard borrows short-lived access tokens from the gcloud CLI (Application Default Credentials) — no key file or secret on disk.
- **Service account:** point the **key file** field at a service-account JSON; PaddleBoard mints OAuth tokens from it.
- **Express API key:** paste a Vertex Express key (stored in the keychain) for a quick start with no project setup.

Location defaults to `global`, where the newest models live (Gemini 3 and the `-latest` aliases); the curated default model list is confirmed-available there, and you can add region-specific ids under `available_models`. Pick a model that isn't published for your project/location and you'll get a clear message pointing you to `global` or `available_models` rather than a cryptic error.

---

### Git Login

Run **`git login: Manage`** from the command palette to save a Personal Access Token for **GitHub**, **GitLab**, **BitBucket**, or a custom host. Tokens are stored in your **OS keychain** (never in settings or plaintext). Once saved, git HTTPS `clone`/`fetch`/`push` authenticate silently — the password prompt only appears when there's no saved login.

- The modal lists each provider with its sign-in status at a glance — click a row to select it, or remove a saved login right from the list.
- It links straight to each provider's token page and shows the scopes to grant.
- Environment variables work as a fallback: set `GITHUB_TOKEN`, `GITLAB_TOKEN`, or `BITBUCKET_TOKEN` and git auth is answered from there.
- PaddleBoard sends the conventional token username per provider (`x-access-token` for GitHub, `oauth2` for GitLab, `x-token-auth` for BitBucket) unless you set your own.
- No saved login yet? When git's password prompt does appear, check **"Remember on this device"** before submitting and the credential is saved to your keychain — the next operation authenticates silently.
- Saved GitHub logins also authenticate PaddleBoard's GitHub API requests (commit-author avatars in blame), so private repos work and you skip the unauthenticated rate limit. `GITHUB_TOKEN` still wins when set.
- **Sign in with GitHub (browser):** builds configured with an OAuth client id show a one-click sign-in — PaddleBoard displays a short code, you approve it on github.com, and the token lands in your keychain automatically (the same device flow the `gh` CLI uses; no client secret involved). PATs always work regardless, and they're the path for GitLab/Bitbucket, which don't offer a device flow.
- SSH and host-key prompts are untouched (they still prompt as usual).

---

### Built-in language servers

PaddleBoard splits language support into two tiers so the default install stays lean and languages that need an external toolchain don't fail silently.

**Ready to use** — self-contained servers (Rust, TypeScript/JavaScript, Python, Go, JSON, YAML, HTML/CSS, Dockerfile, and more) are enabled by default. Open a matching file and the language server attaches automatically, downloading on first use and caching it. Dockerfiles get full syntax highlighting plus [docker-langserver](https://github.com/rcjsuen/dockerfile-language-server) (completion, hover, diagnostics) the moment you open a `Dockerfile` or `Containerfile`.

**Install support** — languages that aren't enabled by default. Run **`Manage Languages`** from the command palette to add them. Six ship a built-in server; clicking **Install** writes the setting (and downloads the server binary where one is needed) so it's ready before you open a file:

- **Java** via [jdtls](https://github.com/eclipse-jdtls/eclipse.jdt.ls) (Eclipse JDT Language Server) — requires JDK 17+
- **Kotlin** via [kotlin-language-server](https://github.com/fwcd/kotlin-language-server) — requires JDK 17+
- **PHP** via [intelephense](https://intelephense.com/) — requires Node
- **C#** via [roslyn](https://github.com/dotnet/roslyn) — requires .NET
- **C++** via [clangd](https://clangd.llvm.org/) — downloads the clangd binary (C stays enabled by default; clangd is shared)
- **Swift** via [SourceKit-LSP](https://github.com/swiftlang/sourcekit-lsp) — ships with the Swift toolchain (Xcode / swift.org); resolved from your PATH, not downloaded

Each row shows its prerequisite up front, so you opt into a heavier toolchain knowingly rather than hitting a confusing "server reset the connection" crash when the runtime is missing.

Two more — **Ruby** and **Dart** — get their language servers from extensions rather than a built-in adapter, so their row opens the Extensions page where you install the extension.

**Build tool context** — Java and Kotlin files detect Gradle (`build.gradle`, `build.gradle.kts`) and Maven (`pom.xml`) projects automatically. The `JAVA_BUILD_TOOL` and `JAVA_PROJECT_ROOT` task variables are available in task templates for build/test workflows.

**Prose checking** — Markdown files and git commit messages get spelling and grammar checking by default via [Harper](https://writewithharper.com) (`harper-ls`), an offline, privacy-first checker — no text leaves your machine. Misspellings and grammar slips show up as squiggles with quick-fix suggestions; the server downloads on first use, like the others. To keep a deliberate word (a name, an acronym, a coined term), put the cursor on it and open **Code Actions** (`cmd-.`) → **Add to dictionary**, and Harper won't flag it again. The default lint set is tuned to stay quiet on casual prose; re-enable the stricter style checks via `lsp.harper-ls.settings` if you want them.

### Search as you type

Project search runs automatically as you type — results update a beat after you stop, no Enter required (the way VSCode behaves, and one of upstream Zed's most-requested changes, [zed#9318](https://github.com/zed-industries/zed/issues/9318)). Prefer the classic press-Enter behavior? Turn it off in settings:

```json
"search": { "search_on_type": false }
```

---

### Agent context gauge

A status-bar meter shows how much of the model's context window the active agent thread has used — the percentage ticks up as the conversation grows, turning yellow as you approach the limit and red when you've hit it. Hover for the token breakdown (used / total, input vs. output); click to jump to the agent panel.

- Appears only while a thread has token usage; otherwise it stays out of your way.
- **Purely local.** It reads the counts the agent thread already tracks on your machine — PaddleBoard's telemetry stays hard-disabled, and nothing is reported anywhere.

---

## What PaddleBoard inherits from Zed

Everything else: multi-buffer editor, LSP, DAP debugger, git panel, terminal, Vim mode, remote development, extension system, collaborative editing, inline AI assistant, edit predictions, Jupyter notebooks, and the full GPUI-based GPU-accelerated rendering pipeline. See [Zed's documentation](https://zed.dev/docs) for details on anything not listed here.

---

## Quick-start tips

| What you want | How to do it |
|---|---|
| Open the browser | `Cmd-Shift-P` → `workspace: Open Browser` |
| Revisit this document | `Cmd-Shift-P` → `workspace: Open Paddle Board Tour` |
| Enable step-through mode | Click the ⏭ icon in the agent thread toolbar |
| See all agent threads | Click the list-tree icon in the panel bar |
| Switch LLM provider | Open the LLM Picker panel from the panel bar |
| Configure MCP servers | `Cmd-Shift-P` → `paddleboard: Mcp Servers` |
| Scaffold an ADK agent | `Cmd-Shift-P` → `adk: Scaffold Agent` |
| Run an ADK agent | `Cmd-Shift-P` → `adk: Run Agent` |
| Start a Scion agent | `Cmd-Shift-P` → `scion: Start Agent` |
| Enable OTEL tracing | Set `PADDLEBOARD_OTEL_ENABLED=1` or add `"paddleboard_otel": { "enabled": true }` to settings |
| Switch / create a worktree | `Cmd-Shift-P` → `git: Worktree` |
| Run code in a sandbox | Ask the agent to run a command — it uses the Sandbox Tool automatically |
| Run a service in a sandbox | Ask the agent to start a server (e.g. `python3 -m http.server 8000`) — it uses the Sandbox Service Tool, and the URL appears in the Forwarded Ports row of the browser panel |

---

PaddleBoard is a work in progress. If something breaks or behaves unexpectedly, check the logs (`paddleboard --help` for log location) and open an issue.
