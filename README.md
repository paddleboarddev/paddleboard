# PaddleBoard

An editor for AI-driven development. PaddleBoard is a fork of [Zed](https://github.com/zed-industries/zed) that keeps Zed's speed, GPU-accelerated rendering, LSP, debugger, terminal, and git workflow, then layers on the pieces a modern coding agent actually needs: sandboxed code execution, sandboxed MCP servers, an embedded browser, step-by-step tool approval, and a live view of every agent thread in the workspace.

> Status: alpha. Build from source — there are no released binaries yet.

---

## What's different from Zed

Every Zed feature still works. The additions, all reachable from the command palette or panel bar:

- **Secure agent sandbox.** Tool calls that run code execute inside an ephemeral `ubuntu:latest` container via [Podman](https://podman.io/) + the [`runsc`](https://gvisor.dev/) (gVisor) kernel runtime. Your project is bind-mounted; the host filesystem is not exposed. Permissions still flow through Zed's existing approve / deny / always-allow UI. A status-bar shield surfaces the live prereq status, and missing prereqs are enforced — `paddleboard_sandbox.on_missing_runtime` chooses between `block` (default, opens the install modal), `fall_back_to_host`, or `warn_once`.
- **Forwarded ports.** Long-lived services (dev servers, `adk web`, demo apps) run in detached gVisor containers. PaddleBoard publishes the port on `127.0.0.1` only and shows a one-click link above the browser viewport. Stop the service with the × on the row — the container is discarded.
- **Embedded browser panel.** Native Chromium/WebKit panel that docks like any other. Used for forwarded service URLs and for the **Unsloth Studio** one-step launch (`workspace: Open Unsloth` spins up a Jupyter fine-tuning environment and navigates straight to it).
- **Sandboxed MCP servers.** A `sandboxed_stdio` context-server transport runs MCP servers inside Podman + gVisor instead of on your host. Stdin/stdout are proxied transparently so the JSON-RPC framing is unchanged. Opt-in per server; the original `stdio` transport still works for servers that don't need isolation.
- **Step-through mode.** Toggle the ⏭ icon in the agent thread toolbar to pause before every tool call. Step / Skip each one individually. Great for sanity-checking risky operations or watching an agent work.
- **Agent orchestration panel.** Live tree view of every active agent thread across the workspace, with subagents indented under whatever spawned them. Click a row to jump to that thread.
- **LLM provider picker panel.** Switch providers from a dockable panel without opening settings.

See [WELCOME.md](./WELCOME.md) for the deep-dive on each feature, including a worked example of building and running a [Google ADK](https://google.github.io/adk-docs/) agent end-to-end inside the sandbox.

We've also deliberately disabled some upstream behavior: telemetry is hard-disabled (events drop at `Telemetry::report_event`, settings toggles removed from onboarding) and Zed Pro trial upsells are inert. See [FORK_HYGIENE.md](./FORK_HYGIENE.md) for the policy.

## What's inherited from Zed

Everything else: multi-buffer editor, LSP, DAP debugger, git panel, terminal, Vim mode, remote development, extensions, collaborative editing, inline AI assistant, edit predictions, Jupyter notebooks, and Zed's GPUI rendering pipeline. Refer to [Zed's docs](https://zed.dev/docs) for anything in this category — the behavior is unchanged.

---

## Building

The upstream build process is unmodified:

- [Building on macOS](./docs/src/development/macos.md)
- [Building on Linux](./docs/src/development/linux.md)
- [Building on Windows](./docs/src/development/windows.md)

Substitute `paddleboard` for `zed` when invoking cargo — for example, `cargo run -p paddleboard` instead of `cargo run -p zed`.

For agent contributors, see [`CLAUDE.md`](./CLAUDE.md) for project-specific coding guidelines and [`FORK_HYGIENE.md`](./FORK_HYGIENE.md) for the rules around keeping fork divergence merge-friendly.

---

## Relationship to upstream Zed

A scheduled workflow merges `zed-industries/zed:main` into this repo every Monday. Conflicts open a draft PR for manual resolution; clean merges open a normal PR. Most fork-specific code lives in `paddleboard_*` crates so it never touches upstream files. Where we do edit a shared file, the change is tagged with `// PaddleBoard: <reason>` so future merge resolution stays mechanical. Full policy in [FORK_HYGIENE.md](./FORK_HYGIENE.md).

## License

PaddleBoard inherits Zed's tri-license (AGPL / Apache 2.0 / GPL). See the `LICENSE-AGPL`, `LICENSE-APACHE`, and `LICENSE-GPL` files in this repo. Code derived from upstream Zed retains its original licensing.
