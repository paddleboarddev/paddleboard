# PaddleBoard — Architecture Report

_Date: 2026-04-16_
_Scope: Overall architecture of the PaddleBoard repository_

## 1. What this repository is

PaddleBoard is a fork of the [Zed editor](https://zed.dev) by Jason "Jay" Smith. The fork was introduced in a single commit, `32b05c71a5 — v0.1: Initial PaddleBoard fork (Browser, Sandbox, Tour)` (2026-04-07), on top of recent upstream Zed `main`. All of the Zed codebase is still present — including its README, licenses (GPL/AGPL/Apache), docs site, and the entire 228-crate Cargo workspace — and PaddleBoard's changes sit on top as additive features plus branding.

At a glance:

- Language: Rust, `edition = "2024"`, `resolver = "2"`.
- Size: 228 member crates under `crates/`, plus three crates under `tooling/` (`compliance`, `perf`, `xtask`).
- Primary binary: `paddleboard` (`crates/zed/src/main.rs`, `default-run = "paddleboard"`). A second binary, `zed_visual_test_runner`, is gated behind the `visual-tests` feature.
- Version: the `zed` package is at `0.232.0`; the new `browser` crate is `0.1.0`.
- User-facing branding: the app identifier, config directory (`~/.config/paddleboard`), data directory (`PaddleBoard`), log file (`PaddleBoard.log`), per-project config directory (`.paddleboard/`), remote-server directories (`.paddleboard_server`, `.paddleboard_wsl_server`), and app icons have all been rebranded in `crates/paths/src/paths.rs` and `crates/zed/resources/`.

The repository also preserves Zed's infrastructure: `Dockerfile-*` files for Linux builds, `Procfile*` for local dev, `flake.nix`/`default.nix`/`shell.nix` for Nix, `script/` (109 entries) for build/CI helpers, `docs/` (mdBook), `extensions/` (first-party extensions), and `legal/`, `assets/`, `nix/`, `ci/`.

## 2. The Zed architectural foundation PaddleBoard inherits

PaddleBoard is structurally a layered, crate-per-concern Rust workspace, and the layering is dictated by `crates/zed` (the entry point) and `crates/gpui` (the foundation). The dependency graph flows roughly bottom-up through four tiers.

### 2.1 UI and runtime foundation

`crates/gpui` is the heart of the UI stack. It is a hybrid immediate/retained, GPU-accelerated UI framework where every piece of application state is an `Entity<T>` accessed through context types (`App`, `Context<T>`, `Window`, `AsyncApp`, `AsyncWindowContext`). All entity access and rendering happen on a single foreground thread; background work is dispatched explicitly via `cx.background_spawn(...)`.

Platform backends are split by OS:

- `gpui_macos` — Metal-based renderer, Cocoa/AppKit integration.
- `gpui_linux` — Linux renderer (X11/Wayland).
- `gpui_windows` — Windows renderer.
- `gpui_web` — early WebAssembly target.
- `gpui_wgpu`, `gpui_platform`, `gpui_tokio`, `gpui_util`, `gpui_macros` — shared wgpu renderer, the platform trait, a Tokio bridge, utilities, and proc-macro helpers.

Higher-level UI primitives live in `crates/ui` (1.1 MB of source, the component library), with theming in `crates/theme`, `crates/theme_settings`, `crates/theme_selector`, `crates/theme_extension`, and `crates/theme_importer`. `crates/icons` and `crates/file_icons` handle iconography.

Text primitives are factored out: `crates/rope`, `crates/sum_tree`, and `crates/text` provide the rope/tree data structures; `crates/multi_buffer` implements the multi-buffer abstraction on which the editor is built; `crates/buffer_diff` handles diffs; `crates/streaming_diff` is a streaming variant used for AI edit previews.

### 2.2 Editor, project, language

`crates/editor` is the single largest crate (~4.9 MB of source). It depends on `multi_buffer`, `language`, `project`, and `workspace`, and provides the actual code editing surface.

`crates/language` and `crates/language_core` hold language semantics: buffers, diagnostics, language registry, and tree-sitter wiring. `crates/languages` bundles first-party grammars/servers; `crates/grammars` aggregates tree-sitter grammars (~696 KB). `crates/lsp` is the LSP client; `crates/language_extension`, `crates/language_tools`, `crates/language_selector`, and `crates/language_onboarding` layer on top.

`crates/project` (~2.6 MB) is the model of "an open project" — worktrees, buffers, LSP clients, DAP clients, context servers, agent servers, terminals, settings, search, etc. `crates/worktree` is the filesystem abstraction; `crates/fs` is the lower-level filesystem trait (with `fs_benchmarks`); `crates/git` and `crates/git_ui`/`crates/git_graph`/`crates/git_hosting_providers` are the VCS layer; `crates/remote` + `crates/remote_connection` + `crates/remote_server` implement remote development; `crates/dev_container` implements devcontainer support.

`crates/workspace` (~1.6 MB) is the window/panes/tabs/docks/status-bar model. It owns global concepts like `AppState`, pane groups, items, and notifications. This is also where PaddleBoard adds its `OpenBrowser`, `OpenPaddleBoardTour`, and `TourStatusItem` (see §3).

`crates/settings`, `crates/settings_ui`, `crates/settings_content`, `crates/settings_json`, `crates/settings_macros`, and `crates/settings_profile_selector` form the settings system, backed by `crates/db`, `crates/sqlez`, and `crates/sqlez_macros` for persistence.

### 2.3 Agents, AI, and providers

PaddleBoard retains Zed's extensive AI stack. The agent runtime lives in:

- `crates/agent` (~3.9 MB) — the in-process agent, thread model, edit-agent, tool registry, evals.
- `crates/agent_ui` (~2.2 MB) — the Agent Panel and surrounding UI.
- `crates/agent_settings`, `crates/agent_servers` — settings and external agent server protocol support.
- `crates/acp_thread`, `crates/acp_tools` — Agent Client Protocol types and helpers.
- `crates/action_log` — per-session action log used by the agent.
- `crates/spawn_agent_tool`, and many other tools under `crates/agent/src/tools/` (grep, read/write/edit file, diagnostics, terminal, web search, fetch, plan updates, and PaddleBoard's `sandbox_tool`).
- `crates/context_server` — Model Context Protocol (MCP) client.
- `crates/prompt_store`, `crates/rules_library`, `crates/zeta_prompt` — prompt/rules assembly.

`crates/language_model` and `crates/language_models` implement the provider-agnostic LLM abstraction plus concrete provider integrations. Provider crates: `anthropic`, `bedrock`, `cloud_llm_client` / `cloud_api_client` / `cloud_api_types`, `codestral`, `copilot` + `copilot_chat` + `copilot_ui`, `deepseek`, `google_ai`, `lmstudio`, `mistral`, `ollama`, `open_ai`, `open_router`, `opencode`, `vercel`, `x_ai`. Edit prediction ("Zeta") is split across `edit_prediction`, `edit_prediction_cli`, `edit_prediction_context`, `edit_prediction_types`, and `edit_prediction_ui`. `crates/web_search` + `crates/web_search_providers` expose web search to tools.

### 2.4 Collaboration, extensions, surrounding systems

- Real-time collaboration: `call`, `channel`, `collab` (the server), `collab_ui`, `livekit_api`, `livekit_client`, `client`, `rpc`, `proto`, `notifications`, `feedback`.
- Extensions: `extension`, `extension_api`, `extension_host`, `extensions_ui`, `extension_cli`.
- Debug adapters: `dap`, `dap_adapters` (codelldb, gdb, go, javascript, python), `debug_adapter_extension`, `debugger_ui`, `debugger_tools`.
- Terminal & REPL: `terminal`, `terminal_view`, `repl`.
- Vim mode: `vim`, `vim_mode_setting`.
- Search / navigation: `search`, `fuzzy`, `file_finder`, `project_symbols`, `outline`, `outline_panel`, `go_to_line`, `tab_switcher`, `command_palette`, `command_palette_hooks`, `recent_projects`, `which_key`.
- Diagnostics & observability: `diagnostics`, `telemetry`, `telemetry_events`, `zlog`, `zlog_settings`, `crashes`, `miniprofiler_ui`, `ztracing`, `ztracing_macro`, `etw_tracing`, `system_specs`.
- Onboarding & previews: `onboarding`, `ai_onboarding`, `component`, `component_preview`, `story`, `storybook`, `csv_preview`, `svg_preview`, `markdown_preview`, `image_viewer`.
- Utilities: `util`, `util_macros`, `collections`, `time_format`, `paths`, `env_var`, `http_client`, `http_client_tls`, `aws_http_client`, `reqwest_client`, `net`, `node_runtime`, `watch`, `snippet`, `snippet_provider`, `snippets_ui`, `encoding_selector`, `line_ending_selector`, `refineable`, `denoise`, `audio`, `media`, `schema_generator`, `json_schema_store`, `html_to_markdown`, `shell_command_parser`, `scheduler`, `clock`, `session`, `menu`, `picker`, `sidebar`, `panel`, `platform_title_bar`, `title_bar`, `keymap_editor`, `migrator`, `release_channel`, `auto_update`, `auto_update_helper`, `auto_update_ui`, `install_cli`, `cli`, `explorer_command_injector`, `feature_flags`, `mac_only_instance`, `windows_only_instance`, `credentials_provider`, `zed_credentials_provider`, `zed_env_vars`, `zed_actions`.

## 3. What PaddleBoard adds on top of Zed

The `v0.1` fork commit is tightly scoped. Three new features and some branding:

### 3.1 Embedded native web browser

A new `crates/browser` (single file, `src/lib.rs`, 130 lines) defines:

- `Browser`, a focusable GPUI entity with a URL.
- `impl Item for Browser` so it shows up as a workspace pane item with a `Tool Web` tab icon and `"Browser: <url>"` tab label.
- A `BrowserElement` custom GPUI `Element` whose `prepaint` hook calls `window.update_webview(bounds)` each frame, pinning a native web view to the element's screen rectangle.
- A `pub fn init(cx: &mut App)` that, on each new `Workspace`, registers an `OpenBrowser` action handler that constructs a `Browser` entity hard-coded to `https://google.com`, calls `window.add_webview("https://google.com", ...)` to mount a `wry::WebView`, and adds the browser to the active pane.

The `browser` crate depends on `wry = "0.40.0"` (declared in `[workspace.dependencies]` alongside the `browser` path dependency) and uses `raw-window-handle` to attach the web view as a child of the GPUI window on macOS.

The native wiring is in `crates/gpui` and `crates/gpui_macos`:

- `crates/gpui/src/window.rs` gains `Window::add_webview(&mut self, url, bounds)` and `Window::update_webview(&mut self, bounds)`, which delegate to `platform_window`.
- `crates/gpui/src/platform.rs` defines default no-op implementations of `add_webview`/`update_webview` on the `PlatformWindow` trait, so non-macOS platforms compile but do nothing.
- `crates/gpui_macos/src/window.rs` holds an `Option<wry::WebView>` in window state (field `webview`, initialized to `None`); `add_webview` uses `raw_window_handle` to attach a `wry::WebViewBuilder::new_as_child` into the window, and `update_webview` calls `webview.set_bounds(...)` to match the element's current physical bounds.

The `tour.md` file advertises this feature as "Press `Cmd-Shift-P` and search for **`workspace: Open Browser`** to test it out!". See §5.1 for a functional issue with this.

### 3.2 Secure agent sandbox (Podman + gVisor)

`crates/agent/src/tools/sandbox_tool.rs` (280 lines) adds a new `SandboxTool` implementing `AgentTool` with `NAME = "sandbox_tool"` and `kind = acp::ToolKind::Execute`. Its input is:

```rust
pub struct SandboxToolInput {
    pub command: String,
    pub cd: String,
    pub image: Option<String>,     // defaults to "ubuntu:latest"
    pub timeout_ms: Option<u64>,
}
```

`run(...)` does the following, on the foreground thread:

1. Validates `cd` against the project's worktree absolute paths via string equality.
2. Asks the settings-driven permission layer (`decide_permission_from_settings`) whether to allow / deny / confirm the command; on `Confirm`, emits a permission request through the `ToolCallEventStream`.
3. Builds a single shell line:
   ```
   podman run --rm --runtime=runsc -v '<wd>:<wd>' -w '<wd>' <image> bash -c '<escaped command>'
   ```
4. Delegates execution to the existing agent terminal machinery (`ThreadEnvironment::create_terminal`, `terminal.wait_for_exit`, `terminal.kill`), supporting an optional timeout and user-initiated cancellation.
5. Formats the captured output, exit status, truncation, timeout, and cancellation state into a single string returned as the tool output.

The tool is registered during thread construction in `crates/agent/src/thread.rs` (`self.add_tool(SandboxTool::new(self.project.clone(), environment.clone()))`), alongside the pre-existing `TerminalTool`. The module is exposed via `crates/agent/src/tools.rs` (`mod sandbox_tool; pub use sandbox_tool::*;`).

### 3.3 PaddleBoard Tour

A lightweight welcome flow:

- `crates/workspace/src/tour.md` — 27-line markdown tour document embedded via `include_str!`.
- `crates/workspace/src/tour_status_item.rs` — a `TourStatusItem` status-bar widget rendering a `🏄‍♂️ Tour` button that dispatches the `OpenPaddleBoardTour` action.
- `crates/workspace/src/workspace.rs` — at the end of the file, two new action types are defined inside the `workspace` namespace: `OpenBrowser` and `OpenPaddleBoardTour`. The tour status item is instantiated in the workspace status bar (line 1697: `let tour_btn = cx.new(|cx| tour_status_item::TourStatusItem::new(cx));`). The `OpenPaddleBoardTour` handler (lines 779–796) writes `~/.config/paddleboard/PaddleBoard_Tour.md` (if missing) and `.tour_seen` as a marker, then opens the tour file as an ordinary editor item via `open_paths`.
- `crates/onboarding/src/onboarding.rs` and `crates/ai_onboarding/` are still the main onboarding flows; the tour is an additional entry point.

### 3.4 Branding & miscellaneous

- `crates/paths/src/paths.rs` — all Zed directories renamed to PaddleBoard equivalents (see §1).
- `crates/zed/Cargo.toml` — `default-run = "paddleboard"`, `[[bin]] name = "paddleboard"`, and a new dev bin metadata set, and `browser.workspace = true` added as a dependency.
- `crates/cli/src/main.rs` and `crates/install_cli/` — CLI rebranded.
- `crates/zed/src/zed/app_menus.rs` — menu items relabelled.
- Icons & images under `assets/` and `crates/zed/resources/` were replaced with PaddleBoard art; `docs/source/paddleboard_logo.svg` is new.
- `fix_cli_toast.pl` — a one-off Perl script at the repo root, apparently used once during the rebrand.

## 4. Runtime data flow (condensed)

A typical launch (from `crates/zed/src/main.rs`) follows this order, and understanding this ordering is the key to navigating the codebase:

1. Parse CLI flags (`clap`), set up panic/crash handlers (`reliability.rs`, `crashes::InitCrashHandler`), and decide whether to run as CLI vs. GUI.
2. Construct `gpui::Application` and enter the `run` callback.
3. Initialise global subsystems in a fixed order (lines ~470–760 of `main.rs`): `gpui_tokio`, `settings`, `zlog_settings`, `git_hosting_providers`, `extension`, then construct `Client`, `UserStore`, `WorkspaceStore`, `AppSession`, then `zed::init`, `debugger_ui`, `debugger_tools`, `dap_adapters`, `auto_update_ui`, `command_palette`, `language_model`, `acp_tools`, `edit_prediction_ui`, `web_search`, ... ending with `editor`, `image_viewer`, `repl::notebook`, `diagnostics`, `audio`, `ui_prompt`, `go_to_line`, `file_finder`, `tab_switcher`, `outline`, `project_symbols`, `project_panel`, `outline_panel`, `tasks_ui`, `snippets_ui`, `channel`, `search`, `vim`, `terminal_view`, `journal`, `git_ui`, `git_graph`, `feedback`, `markdown_preview`, `csv_preview`, `svg_preview`, `onboarding`, `settings_ui`, `keymap_editor`, `extensions_ui`, `edit_prediction`, `inspector_ui`, `json_schema_store`, `miniprofiler_ui`, `which_key`.
4. Register global observers on `SettingsStore` changes (keymap reloads, theme reloads, background appearance updates).
5. Restore prior workspace state via `restore_multiworkspace` and open any paths/URLs passed in.
6. Spin the GPUI event loop until quit.

Within a running workspace, user input becomes one of three things:

- A GPUI **action** dispatched via keymap or code; handlers are registered on elements or entities with `on_action` / `register_action`.
- An editor/project mutation via `Entity::update`, which may trigger LSP, DAP, buffer_diff, or agent tool calls.
- An AI interaction — the Agent Panel (`agent_ui::AgentPanel`) sends messages through a `Thread` (`crates/agent/src/thread.rs`); the `Thread` dispatches tool calls to registered `AgentTool` implementations (including PaddleBoard's `SandboxTool`), which in turn use `Project`, `ThreadEnvironment`, and the LLM provider abstractions.

## 5. Quick navigation reference

If you want to go straight to a specific area:

- App entry point → `crates/zed/src/main.rs` and `crates/zed/src/zed.rs`.
- Editor core → `crates/editor/src/`.
- Workspace model → `crates/workspace/src/workspace.rs`.
- Project / worktree / LSP / DAP → `crates/project/src/`.
- GPUI framework & docs → `crates/gpui/README.md`, `crates/gpui/src/_ownership_and_data_flow.rs`.
- Agent runtime → `crates/agent/src/agent.rs`, `crates/agent/src/thread.rs`, tools under `crates/agent/src/tools/`.
- Agent UI → `crates/agent_ui/src/`.
- LLM providers → `crates/language_models/src/provider/`.
- PaddleBoard browser → `crates/browser/src/lib.rs` + `crates/gpui_macos/src/window.rs` (webview).
- PaddleBoard sandbox tool → `crates/agent/src/tools/sandbox_tool.rs`.
- PaddleBoard tour → `crates/workspace/src/tour.md`, `crates/workspace/src/tour_status_item.rs`, and the two action handlers in `crates/workspace/src/workspace.rs`.
- Path/branding constants → `crates/paths/src/paths.rs`.
