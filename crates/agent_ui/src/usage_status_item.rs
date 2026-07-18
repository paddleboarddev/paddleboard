//! PaddleBoard: status-bar context-window gauge for the active agent thread.
//!
//! Shows how much of the model's context window the active thread has used,
//! colored by the same thresholds the agent panel uses. All data is local —
//! this reads the token usage the thread already tracks; nothing is reported
//! anywhere.

use acp_thread::{TokenUsage, TokenUsageRatio};
use gpui::{EntityId, Subscription, WeakEntity};
use ui::{Tooltip, prelude::*};
use workspace::{HideStatusItem, StatusItemView, Workspace};

use crate::AgentPanel;

pub struct UsageStatusItem {
    workspace: WeakEntity<Workspace>,
    observed_thread: Option<(EntityId, Subscription)>,
    _workspace_observation: Option<Subscription>,
}

impl UsageStatusItem {
    pub fn new(workspace: &Workspace, cx: &mut Context<Self>) -> Self {
        let weak = workspace.weak_handle();
        let workspace_observation = weak
            .upgrade()
            .map(|workspace| cx.observe(&workspace, |this, _, cx| this.sync(cx)));
        Self {
            workspace: weak,
            observed_thread: None,
            _workspace_observation: workspace_observation,
        }
    }

    /// Track the panel's active thread, re-subscribing when it changes so
    /// streaming token updates (which notify the thread entity) re-render us.
    /// The workspace notifies often, so this stays cheap: resolve + id compare.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let thread = self
            .workspace
            .upgrade()
            .and_then(|workspace| workspace.read(cx).panel::<AgentPanel>(cx))
            .and_then(|panel| panel.read(cx).active_agent_thread(cx));
        match (&thread, &self.observed_thread) {
            (Some(thread), Some((observed_id, _))) if thread.entity_id() == *observed_id => {}
            (Some(thread), _) => {
                let subscription = cx.observe(thread, |_, _, cx| cx.notify());
                self.observed_thread = Some((thread.entity_id(), subscription));
                cx.notify();
            }
            (None, Some(_)) => {
                self.observed_thread = None;
                cx.notify();
            }
            (None, None) => {}
        }
    }

    fn active_usage(&self, cx: &App) -> Option<TokenUsage> {
        self.workspace
            .upgrade()
            .and_then(|workspace| workspace.read(cx).panel::<AgentPanel>(cx))
            .and_then(|panel| panel.read(cx).active_agent_thread(cx))
            .and_then(|thread| thread.read(cx).token_usage().cloned())
    }
}

impl Render for UsageStatusItem {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let gauge = Some(())
            .filter(|_| paddleboard_ui::PaddleboardUiSettings::get(cx).usage_status)
            .and_then(|_| self.active_usage(cx))
            .filter(|usage| usage.used_tokens > 0)
            .map(|usage| {
                let color = match usage.ratio() {
                    TokenUsageRatio::Normal => Color::Muted,
                    TokenUsageRatio::Warning => Color::Warning,
                    TokenUsageRatio::Exceeded => Color::Error,
                };
                let text = if usage.max_tokens > 0 {
                    format!(
                        "{:.0}%",
                        usage.used_tokens as f32 / usage.max_tokens as f32 * 100.0
                    )
                } else {
                    crate::humanize_token_count(usage.used_tokens)
                };
                let tooltip_text = format!(
                    "Agent context: {} of {} tokens used ({} input, {} output). Local only — never reported.",
                    crate::humanize_token_count(usage.used_tokens),
                    if usage.max_tokens > 0 {
                        crate::humanize_token_count(usage.max_tokens)
                    } else {
                        "unknown".to_string()
                    },
                    crate::humanize_token_count(usage.input_tokens),
                    crate::humanize_token_count(usage.output_tokens),
                );
                Button::new("agent-usage-status", text)
                    .label_size(LabelSize::Small)
                    .color(color)
                    .tooltip(Tooltip::text(tooltip_text))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(workspace) = this.workspace.upgrade() {
                            workspace.update(cx, |workspace, cx| {
                                workspace.toggle_panel_focus::<AgentPanel>(window, cx);
                            });
                        }
                    }))
            });
        div().children(gauge)
    }
}

impl StatusItemView for UsageStatusItem {
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn workspace::ItemHandle>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // PaddleBoard: the status bar calls this while the workspace entity is
        // mid-update, so `sync` (which reads the workspace) would re-enter and
        // panic. Defer it until the current update completes.
        let this = cx.entity().downgrade();
        cx.defer(move |cx| {
            this.update(cx, |this, cx| this.sync(cx)).ok();
        });
    }

    fn hide_setting(&self, _cx: &App) -> Option<HideStatusItem> {
        Some(HideStatusItem::new(|settings| {
            settings.paddleboard_ui.get_or_insert_default().usage_status = Some(false);
        }))
    }
}
