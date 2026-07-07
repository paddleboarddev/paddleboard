use gpui::{
    DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Render, Stateful, WeakEntity,
};
use paddleboard_personas::{Persona, build_overlay, prepend_overlay_to_task};
use paddleboard_personas_settings::PersonasSettings;
use paddleboard_scion::{StartAgentOptions, TemplateInfo};
use settings::Settings as _;
use ui::{
    Button, Icon, IconName, KeyBinding, Label, LabelSize, Modal, ModalFooter, ModalHeader,
    prelude::*,
};
use ui_input::InputField;
use workspace::{ModalView, Workspace};

use crate::ScionStore;

pub struct StartAgentModal {
    scion_store: Entity<ScionStore>,
    workspace: WeakEntity<Workspace>,
    task_input: Entity<InputField>,
    name_input: Entity<InputField>,
    templates: Vec<TemplateInfo>,
    selected_template: Option<usize>,
    personas: Vec<Persona>,
    personas_enabled: bool,
    selected_persona: Option<usize>,
    focus_handle: FocusHandle,
}

impl StartAgentModal {
    pub fn toggle(
        store: Entity<ScionStore>,
        workspace: &mut Workspace,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        // Discover personas HERE, while we hold the Workspace lease — the closure
        // passed to `toggle_modal` runs inside `Workspace::update`, so reading the
        // workspace from the modal constructor double-leases and panics.
        let personas_enabled = PersonasSettings::get_global(cx).enabled;
        let personas = if personas_enabled {
            let project_root = workspace
                .project()
                .read(cx)
                .visible_worktrees(cx)
                .next()
                .map(|worktree| worktree.read(cx).abs_path().to_path_buf());
            paddleboard_personas::discover(project_root.as_deref())
        } else {
            Vec::new()
        };
        let weak_workspace = cx.entity().downgrade();
        workspace.toggle_modal(window, cx, |window, cx| {
            Self::new(store, weak_workspace, personas, personas_enabled, window, cx)
        });
    }

    fn new(
        store: Entity<ScionStore>,
        workspace: WeakEntity<Workspace>,
        personas: Vec<Persona>,
        personas_enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let templates = store.read(cx).templates().to_vec();

        let task_input = cx.new(|cx| {
            InputField::new(window, cx, "Describe what this agent should work on...")
                .label("Task")
                .tab_index(1)
                .tab_stop(true)
        });

        let name_input = cx.new(|cx| {
            let name = format!("pb-agent-{}", cx.entity_id().as_u64() % 10000);
            let input = InputField::new(window, cx, "agent-name")
                .label("Name")
                .tab_index(2)
                .tab_stop(true);
            input.set_text(&name, window, cx);
            input
        });

        Self {
            scion_store: store,
            workspace,
            task_input,
            name_input,
            templates,
            selected_template: None,
            personas,
            personas_enabled,
            selected_persona: None,
            focus_handle: cx.focus_handle(),
        }
    }

    fn confirm(&mut self, _: &menu::Confirm, _window: &mut Window, cx: &mut Context<Self>) {
        let task_text = self.task_input.read(cx).text(cx);
        let task_text = task_text.trim().to_string();
        let name = self.name_input.read(cx).text(cx);
        let name = name.trim().to_string();

        if name.is_empty() {
            return;
        }

        let mut options = StartAgentOptions::default();
        if let Some(idx) = self.selected_template {
            if let Some(template) = self.templates.get(idx) {
                options.template = Some(template.name.clone());
            }
        }

        // A Scion agent runs outside this session, so a selected persona can't
        // ride a system prompt — prepend its overlay to the task text instead.
        let persona_overlay = self
            .selected_persona
            .and_then(|idx| self.personas.get(idx))
            .map(|persona| build_overlay(persona, &self.personas));
        let task_desc = match persona_overlay {
            Some(overlay) => Some(prepend_overlay_to_task(&overlay, &task_text)),
            None if task_text.is_empty() => None,
            None => Some(task_text),
        };

        let start_task = self.scion_store.update(cx, |store, cx| {
            store.start_agent(name, task_desc, options, cx)
        });

        let store = self.scion_store.clone();
        let workspace = self.workspace.clone();
        cx.emit(DismissEvent);
        cx.spawn(async move |_this, cx| {
            match start_task.await {
                Ok(_) => {
                    store.update(cx, |store, cx| store.refresh(cx));
                }
                Err(err) => {
                    workspace
                        .update(cx, |workspace, cx| {
                            workspace.show_error(err, cx);
                        })
                        .ok();
                }
            }
        })
        .detach();
    }

    fn cancel(&mut self, _: &menu::Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }

    fn on_tab(&mut self, _: &menu::SelectNext, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_next(cx);
    }

    fn on_tab_prev(
        &mut self,
        _: &menu::SelectPrevious,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus_prev(cx);
    }

    fn render_persona_selector(&self, cx: &mut Context<Self>) -> Div {
        // Empty state instead of hiding the section — an invisible feature with
        // no explanation reads as a bug (and hides how to get personas at all).
        if self.personas.is_empty() {
            return v_flex()
                .gap_1()
                .child(Label::new("Persona").size(LabelSize::Small))
                .child(
                    Label::new(
                        "No personas found — drop a PERSONA.md at the project root, or grab \
                         a starter role in AI Dock → Personas.",
                    )
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
                );
        }

        let mut options = v_flex().gap_0p5();
        options = options.child(self.render_persona_option(None, cx));
        for idx in 0..self.personas.len() {
            options = options.child(self.render_persona_option(Some(idx), cx));
        }

        v_flex()
            .gap_1()
            .child(Label::new("Persona").size(LabelSize::Small))
            .child(options)
    }

    fn render_persona_option(
        &self,
        index: Option<usize>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let is_selected = self.selected_persona == index;
        let (label, description) = match index {
            Some(idx) => {
                let persona = &self.personas[idx];
                let description = if persona.description.is_empty() {
                    persona.kind.clone()
                } else {
                    persona.description.clone()
                };
                (persona.name.clone(), description)
            }
            None => (
                "No persona".to_string(),
                "The agent runs with its default identity".to_string(),
            ),
        };

        let icon = if is_selected {
            IconName::Check
        } else {
            IconName::Circle
        };

        let icon_color = if is_selected {
            Color::Accent
        } else {
            Color::Muted
        };

        h_flex()
            .id(SharedString::from(format!(
                "persona-{}",
                index.map_or("none".to_string(), |i| i.to_string())
            )))
            .gap_2()
            .px_2()
            .py_1()
            .rounded_sm()
            .cursor_pointer()
            .when(is_selected, |el| {
                el.bg(cx.theme().colors().element_selected)
            })
            .hover(|el| el.bg(cx.theme().colors().element_hover))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected_persona = index;
                cx.notify();
            }))
            .child(Icon::new(icon).size(ui::IconSize::Small).color(icon_color))
            .child(
                v_flex()
                    .child(Label::new(label).size(LabelSize::Small))
                    .when(!description.is_empty(), |el| {
                        el.child(
                            Label::new(description)
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        )
                    }),
            )
    }

    fn render_template_selector(&self, cx: &mut Context<Self>) -> Div {
        let mut options = v_flex().gap_0p5();
        options = options.child(self.render_template_option(None, cx));
        for idx in 0..self.templates.len() {
            options = options.child(self.render_template_option(Some(idx), cx));
        }

        v_flex()
            .gap_1()
            .child(Label::new("Template").size(LabelSize::Small))
            .child(options)
    }

    fn render_template_option(
        &self,
        index: Option<usize>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let is_selected = self.selected_template == index;
        let (label, description) = match index {
            Some(idx) => {
                let template = &self.templates[idx];
                let desc = if template.description.is_empty() {
                    template.harness.clone()
                } else {
                    template.description.clone()
                };
                (template.name.clone(), desc)
            }
            None => ("Default".into(), "Use Scion's default template".into()),
        };

        let icon = if is_selected {
            IconName::Check
        } else {
            IconName::Circle
        };

        let icon_color = if is_selected {
            Color::Accent
        } else {
            Color::Muted
        };

        h_flex()
            .id(SharedString::from(format!(
                "template-{}",
                index.map_or("default".to_string(), |i| i.to_string())
            )))
            .gap_2()
            .px_2()
            .py_1()
            .rounded_sm()
            .cursor_pointer()
            .when(is_selected, |el| {
                el.bg(cx.theme().colors().element_selected)
            })
            .hover(|el| el.bg(cx.theme().colors().element_hover))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected_template = index;
                cx.notify();
            }))
            .child(Icon::new(icon).size(ui::IconSize::Small).color(icon_color))
            .child(
                v_flex()
                    .child(Label::new(label).size(LabelSize::Small))
                    .when(!description.is_empty(), |el| {
                        el.child(
                            Label::new(description)
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        )
                    }),
            )
    }
}

impl EventEmitter<DismissEvent> for StartAgentModal {}

impl Focusable for StartAgentModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ModalView for StartAgentModal {}

impl Render for StartAgentModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.focus_handle(cx);

        v_flex()
            .id("start-scion-agent-modal")
            .key_context("StartAgentModal")
            .w(rems(30.))
            .elevation_3(cx)
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::on_tab))
            .on_action(cx.listener(Self::on_tab_prev))
            .capture_any_mouse_down(cx.listener(|this, _, window, cx| {
                this.focus_handle(cx).focus(window, cx);
            }))
            .child(
                Modal::new("start-scion-agent", None)
                    .header(
                        ModalHeader::new()
                            .headline("Start Scion Agent")
                            .description(
                                "Launch an isolated agent with its own worktree and container.",
                            ),
                    )
                    .child(
                        v_flex()
                            .tab_group()
                            .px_3()
                            .pb_2()
                            .gap_2()
                            .child(self.task_input.clone())
                            .child(self.name_input.clone())
                            .when(!self.templates.is_empty(), |el| {
                                el.child(self.render_template_selector(cx))
                            })
                            .when(self.personas_enabled, |el| {
                                el.child(self.render_persona_selector(cx))
                            }),
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
                                    Button::new("start", "Start Agent")
                                        .style(ButtonStyle::Filled)
                                        .key_binding(
                                            KeyBinding::for_action_in(
                                                &menu::Confirm,
                                                &focus_handle,
                                                cx,
                                            )
                                            .map(|kb| kb.size(rems_from_px(12.))),
                                        )
                                        .disabled(self.name_input.read(cx).is_empty(cx))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.confirm(&menu::Confirm, window, cx);
                                        })),
                                ),
                        ),
                    ),
            )
    }
}
