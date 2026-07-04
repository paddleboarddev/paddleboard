use anyhow::Result;
use gpui::{
    Action, AnyView, App, AsyncWindowContext, Context, Entity, EventEmitter, FocusHandle,
    Focusable, IntoElement, Pixels, Render, WeakEntity, Window, prelude::*, px,
};
use language_model::{
    ApiKeyConfiguration, ConfiguredModel, IconOrSvg, LanguageModelProvider,
    LanguageModelProviderId, LanguageModelRegistry, PADDLEBOARD_CLOUD_PROVIDER_ID,
    ProviderSettingsView,
};
use std::sync::Arc;
use ui::{ButtonSize, ButtonStyle, prelude::*};
use workspace::{
    Workspace,
    dock::{DockPosition, Panel, PanelEvent},
};

gpui::actions!(llm_picker, [ToggleFocus]);

const LLM_PICKER_PANEL_KEY: &str = "LlmPickerPanel";

// PaddleBoard: upstream replaced the per-provider `configuration_view()` with
// `settings_view()`, whose API-key mode is pure data rendered by settings_ui.
// Inline/sub-page providers still hand us an embeddable view; API-key
// providers get a status summary plus a jump to the settings page.
#[derive(Clone)]
enum ProviderConfigView {
    Embedded(AnyView),
    ApiKey(ApiKeyConfiguration),
}

pub struct LlmPicker {
    focus_handle: FocusHandle,
    position: DockPosition,
    selected_provider_id: Option<LanguageModelProviderId>,
    configuration_view: Option<ProviderConfigView>,
    _subscriptions: Vec<gpui::Subscription>,
}

impl LlmPicker {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let registry = LanguageModelRegistry::global(cx);

        let subscriptions = vec![cx.observe(&registry, |_, _, cx| {
            cx.notify();
        })];

        let initial_provider_id = LanguageModelRegistry::read_global(cx)
            .default_model()
            .map(|m| m.provider.id())
            .filter(|id| id != &PADDLEBOARD_CLOUD_PROVIDER_ID);

        let configuration_view = initial_provider_id.as_ref().and_then(|id| {
            let provider = LanguageModelRegistry::read_global(cx).provider(id)?;
            Self::configuration_view_for(&provider, window, cx)
        });

        Self {
            focus_handle: cx.focus_handle(),
            position: DockPosition::Right,
            selected_provider_id: initial_provider_id,
            configuration_view,
            _subscriptions: subscriptions,
        }
    }

    pub async fn load(
        _workspace: WeakEntity<Workspace>,
        mut cx: AsyncWindowContext,
    ) -> Result<Entity<Self>> {
        cx.new_window_entity(|window, cx| Self::new(window, cx))
    }

    fn select_provider(
        &mut self,
        provider_id: LanguageModelProviderId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(provider) = LanguageModelRegistry::read_global(cx).provider(&provider_id) else {
            return;
        };

        self.selected_provider_id = Some(provider_id);
        self.configuration_view = Self::configuration_view_for(&provider, window, cx);
        cx.notify();
    }

    fn configuration_view_for(
        provider: &Arc<dyn LanguageModelProvider>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<ProviderConfigView> {
        match provider.settings_view(cx)? {
            ProviderSettingsView::ApiKey(config) => Some(ProviderConfigView::ApiKey(config)),
            ProviderSettingsView::Inline(settings) => Some(ProviderConfigView::Embedded(
                (settings.create_view)(window, cx),
            )),
            ProviderSettingsView::SubPage(settings) => Some(ProviderConfigView::Embedded(
                (settings.create_view)(window, cx),
            )),
        }
    }

    fn use_as_default(&mut self, cx: &mut Context<Self>) {
        let Some(provider_id) = self.selected_provider_id.clone() else {
            return;
        };

        let registry = LanguageModelRegistry::global(cx);
        registry.update(cx, |registry, cx| {
            let Some(provider) = registry.provider(&provider_id) else {
                return;
            };
            let Some(model) = provider.default_model(cx) else {
                return;
            };
            registry.set_default_model(Some(ConfiguredModel { provider, model }), cx);
        });
    }
}

impl EventEmitter<PanelEvent> for LlmPicker {}

impl Focusable for LlmPicker {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for LlmPicker {
    fn persistent_name() -> &'static str {
        "LlmPickerPanel"
    }

    fn panel_key() -> &'static str {
        LLM_PICKER_PANEL_KEY
    }

    fn position(&self, _window: &Window, _cx: &App) -> DockPosition {
        self.position
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        matches!(position, DockPosition::Left | DockPosition::Right)
    }

    fn set_position(
        &mut self,
        position: DockPosition,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.position = position;
        cx.notify();
    }

    fn default_size(&self, _window: &Window, _cx: &App) -> Pixels {
        px(280.0)
    }

    fn icon(&self, _window: &Window, _cx: &App) -> Option<IconName> {
        Some(IconName::Sparkle)
    }

    fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
        Some("AI Provider")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        8
    }
}

impl Render for LlmPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();

        let (provider_rows, default_provider_id) = {
            let registry = LanguageModelRegistry::read_global(cx);
            let rows = registry
                .visible_providers()
                .into_iter()
                .filter(|p| p.id() != PADDLEBOARD_CLOUD_PROVIDER_ID)
                .map(|p| {
                    let id = p.id();
                    let name = p.name().0;
                    let icon = p.icon();
                    let is_authenticated = p.is_authenticated(cx);
                    (id, name, icon, is_authenticated)
                })
                .collect::<Vec<_>>();
            let default_id = registry.default_model().map(|m| m.provider.id());
            (rows, default_id)
        };

        let selected_id = self.selected_provider_id.clone();
        let selected_is_authenticated = selected_id.as_ref().map_or(false, |id| {
            provider_rows
                .iter()
                .any(|(pid, _, _, auth)| pid == id && *auth)
        });

        v_flex()
            .size_full()
            .pt(DynamicSpacing::Base32.px(cx))
            .bg(colors.panel_background)
            .child(
                v_flex()
                    .px_2()
                    .py_1()
                    .gap_0p5()
                    .border_b_1()
                    .border_color(colors.border_variant)
                    .children(
                        provider_rows
                            .into_iter()
                            .enumerate()
                            .map(|(index, (provider_id, name, icon, is_authenticated))| {
                                let is_selected =
                                    selected_id.as_ref() == Some(&provider_id);
                                let is_default =
                                    default_provider_id.as_ref() == Some(&provider_id);

                                h_flex()
                                    .id(("llm-provider-row", index))
                                    .w_full()
                                    .px_2()
                                    .py_1()
                                    .gap_2()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .when(is_selected, |this| {
                                        this.bg(colors.element_selected)
                                    })
                                    .when(!is_selected, |this| {
                                        this.hover(|this| this.bg(colors.element_hover))
                                    })
                                    .on_click(cx.listener({
                                        move |this, _, window, cx| {
                                            this.select_provider(
                                                provider_id.clone(),
                                                window,
                                                cx,
                                            );
                                        }
                                    }))
                                    .child(
                                        match icon {
                                            IconOrSvg::Icon(icon_name) => {
                                                Icon::new(icon_name)
                                            }
                                            IconOrSvg::Svg(path) => {
                                                Icon::from_external_svg(path)
                                            }
                                        }
                                        .size(IconSize::Small)
                                        .color(if is_selected {
                                            Color::Default
                                        } else {
                                            Color::Muted
                                        }),
                                    )
                                    .child(
                                        Label::new(name)
                                            .size(LabelSize::Small)
                                            .color(if is_selected {
                                                Color::Default
                                            } else {
                                                Color::Muted
                                            }),
                                    )
                                    .child(div().flex_1())
                                    .when(is_default, |this| {
                                        this.child(
                                            Label::new("default")
                                                .size(LabelSize::XSmall)
                                                .color(Color::Muted),
                                        )
                                    })
                                    .child(
                                        Icon::new(if is_authenticated {
                                            IconName::Check
                                        } else {
                                            IconName::Close
                                        })
                                        .size(IconSize::XSmall)
                                        .color(if is_authenticated {
                                            Color::Success
                                        } else {
                                            Color::Muted
                                        }),
                                    )
                            }),
                    ),
            )
            .when_some(self.configuration_view.clone(), |this, config_view| {
                this.child(
                    v_flex()
                        .id("llm-config-section")
                        .flex_1()
                        .overflow_y_scroll()
                        .px_3()
                        .py_2()
                        .gap_2()
                        .map(|this| match config_view {
                            ProviderConfigView::Embedded(view) => this.child(view),
                            ProviderConfigView::ApiKey(config) => this
                                .child(
                                    Label::new(if config.is_from_env_var {
                                        format!("API key set via {}.", config.env_var_name)
                                    } else if config.has_key {
                                        "API key configured.".to_string()
                                    } else {
                                        "No API key configured.".to_string()
                                    })
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                                )
                                .child(
                                    Button::new("open-provider-settings", "Open Provider Settings")
                                        .style(ButtonStyle::Outlined)
                                        .size(ButtonSize::Default)
                                        .on_click(|_, window, cx| {
                                            window.dispatch_action(
                                                Box::new(paddleboard_actions::OpenSettingsAt {
                                                    path: "llm_providers".to_string(),
                                                    target: None,
                                                }),
                                                cx,
                                            );
                                        }),
                                ),
                        })
                        .when(selected_is_authenticated, |this| {
                            this.child(
                                Button::new("use-as-default", "Use as Default")
                                    .style(ButtonStyle::Filled)
                                    .size(ButtonSize::Default)
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.use_as_default(cx);
                                    })),
                            )
                        }),
                )
            })
    }
}

pub fn init(cx: &mut App) {
    cx.observe_new(
        |workspace: &mut Workspace,
         _window: Option<&mut Window>,
         _cx: &mut Context<Workspace>| {
            workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
                workspace.toggle_panel_focus::<LlmPicker>(window, cx);
            });
        },
    )
    .detach();
}
