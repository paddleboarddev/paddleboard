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

For now you opt in by hand-editing `settings.json`:

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

The original `stdio` transport (which runs the binary directly on your host) is unchanged — sandboxing is opt-in per server until the configure modal grows controls for it.

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

### LLM provider picker panel

A dedicated panel for configuring and switching your active language model provider without opening settings. Dock it wherever is convenient and change providers as you work.

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
| Run code in a sandbox | Ask the agent to run a command — it uses the Sandbox Tool automatically |
| Run a service in a sandbox | Ask the agent to start a server (e.g. `python3 -m http.server 8000`) — it uses the Sandbox Service Tool, and the URL appears in the Forwarded Ports row of the browser panel |

---

PaddleBoard is a work in progress. If something breaks or behaves unexpectedly, check the logs (`paddleboard --help` for log location) and open an issue.
