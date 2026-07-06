use crate::{
    CATALOG, DEFAULT_MODEL_ID, LlamaManager, ManagerStatus, catalog_model, manager,
    model_is_downloaded,
};
use fs::Fs;
use gpui::{Entity, FontWeight, Subscription};
use ui::{
    ContextMenu, DropdownMenu, DropdownStyle, IconPosition, ProgressBar, Switch, ToggleState,
    prelude::*,
};

/// The "Local Models" panel embedded at the top of the llama.cpp provider's
/// configuration page. Presents the managed experience: a run toggle, a model
/// picker, a download action with progress, and live status.
pub struct LocalModelsView {
    manager: Option<Entity<LlamaManager>>,
    _subscription: Option<Subscription>,
}

impl LocalModelsView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let manager = manager(cx);
        // Re-render whenever the manager's status changes.
        let subscription = manager
            .as_ref()
            .map(|manager| cx.observe(manager, |_, _, cx| cx.notify()));
        Self {
            manager,
            _subscription: subscription,
        }
    }

    fn is_enabled(&self, cx: &App) -> bool {
        self.manager
            .as_ref()
            .is_some_and(|manager| manager.read(cx).is_enabled())
    }

    fn selected_model_id(&self, cx: &App) -> String {
        self.manager
            .as_ref()
            .map(|manager| manager.read(cx).selected_model().to_string())
            .unwrap_or_else(|| DEFAULT_MODEL_ID.to_string())
    }

    /// Persist the managed settings so the choice survives restarts. The provider
    /// also observes this write and reconciles the manager; we additionally drive
    /// the manager directly below for immediate feedback.
    fn persist(&self, enabled: bool, model_id: String, cx: &App) {
        let fs = <dyn Fs>::global(cx);
        settings::update_settings_file(fs, cx, move |settings, _| {
            let managed = settings
                .language_models
                .get_or_insert_default()
                .llama_cpp
                .get_or_insert_default()
                .managed
                .get_or_insert_default();
            managed.enabled = Some(enabled);
            managed.model = Some(model_id);
        });
    }

    fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let model_id = self.selected_model_id(cx);
        self.persist(enabled, model_id.clone(), cx);
        if let Some(manager) = &self.manager {
            manager.update(cx, |manager, cx| {
                manager.set_managed(enabled, model_id, cx);
            });
        }
    }

    fn select_model(&mut self, model_id: String, cx: &mut Context<Self>) {
        let enabled = self.is_enabled(cx);
        self.persist(enabled, model_id.clone(), cx);
        if let Some(manager) = &self.manager {
            manager.update(cx, |manager, cx| {
                manager.set_managed(enabled, model_id, cx);
            });
        }
    }

    fn download_or_retry(&mut self, cx: &mut Context<Self>) {
        let model_id = self.selected_model_id(cx);
        self.persist(true, model_id.clone(), cx);
        if let Some(manager) = &self.manager {
            manager.update(cx, |manager, cx| {
                manager.ensure_running(model_id, cx);
            });
        }
    }

    fn render_model_picker(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected_id = self.selected_model_id(cx);
        let selected_label = catalog_model(&selected_id)
            .map(|model| model.display_name.to_string())
            .unwrap_or_else(|| selected_id.clone());
        // Switching mid-transfer would restart the download, so lock the picker.
        let busy = matches!(
            self.status(cx),
            ManagerStatus::Preparing
                | ManagerStatus::Downloading { .. }
                | ManagerStatus::Starting { .. }
        );
        let view = cx.entity();

        let menu = ContextMenu::build(window, cx, move |mut menu, _window, _cx| {
            for model in CATALOG {
                let is_selected = model.id == selected_id;
                let view = view.clone();
                let model_id = model.id.to_string();
                menu = menu.toggleable_entry(
                    model.display_name,
                    is_selected,
                    IconPosition::Start,
                    None,
                    move |_window, cx| {
                        view.update(cx, |this, cx| this.select_model(model_id.clone(), cx));
                    },
                );
            }
            menu
        });

        DropdownMenu::new("local-models-picker", selected_label, menu)
            .style(DropdownStyle::Outlined)
            .disabled(busy)
    }

    fn status(&self, cx: &App) -> ManagerStatus {
        self.manager
            .as_ref()
            .map(|manager| manager.read(cx).status().clone())
            .unwrap_or(ManagerStatus::Idle)
    }

    fn render_status(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.status(cx) {
            ManagerStatus::Unsupported => Label::new(
                "Local Models aren't available on this platform yet (macOS Apple Silicon and Linux are supported).",
            )
            .size(LabelSize::Small)
            .color(Color::Muted)
            .into_any_element(),
            ManagerStatus::Idle => {
                if self.is_enabled(cx) {
                    status_row(IconName::Check, Color::Muted, "Enabled").into_any_element()
                } else {
                    Label::new("Turn on to download and run a model locally.")
                        .size(LabelSize::Small)
                        .color(Color::Muted)
                        .into_any_element()
                }
            }
            ManagerStatus::Preparing => {
                status_row(IconName::Download, Color::Muted, "Preparing the local runtime…")
                    .into_any_element()
            }
            ManagerStatus::Downloading {
                model,
                received,
                total,
            } => {
                let name = display_name(&model);
                let label = match total {
                    Some(total) if total > 0 => {
                        let percent = (received as f64 / total as f64 * 100.0).round() as u32;
                        format!(
                            "Downloading {name} — {percent}% ({} / {})",
                            fmt_gb(received),
                            fmt_gb(total)
                        )
                    }
                    _ => format!("Downloading {name} — {}", fmt_gb(received)),
                };
                v_flex()
                    .gap_1p5()
                    .child(
                        Label::new(label)
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .child(ProgressBar::new(
                        "local-models-progress",
                        received as f32,
                        total.filter(|total| *total > 0).unwrap_or(received.max(1)) as f32,
                        cx,
                    ))
                    .into_any_element()
            }
            ManagerStatus::Starting { model } => status_row(
                IconName::Download,
                Color::Muted,
                format!("Starting {}…", display_name(&model)),
            )
            .into_any_element(),
            ManagerStatus::Ready { model, port } => status_row(
                IconName::Check,
                Color::Success,
                format!("Ready — {} is serving on port {port}", display_name(&model)),
            )
            .into_any_element(),
            ManagerStatus::Error { message } => v_flex()
                .gap_1p5()
                .child(status_row(
                    IconName::Warning,
                    Color::Error,
                    format!("Error: {message}"),
                ))
                .child(
                    Button::new("local-models-retry", "Retry")
                        .style(ButtonStyle::Outlined)
                        .label_size(LabelSize::Small)
                        .on_click(cx.listener(|this, _, _, cx| this.download_or_retry(cx))),
                )
                .into_any_element(),
        }
    }

    /// A "Download" button shown when managed mode is on but the selected model
    /// hasn't been fetched yet and nothing is in flight.
    fn render_download_action(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.is_enabled(cx) {
            return None;
        }
        let idle_or_error = matches!(
            self.status(cx),
            ManagerStatus::Idle | ManagerStatus::Error { .. }
        );
        let selected_id = self.selected_model_id(cx);
        let downloaded = catalog_model(&selected_id).is_some_and(model_is_downloaded);
        if !idle_or_error || downloaded {
            return None;
        }
        Some(
            Button::new("local-models-download", "Download & Run")
                .style(ButtonStyle::Filled)
                .label_size(LabelSize::Small)
                .start_icon(Icon::new(IconName::Download).size(IconSize::Small))
                .on_click(cx.listener(|this, _, _, cx| this.download_or_retry(cx)))
                .into_any_element(),
        )
    }
}

impl Render for LocalModelsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = self.is_enabled(cx);
        let supported = !matches!(self.status(cx), ManagerStatus::Unsupported);
        let description = catalog_model(&self.selected_model_id(cx)).map(|model| model.description);

        v_flex()
            .gap_2()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().colors().border_variant)
            .bg(cx.theme().colors().background.opacity(0.5))
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        v_flex()
                            .child(Label::new("Local Models").weight(FontWeight::MEDIUM))
                            .child(
                                Label::new("Run a model locally, managed by PaddleBoard.")
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            ),
                    )
                    .child(
                        Switch::new(
                            "local-models-toggle",
                            if enabled {
                                ToggleState::Selected
                            } else {
                                ToggleState::Unselected
                            },
                        )
                        .disabled(!supported)
                        .on_click(cx.listener(|this, state: &ToggleState, _window, cx| {
                            this.set_enabled(*state == ToggleState::Selected, cx);
                        })),
                    ),
            )
            .when(supported, |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            Label::new("Model")
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        )
                        .child(self.render_model_picker(window, cx)),
                )
                .when_some(description, |this, description| {
                    this.child(
                        Label::new(description)
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                })
            })
            .child(self.render_status(cx))
            .when_some(self.render_download_action(cx), |this, action| {
                this.child(action)
            })
    }
}

fn status_row(icon: IconName, color: Color, text: impl Into<SharedString>) -> impl IntoElement {
    h_flex()
        .gap_1p5()
        .items_center()
        .child(Icon::new(icon).size(IconSize::Small).color(color))
        .child(Label::new(text).size(LabelSize::Small).color(Color::Muted))
}

fn display_name(model_id: &str) -> String {
    catalog_model(model_id)
        .map(|model| model.display_name.to_string())
        .unwrap_or_else(|| model_id.to_string())
}

fn fmt_gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
}
