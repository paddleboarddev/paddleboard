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

---

PaddleBoard is a work in progress. If something breaks or behaves unexpectedly, check the logs (`paddleboard --help` for log location) and open an issue.
