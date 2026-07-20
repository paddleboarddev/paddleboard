// PaddleBoard: the onboarding AI-provider step. Upstream onboarding never let a
// new user configure a model, so they finished setup unable to talk to any
// agent. This section fixes that inline: a zero-key "Local Models" path plus
// per-provider API-key entry, all without leaving onboarding.

use std::collections::HashMap;

use gpui::{AnyView, App, Context, Entity, Subscription, Window};
use language_model::{
    ApiKeyConfiguration, IconOrSvg, LanguageModelProviderId, LanguageModelRegistry,
    PADDLEBOARD_CLOUD_PROVIDER_ID, ProviderSettingsView,
};
use paddleboard_llama_manager::ui::LocalModelsView;
use ui::{ButtonLink, ConfiguredApiCard, Divider, prelude::*};
use ui_input::InputField;

// The managed local-models provider is surfaced as its own hero card, so it is
// excluded from the "bring your own key" list to avoid rendering it twice.
const LLAMA_CPP_PROVIDER_ID: &str = "llama.cpp";

/// How a provider's configuration is presented once its row is expanded.
enum RowConfig {
    /// A single-API-key provider (OpenAI, Anthropic, Google, …).
    ApiKey(ApiKeyConfiguration),
    /// A provider that hands us its own embeddable settings view.
    Embeddable,
    /// A provider with no settings view; nothing to configure inline.
    None,
}

struct RowMeta {
    id: LanguageModelProviderId,
    name: SharedString,
    icon: IconOrSvg,
    authenticated: bool,
    config: RowConfig,
    expanded: bool,
}

pub struct AiProviderSection {
    /// The zero-key path: a managed model running on the user's machine.
    local_models: Entity<LocalModelsView>,
    /// Which provider row is currently expanded, if any.
    expanded: Option<LanguageModelProviderId>,
    /// Lazily-built, persistent API-key inputs, keyed by provider id.
    api_key_inputs: HashMap<LanguageModelProviderId, Entity<InputField>>,
    /// Lazily-built embedded settings views for Inline/SubPage providers.
    embedded_views: HashMap<LanguageModelProviderId, AnyView>,
    _subscriptions: Vec<Subscription>,
}

impl AiProviderSection {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let local_models = cx.new(|cx| LocalModelsView::new(cx));
        let subscription = cx.observe(&LanguageModelRegistry::global(cx), |_, _, cx| cx.notify());

        Self {
            local_models,
            expanded: None,
            api_key_inputs: HashMap::default(),
            embedded_views: HashMap::default(),
            _subscriptions: vec![subscription],
        }
    }

    /// The non-cloud, non-llama providers a user can configure with a key.
    fn byo_providers(cx: &App) -> Vec<LanguageModelProviderId> {
        LanguageModelRegistry::read_global(cx)
            .visible_providers()
            .iter()
            .map(|provider| provider.id())
            .filter(|id| {
                id != &PADDLEBOARD_CLOUD_PROVIDER_ID && id.0.as_ref() != LLAMA_CPP_PROVIDER_ID
            })
            .collect()
    }

    fn toggle_provider(
        &mut self,
        id: LanguageModelProviderId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.expanded.as_ref() == Some(&id) {
            self.expanded = None;
            cx.notify();
            return;
        }

        // Build the (persistent) configuration widgets the first time a row is
        // opened. Inputs and embedded views must outlive a single render, so
        // they are created here — where a Window is available — not in render.
        if let Some(provider) = LanguageModelRegistry::read_global(cx).provider(&id) {
            match provider.settings_view(cx) {
                Some(ProviderSettingsView::ApiKey(_)) => {
                    self.api_key_inputs.entry(id.clone()).or_insert_with(|| {
                        cx.new(|cx| {
                            InputField::new(window, cx, "Paste your API key")
                                .label("API Key")
                                .masked(true)
                                .tab_stop(true)
                        })
                    });
                }
                Some(ProviderSettingsView::Inline(settings)) => {
                    self.embedded_views
                        .entry(id.clone())
                        .or_insert_with(|| (settings.create_view)(window, cx));
                }
                Some(ProviderSettingsView::SubPage(settings)) => {
                    self.embedded_views
                        .entry(id.clone())
                        .or_insert_with(|| (settings.create_view)(window, cx));
                }
                None => {}
            }
        }

        self.expanded = Some(id);
        cx.notify();
    }

    fn save_api_key(
        &mut self,
        id: LanguageModelProviderId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(input) = self.api_key_inputs.get(&id).cloned() else {
            return;
        };
        let key = input.read(cx).text(cx);
        if key.is_empty() {
            return;
        }
        if let Some(provider) = LanguageModelRegistry::read_global(cx).provider(&id) {
            provider.set_api_key(Some(key), cx).detach_and_log_err(cx);
        }
        input.update(cx, |field, cx| field.clear(window, cx));
        // Collapse on save; the row now shows the "configured" state.
        self.expanded = None;
        cx.notify();
    }

    fn collect_rows(&self, cx: &mut App) -> Vec<RowMeta> {
        Self::byo_providers(cx)
            .into_iter()
            .filter_map(|id| {
                let provider = LanguageModelRegistry::read_global(cx).provider(&id)?;
                let name = provider.name().0;
                let icon = provider.icon();
                let authenticated = provider.is_authenticated(cx);
                let config = match provider.settings_view(cx) {
                    Some(ProviderSettingsView::ApiKey(config)) => RowConfig::ApiKey(config),
                    Some(_) => RowConfig::Embeddable,
                    None => RowConfig::None,
                };
                Some(RowMeta {
                    expanded: self.expanded.as_ref() == Some(&id),
                    id,
                    name,
                    icon,
                    authenticated,
                    config,
                })
            })
            .collect()
    }

    fn render_provider_icon(icon: &IconOrSvg) -> Icon {
        match icon {
            IconOrSvg::Icon(icon_name) => Icon::new(*icon_name),
            IconOrSvg::Svg(icon_path) => Icon::from_external_svg(icon_path.clone()),
        }
    }

    fn render_expanded_config(
        &self,
        row: &RowMeta,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        match &row.config {
            RowConfig::ApiKey(config) if config.has_key => {
                let label = if config.is_from_env_var {
                    "API Key set from environment variable"
                } else {
                    "API Key configured"
                };
                let id = row.id.clone();
                let card = ConfiguredApiCard::new(format!("reset-{}", row.id.0), label)
                    .button_label("Reset Key")
                    .disabled(config.is_from_env_var)
                    .on_click(cx.listener(move |_, _, _, cx| {
                        if let Some(provider) =
                            LanguageModelRegistry::read_global(cx).provider(&id)
                        {
                            provider.set_api_key(None, cx).detach_and_log_err(cx);
                        }
                    }));
                Some(v_flex().pt_2().child(card).into_any_element())
            }
            RowConfig::ApiKey(config) => {
                let input = self.api_key_inputs.get(&row.id)?.clone();
                let save_id = row.id.clone();
                let dashboard_url = config.api_key_url.clone();
                Some(
                    v_flex()
                        .pt_2()
                        .gap_1p5()
                        .child(input)
                        .child(
                            h_flex()
                                .justify_between()
                                .gap_2()
                                .child(
                                    ButtonLink::new("Get an API key", dashboard_url)
                                        .no_icon(true)
                                        .label_size(LabelSize::Small)
                                        .label_color(Color::Muted),
                                )
                                .child(
                                    Button::new("save-api-key", "Save Key")
                                        .style(ButtonStyle::Filled)
                                        .label_size(LabelSize::Small)
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.save_api_key(save_id.clone(), window, cx);
                                        })),
                                ),
                        )
                        .into_any_element(),
                )
            }
            RowConfig::Embeddable => {
                let view = self.embedded_views.get(&row.id)?.clone();
                Some(v_flex().pt_2().child(view).into_any_element())
            }
            RowConfig::None => None,
        }
    }

    fn render_provider_row(&self, row: RowMeta, cx: &mut Context<Self>) -> impl IntoElement {
        let border_variant = cx.theme().colors().border_variant;
        let surface = cx.theme().colors().elevated_surface_background;
        let id = row.id.clone();
        let expanded = row.expanded;
        let expanded_config = if expanded {
            self.render_expanded_config(&row, cx)
        } else {
            None
        };

        v_flex()
            .w_full()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(border_variant)
            .bg(surface.opacity(0.5))
            .child(
                h_flex()
                    .id(SharedString::from(format!("provider-row-{}", row.id.0)))
                    .w_full()
                    .justify_between()
                    .gap_2()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.toggle_provider(id.clone(), window, cx);
                    }))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Self::render_provider_icon(&row.icon)
                                    .size(IconSize::Small)
                                    .color(Color::Muted),
                            )
                            .child(Label::new(row.name.clone())),
                    )
                    .child(
                        h_flex()
                            .gap_1p5()
                            .when(row.authenticated, |this| {
                                this.child(
                                    Icon::new(IconName::Check)
                                        .size(IconSize::Small)
                                        .color(Color::Success),
                                )
                            })
                            .child(
                                Icon::new(if expanded {
                                    IconName::ChevronUp
                                } else {
                                    IconName::ChevronDown
                                })
                                .size(IconSize::Small)
                                .color(Color::Muted),
                            ),
                    ),
            )
            .when_some(expanded_config, |this, config| this.child(config))
    }
}

impl Render for AiProviderSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.collect_rows(cx);
        // Build rows eagerly: each borrows `cx` mutably, which a lazy `.map`
        // closure can't express (the borrow would escape the FnMut body).
        let mut provider_rows = Vec::with_capacity(rows.len());
        for row in rows {
            provider_rows.push(self.render_provider_row(row, cx).into_any_element());
        }

        v_flex()
            .gap_0p5()
            .child(Label::new("AI Providers"))
            .child(
                Label::new(
                    "Connect a model so the agent can run. Use a local model with no \
                     key, or add your own API key.",
                )
                .color(Color::Muted),
            )
            // Zero-key hero: a managed model on the user's own machine.
            .child(
                v_flex()
                    .mt_1p5()
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().colors().border_variant)
                    .bg(cx.theme().colors().elevated_surface_background.opacity(0.5))
                    .gap_1()
                    .child(
                        Label::new("Local Models — no API key needed")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .child(self.local_models.clone()),
            )
            .child(
                h_flex()
                    .mt_2()
                    .gap_2()
                    .child(
                        Label::new("Or bring your own key")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .child(Divider::horizontal()),
            )
            .child(v_flex().gap_1p5().children(provider_rows))
    }
}
