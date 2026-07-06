# PaddleBoard

An editor for AI-driven development. PaddleBoard is a fork of [Zed](https://github.com/zed-industries/zed) that keeps Zed's speed, GPU-accelerated rendering, LSP, debugger, terminal, and git workflow, then layers on the pieces a modern coding agent actually needs: a persona system that lets you define *who* your agent should be, one-command serverless deploys, sandboxed code execution, sandboxed MCP servers, an embedded browser, step-by-step tool approval, and a live view of every agent thread in the workspace.

> Status: alpha (0.1.x). Signed, notarized macOS builds (Apple Silicon) are available on the [Releases page](https://github.com/paddleboarddev/paddleboard/releases/latest) — or build from source (see below).

> ✅ **Signed & notarized.** macOS downloads are code-signed with an Apple Developer ID and notarized by Apple, so they open without a Gatekeeper warning. Currently macOS Apple Silicon only; other platforms build from source.

---

## What PaddleBoard does differently

Everything you rely on still works — and these additions are all reachable from the command palette or panel bar:

- **Personas — tell the agent who to be.** Drop a `PERSONA.md` at your project root describing who the agent should mimic — a Senior Developer, an SRE, a QA Engineer — and every new agent thread adopts that identity automatically: its voice, values, and what it pushes back on. Plain prose works; frontmatter is optional. Keep a library of `<name>.persona.md` files in `.claude/personas/` (project or `~/.claude/personas/` user-wide) and switch per thread with the picker in the agent panel — or just ask mid-conversation ("be my QA tester") and the agent switches itself via its `adopt_persona` tool. Three starter roles ship in the AI Dock's **Personas** tab with one-click install. Personas ride the system prompt, so they work identically with every LLM provider. Skills define what your agent can *do*; a persona defines who it should *be*.

- **Set Sail — serverless-first deploys.** `set sail: Deploy` (command palette, or the ⛵ in the status bar) quick-deploys the current project to **[Cloud Run](https://cloud.run), [AWS Lambda](https://aws.amazon.com/pm/lambda), or [Vercel](https://vercel.com)**: pick a platform, a service name, and public/private, and the agent takes the helm. Powered by the open-source [s8sskills](https://s8sskills.com) catalog — PaddleBoard installs the platform's skill pack into `.agents/skills/` and the agent follows that playbook, so platform knowledge lives in a versioned catalog rather than hardcoded in the editor. Interactive steps like `gcloud auth login` / `aws configure` / `vercel login` are handed to you in the terminal; the agent never runs auth flows itself. More platforms (Azure, Cloudflare, Netlify…) arrive as skill packs, and a future "Rig the pipeline" mode will set up git-push-driven CD.

- **Secure agent sandbox.** Tool calls that run code execute inside an ephemeral `ubuntu:latest` container via [Podman](https://podman.io/) + the [`runsc`](https://gvisor.dev/) (gVisor) kernel runtime. Your project is bind-mounted; the host filesystem is not exposed. Permissions still flow through Zed's existing approve / deny / always-allow UI. A status-bar shield surfaces the live prereq status, and missing prereqs are enforced — `paddleboard_sandbox.on_missing_runtime` chooses between `block` (default, opens the install modal), `fall_back_to_host`, or `warn_once`.

- **Forwarded ports.** Long-lived services (dev servers, `adk web`, demo apps) run in detached gVisor containers. PaddleBoard publishes the port on `127.0.0.1` only and shows a one-click link above the browser viewport. Stop the service with the × on the row — the container is discarded.

- **Embedded browser panel.** Native Chromium/WebKit panel that docks like any other. Used for forwarded service URLs and for the **Unsloth Studio** one-step launch (`workspace: Open Unsloth` spins up a Jupyter fine-tuning environment and navigates straight to it).

- **Sandboxed MCP servers.** A `sandboxed_stdio` context-server transport runs MCP servers inside Podman + gVisor instead of on your host. Stdin/stdout are proxied transparently so the JSON-RPC framing is unchanged. Opt-in per server; the original `stdio` transport still works for servers that don't need isolation. A dedicated **MCP Servers** settings page (command palette → `zed: Mcp Servers`) lists configured servers and surfaces status without hand-editing JSON.

- **Step-through mode.** Toggle the ⏭ icon in the agent thread toolbar to pause before every tool call. Step / Skip each one individually. Great for sanity-checking risky operations or watching an agent work.

- **Agent orchestration panel.** Live tree view of every active agent thread across the workspace, with subagents indented under whatever spawned them. Click a row to jump to that thread.

- **Scion integration.** Optional backend for running parallel agents in container-isolated worktrees via [Scion](https://github.com/GoogleCloudPlatform/scion). The orchestration panel polls the local Scion daemon and renders a **Scion Agents** section with per-agent status (phase, activity). Start agents from a modal with task description, name, and template selector; right-click any agent row for **View Logs** (opens in a read-only editor pane), **Sync Changes**, or **Stop Agent**. Agents can also delegate a subtask to an isolated Scion agent themselves via the `spawn_scion_agent` tool. The integration is **opt-in**: enable it with `"paddleboard_scion": { "enabled": true }` in settings (installing the CLI alone does not activate it), then install Scion with `go install github.com/GoogleCloudPlatform/scion/cmd/scion@latest`.

- **AI Dock.** Unified modal (command palette → `ai dock: Open`) for browsing, installing, and creating agents, skills, and MCP servers in one place. **Add Agent** registers a custom agent server by registry ID. **Create Skill** writes a new `.claude/commands/` markdown file with a name, prompt, and project/user scope selector. The catalog is `assets/ai_dock/catalog.json` — additions are PRs, not fetches.

- **Multi-workspace.** Hold multiple projects in one window, each as its own workspace with its own pane tree. Switch via the workspace picker; create worktree-backed workspaces with auto-generated branch names. Designed for running parallel agent sessions against different projects without window-juggling.

- **LLM provider picker panel + ChatGPT Subscription auth.** Switch providers from a dockable panel without opening settings. Includes a ChatGPT Subscription provider that authenticates via OAuth — sign in with your ChatGPT Plus or Pro account to use OpenAI models without managing an API key.

- **Git Login.** Save a Personal Access Token per provider (GitHub, GitLab, BitBucket, or a custom host) via `git login: Manage` — stored in your OS keychain, never in settings. Git HTTPS clone/fetch/push then authenticate silently instead of prompting every time; `GITHUB_TOKEN` / `GITLAB_TOKEN` / `BITBUCKET_TOKEN` also work as a fallback. GitHub additionally offers one-click **Sign in with GitHub** (OAuth device flow) out of the box; GitLab, BitBucket, and custom hosts are PAT-based for now.

- **Tiered language support.** A *Ready to use* tier (Rust, TypeScript/JavaScript, Python, Go, JSON, YAML, HTML/CSS, Dockerfile, and more) attaches automatically and downloads on first use — Dockerfiles (and Podman `Containerfile`s) get syntax highlighting plus `docker-langserver` out of the box. Languages that need an external toolchain — Java, Kotlin, PHP (via jdtls/kotlin-language-server/intelephense), C#, C++, and Swift — are opt-in: run `Manage Languages` from the command palette to enable a server in one click, with its prerequisite (JDK 17+, Node, .NET, or the Swift toolchain) shown up front so it never fails silently when the runtime is missing. (Swift's SourceKit-LSP ships with the platform toolchain and is resolved from PATH rather than downloaded.) See [WELCOME.md](./WELCOME.md#built-in-language-servers) for the full breakdown.

- **Offline prose checking.** Markdown files and git commit messages get spelling and grammar checking by default via [Harper](https://writewithharper.com) — privacy-first and fully local, so no text leaves your machine. Squiggles come with quick-fix suggestions; keep a deliberate word with `cmd-.` → Add to dictionary.

See [WELCOME.md](./WELCOME.md) for the deep-dive on each feature, including a worked example of building and running a [Google ADK](https://google.github.io/adk-docs/) agent end-to-end inside the sandbox.

We've also deliberately disabled some upstream behavior: telemetry is hard-disabled (events drop at `Telemetry::report_event`, settings toggles removed from onboarding) and Zed Pro trial upsells are inert.

## What's inherited from Zed

Everything else: multi-buffer editor, LSP, DAP debugger, git panel, terminal, Vim mode, remote development, extensions, collaborative editing, inline AI assistant, edit predictions, Jupyter notebooks, and Zed's GPUI rendering pipeline. Refer to [Zed's docs](https://zed.dev/docs) for anything in this category — the behavior is unchanged.

---

## Building

PaddleBoard builds with [Cargo](https://doc.rust-lang.org/cargo/), the same toolchain as upstream Zed.

1. **Install the prerequisites.** Follow [BUILDING.md](./BUILDING.md) — PaddleBoard's own guide, with macOS, Linux, and Windows sections (clone URL, system dependencies, and build commands).

   > **On Linux?** The build needs a bunch of system libraries (X11/XCB, fontconfig, glib, ALSA, …). To err on the side of caution, run `./script/linux` first — it auto-installs them via your package manager (apt, dnf, pacman, zypper, and more). Skipping it ends in `rust-lld: error: unable to find library` at the final link step.

2. **Run from source:**

   ```bash
   cargo run -p paddleboard
   ```

3. **Build a proper macOS app bundle** (optional) — produces `PaddleBoard.app` with the icon, dock name, and `paddleboard://` URL scheme. A plain `cargo build` only gives you `target/debug/paddleboard`, which macOS shows as a generic executable.

   ```bash
   ./script/bundle-mac -d -o    # debug .app (opens when done)
   ./script/bundle-mac -o       # release .app + .dmg (what the release pipeline ships)
   ```

Contributing? See [CONTRIBUTING.md](./CONTRIBUTING.md) for coding conventions and the rules around keeping fork divergence merge-friendly.

---

## Relationship to upstream Zed

A scheduled workflow merges `zed-industries/zed:main` into this repo every Monday. Conflicts open a draft PR for manual resolution; clean merges open a normal PR. Most fork-specific code lives in `paddleboard_*` crates so it never touches upstream files. Where we do edit a shared file, the change is tagged with `// PaddleBoard: <reason>` so future merge resolution stays mechanical.

## License

PaddleBoard inherits Zed's tri-license (AGPL / Apache 2.0 / GPL). See the `LICENSE-AGPL`, `LICENSE-APACHE`, and `LICENSE-GPL` files in this repo. Code derived from upstream Zed retains its original licensing.

## Credits

PaddleBoard builds on a lot of open-source work. See [CREDITS.md](./CREDITS.md) for the third-party language and prose servers it integrates (and their licenses), and `assets/licenses.md` for the full Rust dependency manifest.
