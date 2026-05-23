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
- Click the × → the container is stopped and the entry disappears
- The agent can wait for a readiness substring in the container logs before reporting success, so the URL it gives you is actually live

For one-shot commands (builds, tests, scripts) the agent still uses the regular Sandbox Tool. The Service Tool is for processes that should outlive the tool call.

#### Recipe — build & run an ADK agent

Google's [Agent Development Kit](https://google.github.io/adk-docs/) ships a `adk web` UI that's a natural fit for the sandbox service flow. PaddleBoard can scaffold and run it for you without leaving the editor.

1. Export your model credential in the shell you launch PaddleBoard from — e.g. `export GOOGLE_API_KEY=...`. The value stays in your shell; PaddleBoard never copies it into the agent's context.
2. In a chat thread, say: **"Scaffold a starter ADK agent in this worktree and run `adk web` in the sandbox, forwarding GOOGLE_API_KEY."**
3. The agent will write `agent.py` and `requirements.txt`, then call `sandbox_service_tool` with something like:
   - `image: "python:3.12-slim"`
   - `command: "pip install -r requirements.txt && adk web --host 0.0.0.0 --port 8000"`
   - `port: 8000`
   - `forward_env: ["GOOGLE_API_KEY"]`
4. A Forwarded Ports row appears with the host port; click it to open the ADK UI in the browser panel.

The `forward_env` field accepts a list of host env var **names** only — values are read by the tool at run time and passed to the container via `podman run -e`, never serialized into the conversation.

---

### Sandboxed MCP servers

Most editors run **MCP (Model Context Protocol) servers** directly on your host. That means an MCP server has the same filesystem reach, network access, and credentials as you do — which doesn't match PaddleBoard's "everything the agent touches goes through Podman + gVisor" pitch.

PaddleBoard adds a fourth context-server transport, `sandboxed_stdio`, that runs the MCP server inside a `podman run -i --rm --runtime=runsc` container. Stdin and stdout are proxied transparently, so the JSON-RPC framing keeps working without any change on the agent side.

**Manage servers in the AI Dock** — `paddleboard: Mcp Servers` (or `ai_dock: Open` then the **MCP Servers** tab) opens the PaddleBoard AI Dock with the absorbed server view. You get the full add/filter (All / Running / Stopped / Error) / inspect surface plus a side-by-side **Available** catalog of well-known servers without hand-editing JSON.

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

- Open it from the command palette (`ai_dock: Open`) or the **Open the AI Dock** button on the Welcome screen. The Welcome screen also shows a small **Featured** strip of well-known agents (Claude, Codex, Copilot, Cursor, Gemini) — clicking any pill opens the Dock so first-run users have something concrete to recognize.
- Three tabs: **Agents** (Zed, Claude, Codex, Copilot, Cursor, …), **Skills** (slash commands shipped with the project or installed in `~/.claude/commands/`), and **MCP Servers** (the absorbed management page plus a catalog of common servers).
- Installed items show a green badge; missing ones show an **Install / Sign In / Learn More** action that does the right thing for the category — agent installs are a one-click settings write, sign-in flows route to your existing identity, MCP server adds delegate to the existing setup machinery, and bundled skills (currently `/build` and `/update-tour`) install with **Add to project** / **Add to user** buttons that drop a markdown file into the right `.claude/commands/` directory.
- The catalog itself is `assets/ai_dock/catalog.json` in this repo — adding an entry is a PR, not a network fetch, so what shows up in the Dock is exactly what the team has reviewed.

The AI Dock replaces the old hardcoded 5-card "Agent Setup" row on the Welcome screen and the standalone MCP Servers pane — both routes now land here.

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

---

### Built-in language servers

PaddleBoard ships with built-in support for four languages that Zed historically punts to extensions:

- **Java** via [jdtls](https://github.com/eclipse/eclipse.jdt.ls) (Eclipse JDT Language Server)
- **Kotlin** via [kotlin-language-server](https://github.com/fwcd/kotlin-language-server)
- **PHP** via [intelephense](https://intelephense.com/)
- **Swift** via [SourceKit-LSP](https://github.com/swiftlang/sourcekit-lsp)

Open a `.java`/`.kt`/`.php`/`.swift` file and the LSP attaches automatically — no extension installation required. The first time you open a file in one of these languages, PB downloads the appropriate language server binary and caches it for subsequent sessions.

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
| Switch / create a worktree | `Cmd-Shift-P` → `git: Worktree` |
| Run code in a sandbox | Ask the agent to run a command — it uses the Sandbox Tool automatically |
| Run a service in a sandbox | Ask the agent to start a server (e.g. `python3 -m http.server 8000`) — it uses the Sandbox Service Tool, and the URL appears in the Forwarded Ports row of the browser panel |

---

PaddleBoard is a work in progress. If something breaks or behaves unexpectedly, check the logs (`paddleboard --help` for log location) and open an issue.
