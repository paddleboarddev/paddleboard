# PaddleBoard

An editor for AI-driven development. PaddleBoard is a fork of [Zed](https://github.com/zed-industries/zed) that keeps Zed's speed, GPU-accelerated rendering, LSP, debugger, terminal, and git workflow, then layers on the pieces a modern coding agent actually needs: sandboxed code execution, sandboxed MCP servers, an embedded browser, step-by-step tool approval, and a live view of every agent thread in the workspace.

> Status: alpha. Build from source — there are no released binaries yet.

> ⚠️ **Not yet notarized by Apple.** PaddleBoard is not currently code-signed or notarized with an Apple Developer ID. Once binaries are distributed, macOS Gatekeeper will warn that "Apple cannot check it for malicious software" until notarization is set up — you'd need to allow it via **System Settings → Privacy & Security**. Building and running from source yourself is unaffected.

---

## What's different from Zed

Every Zed feature still works. The additions, all reachable from the command palette or panel bar:

- **Secure agent sandbox.** Tool calls that run code execute inside an ephemeral `ubuntu:latest` container via [Podman](https://podman.io/) + the [`runsc`](https://gvisor.dev/) (gVisor) kernel runtime. Your project is bind-mounted; the host filesystem is not exposed. Permissions still flow through Zed's existing approve / deny / always-allow UI. A status-bar shield surfaces the live prereq status, and missing prereqs are enforced — `paddleboard_sandbox.on_missing_runtime` chooses between `block` (default, opens the install modal), `fall_back_to_host`, or `warn_once`.
- **Forwarded ports.** Long-lived services (dev servers, `adk web`, demo apps) run in detached gVisor containers. PaddleBoard publishes the port on `127.0.0.1` only and shows a one-click link above the browser viewport. Stop the service with the × on the row — the container is discarded.
- **Embedded browser panel.** Native Chromium/WebKit panel that docks like any other. Used for forwarded service URLs and for the **Unsloth Studio** one-step launch (`workspace: Open Unsloth` spins up a Jupyter fine-tuning environment and navigates straight to it).
- **Sandboxed MCP servers.** A `sandboxed_stdio` context-server transport runs MCP servers inside Podman + gVisor instead of on your host. Stdin/stdout are proxied transparently so the JSON-RPC framing is unchanged. Opt-in per server; the original `stdio` transport still works for servers that don't need isolation. A dedicated **MCP Servers** settings page (command palette → `zed: Mcp Servers`) lists configured servers and surfaces status without hand-editing JSON.
- **Step-through mode.** Toggle the ⏭ icon in the agent thread toolbar to pause before every tool call. Step / Skip each one individually. Great for sanity-checking risky operations or watching an agent work.
- **Agent orchestration panel.** Live tree view of every active agent thread across the workspace, with subagents indented under whatever spawned them. Click a row to jump to that thread.
- **Scion integration.** Optional backend for running parallel agents in container-isolated worktrees via [Scion](https://github.com/GoogleCloudPlatform/scion). The orchestration panel polls the local Scion daemon and renders a **Scion Agents** section with per-agent status (phase, activity). Start agents from a modal with task description, name, and template selector; right-click any agent row for **View Logs** (opens in a read-only editor pane), **Sync Changes**, or **Stop Agent**. Agents can also delegate a subtask to an isolated Scion agent themselves via the `spawn_scion_agent` tool (available whenever the `scion` CLI is on `PATH`). Install Scion with `go install github.com/GoogleCloudPlatform/scion/cmd/scion@latest`.
- **AI Dock.** Unified modal (command palette → `ai dock: Open`) for browsing, installing, and creating agents, skills, and MCP servers in one place. **Add Agent** registers a custom agent server by registry ID. **Create Skill** writes a new `.claude/commands/` markdown file with a name, prompt, and project/user scope selector. The catalog is `assets/ai_dock/catalog.json` — additions are PRs, not fetches.
- **Multi-workspace.** Hold multiple projects in one window, each as its own workspace with its own pane tree. Switch via the workspace picker; create worktree-backed workspaces with auto-generated branch names. Designed for running parallel agent sessions against different projects without window-juggling.
- **LLM provider picker panel + ChatGPT Subscription auth.** Switch providers from a dockable panel without opening settings. Includes a ChatGPT Subscription provider that authenticates via OAuth — sign in with your ChatGPT Plus or Pro account to use OpenAI models without managing an API key.
- **Tiered language support.** A *Ready to use* tier (Rust, TypeScript/JavaScript, Python, Go, JSON, YAML, HTML/CSS, and more) attaches automatically and downloads on first use. Languages that need an external toolchain — Java, Kotlin, PHP (via jdtls/kotlin-language-server/intelephense), C#, and C++ — are opt-in: run `Manage Languages` from the command palette to install a server in one click, with its prerequisite (JDK 17+, Node, .NET) shown up front so it never fails silently when the runtime is missing. Swift uses SourceKit-LSP from the platform toolchain. See [WELCOME.md](./WELCOME.md#built-in-language-servers) for the full breakdown.

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
