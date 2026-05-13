use std::collections::HashMap;

use agent_client_protocol as acp;
use acp_thread::ThreadStatus;
use gpui::{
    Action, App, AsyncWindowContext, Context, Entity, EventEmitter, FocusHandle, Focusable,
    IntoElement, Pixels, Render, SharedString, Subscription, WeakEntity, Window, prelude::*, px,
};
use ui::{Color, Icon, IconName, IconSize, Label, LabelSize, prelude::*};
use workspace::{
    Workspace,
    dock::{DockPosition, Panel, PanelEvent},
};

use crate::agent_panel::AgentPanel;
use crate::conversation_view::{ConversationView, ThreadView};

gpui::actions!(orchestration_panel, [ToggleFocus]);

const ORCHESTRATION_PANEL_KEY: &str = "OrchestrationPanel";

pub struct OrchestrationPanel {
    focus_handle: FocusHandle,
    position: DockPosition,
    workspace: WeakEntity<Workspace>,
    /// Cached reference to the AgentPanel once it becomes available.
    agent_panel: Option<Entity<AgentPanel>>,
    /// Per-thread subscriptions so we re-render on status changes.
    thread_subscriptions: HashMap<acp::SessionId, Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl OrchestrationPanel {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();

        let mut panel = Self {
            focus_handle,
            position: DockPosition::Right,
            workspace: workspace.clone(),
            agent_panel: None,
            thread_subscriptions: HashMap::default(),
            _subscriptions: Vec::new(),
        };

        if let Some(workspace_entity) = workspace.upgrade() {
            let agent_panel_now = workspace_entity.read(cx).panel::<AgentPanel>(cx);
            panel.sync_agent_panel_subscription(agent_panel_now, cx);

            let workspace_subscription = cx.observe(&workspace_entity, |this, workspace, cx| {
                let agent_panel = workspace.read(cx).panel::<AgentPanel>(cx);
                this.sync_agent_panel_subscription(agent_panel, cx);
                cx.notify();
            });
            panel._subscriptions.push(workspace_subscription);
        }

        panel
    }

    pub async fn load(
        workspace: WeakEntity<Workspace>,
        mut cx: AsyncWindowContext,
    ) -> anyhow::Result<Entity<Self>> {
        cx.new_window_entity(|window, cx| Self::new(workspace, window, cx))
    }

    /// Called whenever we detect or lose the AgentPanel. Sets up a subscription so that any
    /// change in the AgentPanel triggers a re-render and a refresh of per-thread subscriptions.
    fn sync_agent_panel_subscription(
        &mut self,
        panel: Option<Entity<AgentPanel>>,
        cx: &mut Context<Self>,
    ) {
        match (&self.agent_panel, &panel) {
            (None, Some(_)) => {
                let panel = panel.unwrap();
                let subscription = cx.observe(&panel, |this, agent_panel_entity, cx| {
                    let conv_views = agent_panel_entity.read(cx).all_conversation_views();
                    this.sync_thread_subscriptions(conv_views, cx);
                    cx.notify();
                });
                let conv_views = panel.read(cx).all_conversation_views();
                self.agent_panel = Some(panel);
                self._subscriptions.push(subscription);
                self.sync_thread_subscriptions(conv_views, cx);
            }
            (Some(_), None) => {
                self.agent_panel = None;
                self.thread_subscriptions.clear();
            }
            _ => {}
        }
    }

    /// Subscribes to new thread views and removes stale subscriptions.
    fn sync_thread_subscriptions(
        &mut self,
        conv_views: Vec<Entity<ConversationView>>,
        cx: &mut Context<Self>,
    ) {
        let mut live_ids: std::collections::HashSet<acp::SessionId> =
            std::collections::HashSet::default();

        for conv_view in conv_views {
            let thread_views = conv_view.read(cx).all_thread_views();
            for thread_view in thread_views {
                let session_id = thread_view.read(cx).id.clone();
                live_ids.insert(session_id.clone());
                self.thread_subscriptions
                    .entry(session_id)
                    .or_insert_with(|| {
                        cx.observe(&thread_view, |_, _, cx| {
                            cx.notify();
                        })
                    });
            }
        }

        self.thread_subscriptions
            .retain(|id, _| live_ids.contains(id));
    }

    fn render_thread_tree(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(agent_panel) = &self.agent_panel else {
            return v_flex()
                .p_3()
                .child(
                    Label::new("No agent sessions")
                        .color(Color::Muted)
                        .size(LabelSize::Small),
                )
                .into_any_element();
        };

        let conversation_views = agent_panel.read(cx).all_conversation_views();

        if conversation_views.is_empty() {
            return v_flex()
                .p_3()
                .child(
                    Label::new("No agent sessions")
                        .color(Color::Muted)
                        .size(LabelSize::Small),
                )
                .into_any_element();
        }

        let mut elements: Vec<AnyElement> = Vec::new();

        for conv_view_entity in conversation_views {
            let thread_views = conv_view_entity.read(cx).all_thread_views();

            if thread_views.is_empty() {
                continue;
            }

            // Build a map of session_id → thread_view for hierarchy resolution.
            let view_map: HashMap<acp::SessionId, Entity<ThreadView>> = thread_views
                .iter()
                .map(|tv| (tv.read(cx).id.clone(), tv.clone()))
                .collect();

            // Collect root threads (those with no parent in this conversation).
            let roots: Vec<Entity<ThreadView>> = thread_views
                .iter()
                .filter(|tv| {
                    let parent = tv.read(cx).parent_id.clone();
                    parent.is_none() || !view_map.contains_key(&parent.unwrap())
                })
                .cloned()
                .collect();

            for root in roots {
                self.render_thread_node(
                    &root,
                    &view_map,
                    &conv_view_entity,
                    0,
                    &mut elements,
                    cx,
                );
            }
        }

        if elements.is_empty() {
            return v_flex()
                .p_3()
                .child(
                    Label::new("No agent sessions")
                        .color(Color::Muted)
                        .size(LabelSize::Small),
                )
                .into_any_element();
        }

        v_flex().py_1().children(elements).into_any_element()
    }

    fn render_thread_node(
        &self,
        thread_view: &Entity<ThreadView>,
        view_map: &HashMap<acp::SessionId, Entity<ThreadView>>,
        conv_view: &Entity<ConversationView>,
        depth: usize,
        elements: &mut Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) {
        let thread_data = thread_view.read(cx);
        let session_id = thread_data.id.clone();
        let status = thread_data.thread.read(cx).status();

        let title: SharedString = thread_data
            .thread
            .read(cx)
            .title()
            .unwrap_or_else(|| "Untitled".into());

        let is_generating = matches!(status, ThreadStatus::Generating);

        let indent = depth as f32 * 12.0;

        let nav_session_id = session_id.clone();
        let conv_view_entity = conv_view.clone();
        let workspace = self.workspace.clone();

        let element_id: SharedString = session_id.to_string().into();

        let hover_bg = cx.theme().colors().element_hover;
        let row = h_flex()
            .id(element_id)
            .w_full()
            .h_6()
            .pl(px(4.0 + indent))
            .pr_2()
            .gap_1()
            .items_center()
            .cursor_pointer()
            .hover(move |style| style.bg(hover_bg))
            .on_click(cx.listener(move |_this, _, window, cx| {
                conv_view_entity.update(cx, |conv, cx| {
                    conv.navigate_to_session(nav_session_id.clone(), window, cx);
                });
                if let Some(workspace) = workspace.upgrade() {
                    workspace.update(cx, |workspace, cx| {
                        workspace.focus_panel::<AgentPanel>(window, cx);
                    });
                }
            }))
            .child(
                Icon::new(IconName::ZedAssistant)
                    .size(IconSize::XSmall)
                    .color(if is_generating {
                        Color::Accent
                    } else {
                        Color::Muted
                    }),
            )
            .child(
                Label::new(title)
                    .size(LabelSize::Small)
                    .color(if is_generating {
                        Color::Default
                    } else {
                        Color::Muted
                    }),
            )
            .into_any_element();

        elements.push(row);

        // Collect and render child threads (subagents).
        let children: Vec<Entity<ThreadView>> = view_map
            .values()
            .filter(|tv| tv.read(cx).parent_id.as_ref() == Some(&session_id))
            .cloned()
            .collect();

        for child in children {
            self.render_thread_node(&child, view_map, conv_view, depth + 1, elements, cx);
        }
    }
}

impl EventEmitter<PanelEvent> for OrchestrationPanel {}

impl Focusable for OrchestrationPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for OrchestrationPanel {
    fn persistent_name() -> &'static str {
        "OrchestrationPanel"
    }

    fn panel_key() -> &'static str {
        ORCHESTRATION_PANEL_KEY
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
        px(260.0)
    }

    fn icon(&self, _window: &Window, _cx: &App) -> Option<IconName> {
        Some(IconName::ListTree)
    }

    fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
        Some("Agent Threads")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        9
    }
}

impl Render for OrchestrationPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();

        v_flex()
            .size_full()
            .bg(colors.panel_background)
            .child(
                h_flex()
                    .h(DynamicSpacing::Base32.px(cx))
                    .px_2()
                    .border_b_1()
                    .border_color(colors.border_variant)
                    .items_center()
                    .child(
                        Label::new("Agent Threads")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
            )
            .child(div().flex_1().overflow_hidden().child(self.render_thread_tree(cx)))
    }
}

pub fn init(cx: &mut App) {
    cx.observe_new(
        |workspace: &mut Workspace,
         _window: Option<&mut Window>,
         _cx: &mut Context<Workspace>| {
            workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
                workspace.toggle_panel_focus::<OrchestrationPanel>(window, cx);
            });
        },
    )
    .detach();
}
