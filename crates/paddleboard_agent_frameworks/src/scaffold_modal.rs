use gpui::{DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, WeakEntity};
use ui::{
    Button, KeyBinding, Label, LabelSize, Modal, ModalFooter, ModalHeader, Section, prelude::*,
};
use ui_input::InputField;
use workspace::{ModalView, Workspace};

struct ScaffoldConfig {
    headline: &'static str,
    description: &'static str,
    hint: &'static str,
    command: &'static str,
    subcommand: &'static str,
    task_id_prefix: &'static str,
}

const ADK_CONFIG: ScaffoldConfig = ScaffoldConfig {
    headline: "Create ADK Agent",
    description: "Scaffold a new Google ADK agent project in this workspace.",
    hint: "Runs `adk create <name>` in a terminal to set up the project structure.",
    command: "adk",
    subcommand: "create",
    task_id_prefix: "adk-scaffold",
};

const LANGGRAPH_CONFIG: ScaffoldConfig = ScaffoldConfig {
    headline: "Create LangGraph Agent",
    description: "Scaffold a new LangGraph agent project in this workspace.",
    hint: "Runs `langgraph new <name>` in a terminal to set up the project structure.",
    command: "langgraph",
    subcommand: "new",
    task_id_prefix: "langgraph-scaffold",
};

fn config_for(framework: &str) -> &'static ScaffoldConfig {
    match framework {
        "langgraph" => &LANGGRAPH_CONFIG,
        _ => &ADK_CONFIG,
    }
}

pub struct ScaffoldAgentModal {
    workspace: WeakEntity<Workspace>,
    name_input: Entity<InputField>,
    focus_handle: FocusHandle,
    config: &'static ScaffoldConfig,
}

impl ScaffoldAgentModal {
    pub fn toggle(
        workspace: &mut Workspace,
        framework: &'static str,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let weak_workspace = cx.entity().downgrade();
        workspace.toggle_modal(window, cx, |window, cx| {
            Self::new(weak_workspace, framework, window, cx)
        });
    }

    fn new(
        workspace: WeakEntity<Workspace>,
        framework: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name_input = cx.new(|cx| {
            InputField::new(window, cx, "my-agent")
                .label("Agent Name")
                .tab_index(1)
                .tab_stop(true)
        });

        Self {
            workspace,
            name_input,
            focus_handle: cx.focus_handle(),
            config: config_for(framework),
        }
    }

    fn confirm(&mut self, _: &menu::Confirm, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name_input.read(cx).text(cx);
        let name = name.trim().to_string();

        if name.is_empty() {
            return;
        }

        let config = self.config;
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |workspace, cx| {
                crate::spawn_in_terminal(
                    workspace,
                    window,
                    cx,
                    config.task_id_prefix,
                    &format!("{} Create: {name}", config.headline.replace("Create ", "")),
                    config.command,
                    vec![config.subcommand.to_string(), name],
                );
            });
        }

        cx.emit(DismissEvent);
    }

    fn cancel(&mut self, _: &menu::Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }
}

impl EventEmitter<DismissEvent> for ScaffoldAgentModal {}

impl Focusable for ScaffoldAgentModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ModalView for ScaffoldAgentModal {}

impl Render for ScaffoldAgentModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let confirm_enabled = !self.name_input.read(cx).text(cx).trim().is_empty();
        let config = self.config;

        v_flex()
            .key_context("ScaffoldAgentModal")
            .track_focus(&self.focus_handle(cx))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::cancel))
            .w(rems(34.))
            .child(
                Modal::new("scaffold-agent-modal", None)
                    .header(ModalHeader::new().headline(config.headline))
                    .section(
                        Section::new()
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        Label::new(config.description)
                                            .size(LabelSize::Small)
                                            .color(Color::Muted),
                                    )
                                    .child(
                                        Label::new(config.hint)
                                            .size(LabelSize::XSmall)
                                            .color(Color::Muted),
                                    ),
                            )
                            .child(self.name_input.clone()),
                    )
                    .footer(
                        ModalFooter::new().end_slot(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new("cancel", "Cancel")
                                        .style(ButtonStyle::Transparent)
                                        .key_binding(KeyBinding::for_action(
                                            &menu::Cancel,
                                            cx,
                                        ))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.cancel(&menu::Cancel, window, cx);
                                        })),
                                )
                                .child(
                                    Button::new("create", "Create")
                                        .style(ButtonStyle::Filled)
                                        .disabled(!confirm_enabled)
                                        .key_binding(KeyBinding::for_action(
                                            &menu::Confirm,
                                            cx,
                                        ))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.confirm(&menu::Confirm, window, cx);
                                        })),
                                ),
                        ),
                    ),
            )
    }
}
