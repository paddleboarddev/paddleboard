use std::collections::HashMap;
use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use acp_thread::ThreadStatus;
use gpui::{
    Action, App, AppContext as _, AsyncWindowContext, Context, DismissEvent, Entity, EventEmitter,
    FocusHandle, Focusable, IntoElement, MouseButton, MouseDownEvent, Pixels, Point, Render,
    SharedString, Subscription, WeakEntity, Window, prelude::*, px,
};
use gpui_tokio::Tokio;
// PaddleBoard: Scion orchestration support
use multi_buffer::MultiBuffer;
use paddleboard_scion::{AgentInfo, AgentPhase, ScionCli};
use paddleboard_scion_ui::{
    ScionEvent, ScionEventKind, ScionStore, ScionStoreEvent, ScionStoreGlobal,
};
use settings::Settings;
use ui::{Color, ContextMenu, Icon, IconName, IconSize, Label, LabelSize, prelude::*};
use workspace::{
    Toast, Workspace,
    dock::{DockPosition, Panel, PanelEvent},
    notifications::NotificationId,
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
    // PaddleBoard: Scion agent store for container-isolated parallel agents.
    scion_store: Option<Entity<ScionStore>>,
    scion_context_menu: Option<(Entity<ContextMenu>, Point<Pixels>, Subscription)>,
    _scion_log_streams: Vec<gpui::Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl OrchestrationPanel {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();

        // PaddleBoard: resolve ScionStore from Global if available
        let scion_store = cx
            .try_global::<ScionStoreGlobal>()
            .map(|g| g.0.clone());

        let mut panel = Self {
            focus_handle,
            position: DockPosition::Right,
            workspace: workspace.clone(),
            agent_panel: None,
            thread_subscriptions: HashMap::default(),
            scion_store: scion_store.clone(),
            scion_context_menu: None,
            _scion_log_streams: Vec::new(),
            _subscriptions: Vec::new(),
        };

        // PaddleBoard: subscribe to ScionStore events for re-render
        if let Some(ref store) = scion_store {
            let sub = cx.subscribe(store, |_this, _store, _event: &ScionStoreEvent, cx| {
                cx.notify();
            });
            panel._subscriptions.push(sub);
        }

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
                let session_id = thread_view.read(cx).session_id.clone();
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
                .map(|tv| (tv.read(cx).session_id.clone(), tv.clone()))
                .collect();

            // Collect root threads (those with no parent in this conversation).
            let roots: Vec<Entity<ThreadView>> = thread_views
                .iter()
                .filter(|tv| {
                    let parent = tv.read(cx).parent_session_id.clone();
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
        let session_id = thread_data.session_id.clone();
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
            .filter(|tv| tv.read(cx).parent_session_id.as_ref() == Some(&session_id))
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

impl OrchestrationPanel {
    // PaddleBoard: Scion agent section rendering
    fn render_scion_section(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        // PaddleBoard: opt-in — hide the Scion section unless enabled in settings.
        if !paddleboard_scion_settings::ScionSettings::get_global(cx).enabled {
            return None;
        }
        let store = self.scion_store.as_ref()?;
        let store_read = store.read(cx);
        if !store_read.is_available() {
            return None;
        }

        let agents = store_read.agents().to_vec();
        // Collect owned data now so the `store_read` borrow ends before the row
        // loop below needs `&mut cx`.
        let recent_events: Vec<ScionEvent> =
            store_read.events().iter().rev().take(10).cloned().collect();

        let mut elements: Vec<gpui::AnyElement> = Vec::new();

        elements.push(
            h_flex()
                .h_7()
                .px_2()
                .mt_2()
                .border_b_1()
                .border_color(cx.theme().colors().border_variant)
                .items_center()
                .child(
                    Label::new("Scion Agents")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .into_any_element(),
        );

        if agents.is_empty() {
            elements.push(
                div()
                    .px_2()
                    .py_1()
                    .child(
                        Label::new("No agents running")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .into_any_element(),
            );
        } else {
            for agent in &agents {
                elements.push(self.render_scion_agent_row(agent, cx));
            }
        }

        // PaddleBoard: live activity feed — the same lifecycle transitions that are
        // emitted to OpenTelemetry, surfaced in-panel (newest first).
        if !recent_events.is_empty() {
            elements.push(
                h_flex()
                    .h_7()
                    .px_2()
                    .mt_2()
                    .border_b_1()
                    .border_color(cx.theme().colors().border_variant)
                    .items_center()
                    .child(
                        Label::new("Activity")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .into_any_element(),
            );
            for event in recent_events {
                let summary_color = match event.kind {
                    ScionEventKind::Discovered => Color::Accent,
                    ScionEventKind::Phase => Color::Default,
                    ScionEventKind::Activity => Color::Muted,
                    ScionEventKind::Disappeared => Color::Error,
                };
                elements.push(
                    h_flex()
                        .px_2()
                        .py_0p5()
                        .gap_1p5()
                        .items_center()
                        .child(
                            Label::new(event.agent.clone())
                                .size(LabelSize::Small)
                                .color(Color::Default),
                        )
                        .child(
                            Label::new(event.summary.clone())
                                .size(LabelSize::Small)
                                .color(summary_color),
                        )
                        .into_any_element(),
                );
            }
        }

        Some(v_flex().children(elements).into_any_element())
    }

    fn render_scion_agent_row(
        &self,
        agent: &AgentInfo,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let phase = agent.phase.unwrap_or(AgentPhase::Unknown);
        let activity = agent.activity;

        let icon_color = if phase == AgentPhase::Error {
            Color::Error
        } else if phase == AgentPhase::Running {
            if activity.map_or(false, |a| a.needs_attention()) {
                Color::Warning
            } else {
                Color::Accent
            }
        } else if phase.is_active() {
            Color::Accent
        } else {
            Color::Muted
        };

        let activity_label: Option<SharedString> = activity.map(|a| {
            let base = match a {
                paddleboard_scion::AgentActivity::Working => "working",
                paddleboard_scion::AgentActivity::Thinking => "thinking",
                paddleboard_scion::AgentActivity::Executing => "executing",
                paddleboard_scion::AgentActivity::WaitingForInput => "waiting",
                paddleboard_scion::AgentActivity::Blocked => "blocked",
                paddleboard_scion::AgentActivity::Completed => "done",
                paddleboard_scion::AgentActivity::LimitsExceeded => "limits",
                paddleboard_scion::AgentActivity::Stalled => "stalled",
                paddleboard_scion::AgentActivity::Offline => "offline",
                paddleboard_scion::AgentActivity::Crashed => "crashed",
                paddleboard_scion::AgentActivity::Unknown => "unknown",
            };
            let detail_suffix = agent
                .detail
                .as_ref()
                .and_then(|d| {
                    if !d.tool_name.is_empty() {
                        Some(d.tool_name.as_str())
                    } else if !d.task_summary.is_empty() {
                        let truncated = if d.task_summary.len() > 20 {
                            &d.task_summary[..20]
                        } else {
                            &d.task_summary
                        };
                        Some(truncated)
                    } else {
                        None
                    }
                });
            match detail_suffix {
                Some(detail) => SharedString::from(format!("{base} · {detail}")),
                None => SharedString::from(base),
            }
        });

        let agent_name = agent.name.clone();
        let is_running = phase == AgentPhase::Running || phase.is_active();

        h_flex()
            .id(SharedString::from(format!("scion-agent-{}", agent.name)))
            .w_full()
            .h_7()
            .pl(px(8.0))
            .pr_2()
            .gap_1p5()
            .items_center()
            .cursor_pointer()
            .hover(|style| style.bg(cx.theme().colors().element_hover))
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                this.deploy_scion_context_menu(
                    event.position,
                    agent_name.clone(),
                    is_running,
                    window,
                    cx,
                );
            }))
            .child(
                Icon::new(IconName::Terminal)
                    .size(IconSize::XSmall)
                    .color(icon_color),
            )
            .child(
                Label::new(SharedString::from(agent.name.clone()))
                    .size(LabelSize::Small)
                    .when(phase == AgentPhase::Running, |label| {
                        label.color(Color::Default)
                    })
                    .when(phase != AgentPhase::Running, |label| {
                        label.color(Color::Muted)
                    }),
            )
            .when_some(activity_label, |el, label| {
                el.child(
                    Label::new(label)
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                )
            })
            .into_any_element()
    }

    fn deploy_scion_context_menu(
        &mut self,
        position: Point<Pixels>,
        agent_name: String,
        is_running: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.workspace.clone();
        let store = self.scion_store.clone();

        let name_for_logs = agent_name.clone();
        let name_for_sync = agent_name.clone();
        let name_for_stop = agent_name;

        let store_for_logs = store.clone();
        let store_for_sync = store.clone();
        let store_for_stop = store;

        let workspace_for_logs = workspace.clone();
        let workspace_for_sync = workspace.clone();
        let workspace_for_stop = workspace;

        let context_menu = ContextMenu::build(window, cx, move |menu, _window, _cx| {
            menu.entry(
                "View Logs",
                None,
                move |window, cx| {
                    if let (Some(store), Some(workspace)) =
                        (store_for_logs.as_ref(), workspace_for_logs.upgrade())
                    {
                        let cli = store.read(cx).cli.clone();
                        Self::open_agent_logs(
                            name_for_logs.clone(),
                            cli,
                            workspace,
                            window,
                            cx,
                        );
                    }
                },
            )
            .entry(
                "Sync Changes",
                None,
                move |_window, cx| {
                    if let (Some(store), Some(workspace)) =
                        (store_for_sync.as_ref(), workspace_for_sync.upgrade())
                    {
                        Self::sync_from_agent(
                            name_for_sync.clone(),
                            store.clone(),
                            workspace,
                            cx,
                        );
                    }
                },
            )
            .when(is_running, |menu| {
                menu.separator().entry(
                    "Stop Agent",
                    None,
                    move |_window, cx| {
                        if let (Some(store), Some(workspace)) =
                            (store_for_stop.as_ref(), workspace_for_stop.upgrade())
                        {
                            Self::stop_agent(
                                name_for_stop.clone(),
                                store.clone(),
                                workspace,
                                cx,
                            );
                        }
                    },
                )
            })
        });

        window.focus(&context_menu.focus_handle(cx), cx);
        let subscription = cx.subscribe(&context_menu, |this, _, _: &DismissEvent, cx| {
            this.scion_context_menu.take();
            cx.notify();
        });
        self.scion_context_menu = Some((context_menu, position, subscription));
        cx.notify();
    }

    fn open_agent_logs(
        name: String,
        cli: Arc<ScionCli>,
        workspace: Entity<Workspace>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let tab_title = format!("Scion Logs: {name}");
        let stream_name = name.clone();
        let stream_cli = cli.clone();

        let log_task =
            Tokio::spawn_result(cx, async move { cli.agent_logs(&name, Some(200)).await });

        // PaddleBoard: spawn `scion logs -f` for live streaming. The child
        // is started eagerly so it's ready when the buffer opens. Lines are
        // read on the tokio runtime and sent to the foreground via a channel.
        let (line_tx, line_rx) = async_channel::bounded::<String>(64);
        let _stream_reader = Tokio::spawn_result(cx, {
            let cli = stream_cli;
            let name = stream_name;
            async move {
                let mut child = cli.stream_logs(&name)?;
                let stdout = child
                    .stdout
                    .take()
                    .ok_or_else(|| anyhow::anyhow!("stdout not piped"))?;
                use tokio::io::{AsyncBufReadExt, BufReader};
                let mut reader = BufReader::new(stdout);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) => break,
                        Ok(_) => {
                            if line_tx.send(line.clone()).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                Ok::<(), anyhow::Error>(())
            }
        });

        let window_handle = window.window_handle();
        let project = workspace.read(cx).project().clone();

        cx.spawn(async move |cx| {
            let initial_logs = log_task.await.unwrap_or_default();

            let create_buffer =
                project.update(cx, |project, cx| project.create_buffer(None, false, cx));

            let buffer = match create_buffer.await {
                Ok(buffer) => buffer,
                Err(err) => {
                    log::error!("failed to create log buffer: {err:#}");
                    return;
                }
            };

            buffer.update(cx, |buffer, cx| {
                if !initial_logs.is_empty() {
                    buffer.edit([(0..0, initial_logs)], None, cx);
                }
                buffer.set_capability(language::Capability::ReadOnly, cx);
            });

            let opened = cx.update_window(window_handle, |_view, window, cx| {
                let multibuffer = cx.new(|cx| {
                    MultiBuffer::singleton(buffer.clone(), cx).with_title(tab_title)
                });
                let editor_entity = cx.new(|cx| {
                    let mut editor_view =
                        editor::Editor::for_multibuffer(multibuffer, None, window, cx);
                    editor_view.set_read_only(true);
                    editor_view
                });
                workspace.update(cx, |workspace, cx| {
                    workspace.add_item_to_active_pane(
                        Box::new(editor_entity),
                        None,
                        true,
                        window,
                        cx,
                    );
                });
            });

            if opened.is_err() {
                return;
            }

            // PaddleBoard: receive streamed lines and append to the buffer.
            // Stops when the channel closes (child exited) or buffer drops.
            let weak_buffer = buffer.downgrade();
            while let Ok(line) = line_rx.recv().await {
                let result = weak_buffer.update(cx, |buffer, cx| {
                    let len = buffer.len();
                    buffer.edit([(len..len, line.as_str())], None, cx);
                });
                if result.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn sync_from_agent(
        name: String,
        store: Entity<ScionStore>,
        workspace: Entity<Workspace>,
        cx: &mut App,
    ) {
        let task = store.update(cx, |store, cx| store.sync_from(name.clone(), cx));

        let store_for_refresh = store.clone();
        cx.spawn(async move |cx| {
            match task.await {
                Ok(_output) => {
                    store_for_refresh.update(cx, |store, cx| store.refresh(cx));
                    workspace.update(cx, |workspace, cx| {
                        workspace.show_toast(
                            Toast::new(
                                NotificationId::unique::<ScionStore>(),
                                format!("Synced changes from {name}"),
                            )
                            .autohide(),
                            cx,
                        );
                    });
                }
                Err(err) => {
                    workspace.update(cx, |workspace, cx| {
                        workspace.show_error(err, cx);
                    });
                }
            }
        })
        .detach();
    }

    fn stop_agent(
        name: String,
        store: Entity<ScionStore>,
        workspace: Entity<Workspace>,
        cx: &mut App,
    ) {
        let task = store.update(cx, |store, cx| store.stop_agent(name, cx));

        let store_for_refresh = store.clone();
        cx.spawn(async move |cx| {
            match task.await {
                Ok(()) => {
                    store_for_refresh.update(cx, |store, cx| store.refresh(cx));
                }
                Err(err) => {
                    workspace.update(cx, |workspace, cx| {
                        workspace.show_error(err, cx);
                    });
                }
            }
        })
        .detach();
    }
}

impl Render for OrchestrationPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // PaddleBoard: render Scion section alongside native threads
        let scion_section = self.render_scion_section(cx);
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
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .child(self.render_thread_tree(cx))
                    .when_some(scion_section, |el, section| el.child(section)),
            )
            .children(self.scion_context_menu.as_ref().map(|(menu, position, _)| {
                gpui::deferred(
                    gpui::anchored()
                        .position(*position)
                        .child(menu.clone()),
                )
                .with_priority(1)
            }))
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
