use collections::HashMap;
use fs::Fs;
use gpui::{DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Render};
use settings::{CustomAgentServerSettings, update_settings_file};
use ui::{
    Button, KeyBinding, Label, LabelSize, Modal, ModalFooter, ModalHeader, prelude::*,
};
use ui_input::InputField;
use workspace::ModalView;

pub struct AddAgentModal {
    agent_id_input: Entity<InputField>,
    focus_handle: FocusHandle,
}

impl AddAgentModal {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let agent_id_input = cx.new(|cx| {
            InputField::new(window, cx, "e.g. anthropic/claude-code")
                .label("Agent Server ID")
                .tab_index(1)
                .tab_stop(true)
        });

        Self {
            agent_id_input,
            focus_handle: cx.focus_handle(),
        }
    }

    fn confirm(&mut self, _: &menu::Confirm, _window: &mut Window, cx: &mut Context<Self>) {
        let agent_id = self.agent_id_input.read(cx).text(cx);
        let agent_id = agent_id.trim().to_string();

        if agent_id.is_empty() {
            return;
        }

        let fs = <dyn Fs>::global(cx);
        update_settings_file(fs.clone(), cx, move |settings, _| {
            let agent_servers = settings.agent_servers.get_or_insert_default();
            agent_servers.entry(agent_id).or_insert_with(|| {
                CustomAgentServerSettings::Registry {
                    env: Default::default(),
                    default_mode: None,
                    default_config_options: HashMap::default(),
                    favorite_config_option_values: HashMap::default(),
                }
            });
        });

        cx.emit(DismissEvent);
    }

    fn cancel(&mut self, _: &menu::Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }
}

impl EventEmitter<DismissEvent> for AddAgentModal {}

impl Focusable for AddAgentModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ModalView for AddAgentModal {}

impl Render for AddAgentModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.focus_handle(cx);

        v_flex()
            .id("add-agent-modal")
            .key_context("AddAgentModal")
            .w(rems(28.))
            .elevation_3(cx)
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::cancel))
            .capture_any_mouse_down(cx.listener(|this, _, window, cx| {
                this.focus_handle(cx).focus(window, cx);
            }))
            .child(
                Modal::new("add-agent", None)
                    .header(
                        ModalHeader::new()
                            .headline("Add Agent")
                            .description(
                                "Add an agent server by its registry ID.",
                            ),
                    )
                    .child(
                        v_flex()
                            .px_3()
                            .pb_2()
                            .gap_2()
                            .child(self.agent_id_input.clone())
                            .child(
                                Label::new(
                                    "The agent will be added to your settings and \
                                     available in the Agent panel.",
                                )
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                            ),
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
                                    Button::new("add-agent-confirm", "Add Agent")
                                        .style(ButtonStyle::Filled)
                                        .key_binding(
                                            KeyBinding::for_action_in(
                                                &menu::Confirm,
                                                &focus_handle,
                                                cx,
                                            )
                                            .map(|kb| kb.size(rems_from_px(12.))),
                                        )
                                        .disabled(self.agent_id_input.read(cx).is_empty(cx))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.confirm(&menu::Confirm, window, cx);
                                        })),
                                ),
                        ),
                    ),
            )
    }
}
