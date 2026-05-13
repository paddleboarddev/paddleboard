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

### 3. Forwarded Ports — Sandbox Services
Long-lived processes (dev servers, demo apps, `adk web`) use the **Sandbox Service Tool**. Each running service appears in a **Forwarded Ports** row above the browser viewport.
- Click the label (e.g. `http :54321`) → the browser panel navigates to `http://localhost:54321`.
- Click the × → the container stops and the entry disappears.
- Bindings stay on `127.0.0.1` only — never your LAN.

### 4. Sandboxed MCP Servers
PaddleBoard runs your **MCP servers** inside the same Podman + gVisor sandbox as the Sandbox Tool.
- Use `"source": "sandboxed_stdio"` in `settings.json` instead of plain `"stdio"`.
- Forward only the host env vars you need by name — values stay out of the agent's context.
- The worktree is mounted at `/workspace` so filesystem-touching servers (git, fs, etc.) still work.

### 5. Step-Through Mode
Approve every tool call before the agent executes it.
- Click the **⏭** icon in the agent thread toolbar to enable (it turns accent-colored).
- Each tool call pauses with **Step** (run it) or **Skip** (return empty and move on).
- Only the root thread is gated — subagents run without interruption.

### 6. Agent Orchestration Panel
A live tree view of every active agent session, including subagents.
- Open it: panel bar `ListTree` icon, or `Cmd-Shift-P` → **`orchestration_panel: Toggle Focus`**.
- Subagents nest under the thread that spawned them.
- Status dot shows generating vs. idle; click any row to jump to that thread.

### 7. LLM Provider Picker Panel
A dedicated panel for switching the active language model provider without opening settings. Dock it wherever is convenient and change providers as you work.

---

*You can always revisit this tour by opening the Command Palette (`Cmd-Shift-P`) and selecting **`workspace: Open Paddle Board Tour`**.*
