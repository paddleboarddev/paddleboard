use agent_ui::AgentPanel;
use gpui::{DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Render, WeakEntity};
use ui::{Button, KeyBinding, Modal, ModalFooter, ModalHeader, prelude::*};
use ui_input::InputField;
use workspace::{ModalView, Workspace};

/// "Build an MCP" — collects a service description and hands it to the agent,
/// which researches the service's API, writes an MCP server, tests it in the
/// sandbox, and installs it into the AI Dock.
pub struct BuildMcpModal {
    workspace: WeakEntity<Workspace>,
    service_input: Entity<InputField>,
    docs_input: Entity<InputField>,
    auth_input: Entity<InputField>,
    description_input: Entity<InputField>,
    focus_handle: FocusHandle,
}

impl BuildMcpModal {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let service_input = cx.new(|cx| {
            InputField::new(window, cx, "Substack")
                .label("Service")
                .tab_index(1)
                .tab_stop(true)
        });
        let docs_input = cx.new(|cx| {
            InputField::new(window, cx, "https://… API docs (optional)")
                .label("API docs URL")
                .tab_index(2)
                .tab_stop(true)
        });
        let auth_input = cx.new(|cx| {
            InputField::new(window, cx, "SUBSTACK_API_KEY (optional)")
                .label("Auth env var")
                .tab_index(3)
                .tab_stop(true)
        });
        let description_input = cx.new(|cx| {
            InputField::new(window, cx, "List my latest posts and subscriber count…")
                .label("What it should do")
                .tab_index(4)
                .tab_stop(true)
        });

        Self {
            workspace,
            service_input,
            docs_input,
            auth_input,
            description_input,
            focus_handle: cx.focus_handle(),
        }
    }

    fn confirm(&mut self, _: &menu::Confirm, window: &mut Window, cx: &mut Context<Self>) {
        let service = self.service_input.read(cx).text(cx).trim().to_string();
        let description = self.description_input.read(cx).text(cx).trim().to_string();
        if service.is_empty() || description.is_empty() {
            return;
        }
        let docs = self.docs_input.read(cx).text(cx).trim().to_string();
        let auth = self.auth_input.read(cx).text(cx).trim().to_string();

        let prompt = build_mcp_prompt(&service, &docs, &auth, &description);
        let title: SharedString = format!("Build an MCP: {service}").into();

        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |workspace, cx| {
                let Some(panel) = workspace.panel::<AgentPanel>(cx) else {
                    workspace.show_error(
                        anyhow::anyhow!("Open the Agent panel to build an MCP server."),
                        cx,
                    );
                    return;
                };
                workspace.focus_panel::<AgentPanel>(window, cx);
                panel.update(cx, |panel, cx| {
                    // PaddleBoard: force the native agent so the `install_mcp_server`
                    // tool is available regardless of the panel's selected agent.
                    panel.seed_prompt_thread(title.clone(), prompt.clone(), true, window, cx);
                });
            });
        }

        cx.emit(DismissEvent);
    }

    fn cancel(&mut self, _: &menu::Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }
}

/// Self-contained codegen prompt. Inlines the full procedure so the agent can
/// run it even if the `/build-mcp` skill isn't installed. The thread runs on the
/// native agent (forced by the caller) so `install_mcp_server` is always present;
/// that tool persists the server to the data dir and registers a plain Stdio
/// entry — secrets are never written into settings, only read from the env.
fn build_mcp_prompt(service: &str, docs: &str, auth: &str, description: &str) -> String {
    let mut prompt = String::new();
    prompt.push_str("Build and install an MCP server for me (the `/build-mcp` skill, if available, has the full playbook).\n\n");
    prompt.push_str(&format!("Service: {service}\n"));
    if !docs.is_empty() {
        prompt.push_str(&format!("API docs: {docs}\n"));
    }
    if !auth.is_empty() {
        prompt.push_str(&format!("Auth env var: {auth}\n"));
    }
    prompt.push_str(&format!("What it should do: {description}\n\n"));
    prompt.push_str(
        "Steps:\n\
         1. Research the service's API (fetch the docs URL above, or search for it). Identify the base URL, auth scheme, and the few endpoints needed.\n\
         2. Scaffold a Python MCP server (the `mcp`/FastMCP SDK): a `server.py` and `requirements.txt`. Build/test it in your working directory; the install step (5) persists it to the PaddleBoard data dir for you, so don't worry about host paths here.\n\
         3. Implement one tool per endpoint. ",
    );
    if auth.is_empty() {
        prompt.push_str("If the API needs a key, read it from an environment variable (never hardcode secrets).\n");
    } else {
        prompt.push_str(&format!("Read the API key from `os.environ[\"{auth}\"]` — never hardcode it.\n"));
    }
    prompt.push_str(
        "4. Optionally test it in the sandbox (run it and confirm it starts and registers its tools).\n\
         5. Install it by calling the `install_mcp_server` tool with the server id and the final file contents (server.py, requirements.txt). Do NOT edit settings.json yourself — you build inside the sandbox and can't reach host paths; the tool persists the server and registers it for you so the AI Dock launches it.\n\
         6. Report the server id, the tools it exposes, and remind me to export any required env var in the shell that launches PaddleBoard. Then I can use it from a new agent thread.",
    );
    prompt
}

impl EventEmitter<DismissEvent> for BuildMcpModal {}

impl Focusable for BuildMcpModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ModalView for BuildMcpModal {}

impl Render for BuildMcpModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.focus_handle(cx);
        let service_empty = self.service_input.read(cx).is_empty(cx);
        let description_empty = self.description_input.read(cx).is_empty(cx);

        v_flex()
            .id("build-mcp-modal")
            .key_context("BuildMcpModal")
            .w(rems(34.))
            .elevation_3(cx)
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::cancel))
            .capture_any_mouse_down(cx.listener(|this, _, window, cx| {
                this.focus_handle(cx).focus(window, cx);
            }))
            .child(
                Modal::new("build-mcp", None)
                    .header(
                        ModalHeader::new().headline("Build an MCP").description(
                            "Describe a service and PaddleBoard's agent will research its API, build an MCP server, test it in the sandbox, and install it.",
                        ),
                    )
                    .child(
                        v_flex()
                            .px_3()
                            .pb_2()
                            .gap_2()
                            .child(self.service_input.clone())
                            .child(self.docs_input.clone())
                            .child(self.auth_input.clone())
                            .child(self.description_input.clone()),
                    )
                    .footer(
                        ModalFooter::new().end_slot(
                            h_flex()
                                .gap_1()
                                .child(
                                    Button::new("cancel", "Cancel")
                                        .key_binding(
                                            KeyBinding::for_action_in(
                                                &menu::Cancel,
                                                &focus_handle,
                                                cx,
                                            )
                                            .map(|kb| kb.size(rems_from_px(12.))),
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.cancel(&menu::Cancel, window, cx);
                                        })),
                                )
                                .child(
                                    Button::new("build-mcp-confirm", "Build")
                                        .style(ButtonStyle::Filled)
                                        .key_binding(
                                            KeyBinding::for_action_in(
                                                &menu::Confirm,
                                                &focus_handle,
                                                cx,
                                            )
                                            .map(|kb| kb.size(rems_from_px(12.))),
                                        )
                                        .disabled(service_empty || description_empty)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.confirm(&menu::Confirm, window, cx);
                                        })),
                                ),
                        ),
                    ),
            )
    }
}
