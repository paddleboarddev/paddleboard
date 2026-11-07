use std::sync::Arc;

use gpui::{
    ClickEvent, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, MouseDownEvent,
    WeakEntity,
};
use ui::{
    ToggleButtonGroup, ToggleButtonGroupSize, ToggleButtonGroupStyle, ToggleButtonSimple, Tooltip,
    prelude::*,
};
use workspace::{ModalView, Workspace};

use crate::catalog::{Catalog, CatalogGlobal};

pub(crate) mod add_agent_modal;
pub(crate) mod add_skill_modal;
mod agents_tab;
pub(crate) mod build_mcp_modal;
mod mcp_tab;
mod personas_tab;
mod skills_tab;
mod usage_tab;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiDockTab {
    Agents,
    Skills,
    Personas,
    Mcp,
    Usage,
}

pub struct AiDock {
    focus_handle: FocusHandle,
    workspace: WeakEntity<Workspace>,
    tab: AiDockTab,
    catalog: Arc<Catalog>,
    mcp_view: Option<Entity<agent_ui::McpServersView>>,
    // Cached so rendering the Usage tab never touches disk; refreshed on
    // tab-switch and via the tab's refresh button.
    usage_summary: Option<paddleboard_usage::UsageSummary>,
    expanded: bool,
}

impl AiDock {
    /// Opens the AI Dock on `tab`, or switches an already-open dock to it.
    ///
    /// Deliberately not `toggle_modal` on its own. That helper *dismisses* when a
    /// modal of the same type is already active, and returns silently when the
    /// active modal declines to close (`DismissDecision::Dismiss(false)` or
    /// `Pending`). Either way the user gets no modal, no error and no log line —
    /// which is exactly the report in #200: a long-lived session answers
    /// `ai dock: open` by doing nothing visible.
    ///
    /// Every action routed here is named `Open*`, so opening is what they should do.
    /// Checking for a live dock first means the outcome no longer depends on the
    /// modal layer's view of the world being accurate.
    pub fn open(
        workspace: &mut Workspace,
        tab: AiDockTab,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        if let Some(dock) = workspace.active_modal::<AiDock>(cx) {
            dock.update(cx, |dock, cx| {
                dock.switch_tab(tab, window, cx);
                dock.focus_handle.focus(window, cx);
            });
            return;
        }

        let weak_workspace = workspace.weak_handle();
        workspace.toggle_modal(window, cx, |_window, cx| AiDock {
            focus_handle: cx.focus_handle(),
            workspace: weak_workspace,
            tab,
            catalog: CatalogGlobal::get(cx),
            mcp_view: None,
            usage_summary: None,
            expanded: false,
        });

        // PaddleBoard: one silent path remains. If another modal is active and
        // declines to close, `toggle_modal` returns without opening the dock. Say
        // so in the log, so a future "the dock didn't open" report has a cause.
        if workspace.active_modal::<AiDock>(cx).is_none() {
            log::warn!("AI Dock did not open on {tab:?}: the active modal declined to close");
        }
    }

    fn refresh_usage(&mut self, cx: &mut Context<Self>) {
        self.usage_summary = Some(paddleboard_usage::summary());
        cx.notify();
    }

    fn toggle_expanded(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.expanded = !self.expanded;
        cx.notify();
    }

    fn cancel(&mut self, _: &menu::Cancel, _window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }

    fn switch_tab(&mut self, tab: AiDockTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab == tab {
            return;
        }
        self.tab = tab;
        if matches!(tab, AiDockTab::Mcp) {
            self.ensure_mcp_view(window, cx);
        }
        if matches!(tab, AiDockTab::Usage) {
            self.refresh_usage(cx);
        }
        cx.notify();
    }

    fn ensure_mcp_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mcp_view.is_some() {
            return;
        }
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let view = workspace.update(cx, |workspace, cx| {
            agent_ui::McpServersView::new(workspace, window, cx)
        });
        view.update(cx, |view, _| view.set_compact(true));
        self.mcp_view = Some(view);
    }

    fn render_tab_switcher(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected_index = match self.tab {
            AiDockTab::Agents => 0,
            AiDockTab::Skills => 1,
            AiDockTab::Personas => 2,
            AiDockTab::Mcp => 3,
            AiDockTab::Usage => 4,
        };

        ToggleButtonGroup::single_row(
            "ai-dock-tab-switcher",
            [
                ToggleButtonSimple::new(
                    "Agents",
                    cx.listener(|this, _event, window, cx| {
                        this.switch_tab(AiDockTab::Agents, window, cx);
                    }),
                ),
                ToggleButtonSimple::new(
                    "Skills",
                    cx.listener(|this, _event, window, cx| {
                        this.switch_tab(AiDockTab::Skills, window, cx);
                    }),
                ),
                ToggleButtonSimple::new(
                    "Personas",
                    cx.listener(|this, _event, window, cx| {
                        this.switch_tab(AiDockTab::Personas, window, cx);
                    }),
                ),
                ToggleButtonSimple::new(
                    "MCP Servers",
                    cx.listener(|this, _event, window, cx| {
                        this.switch_tab(AiDockTab::Mcp, window, cx);
                    }),
                ),
                ToggleButtonSimple::new(
                    "Usage",
                    cx.listener(|this, _event, window, cx| {
                        this.switch_tab(AiDockTab::Usage, window, cx);
                    }),
                ),
            ],
        )
        .style(ToggleButtonGroupStyle::Outlined)
        .size(ToggleButtonGroupSize::Medium)
        .selected_index(selected_index)
        .into_any_element()
    }

    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let counts = (
            self.catalog.agents.len(),
            self.catalog.skills.len(),
            self.catalog.mcp_servers.len(),
        );
        let persona_count = self.catalog.personas.len();
        let (expand_icon, expand_tooltip) = if self.expanded {
            (IconName::Minimize, "Collapse")
        } else {
            (IconName::Maximize, "Expand")
        };
        h_flex()
            .w_full()
            .justify_between()
            .gap_2()
            .child(
                v_flex()
                    .gap_0p5()
                    .child(Headline::new("AI Dock").size(HeadlineSize::Small))
                    .child(
                        Label::new(format!(
                            "{} agents · {} skills · {} personas · {} MCP servers",
                            counts.0, counts.1, persona_count, counts.2
                        ))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        IconButton::new("ai-dock-expand", expand_icon)
                            .tooltip(Tooltip::text(expand_tooltip))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.toggle_expanded(window, cx);
                            })),
                    )
                    .child(
                        IconButton::new("ai-dock-close", IconName::Close)
                            .tooltip(Tooltip::text("Close"))
                            .on_click(cx.listener(|_, _: &ClickEvent, _window, cx| {
                                cx.emit(DismissEvent);
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_tab_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.tab {
            AiDockTab::Agents => agents_tab::render(self, cx).into_any_element(),
            AiDockTab::Skills => skills_tab::render(self, cx).into_any_element(),
            AiDockTab::Personas => personas_tab::render(self, cx).into_any_element(),
            AiDockTab::Mcp => mcp_tab::render(self, window, cx),
            AiDockTab::Usage => usage_tab::render(self, cx).into_any_element(),
        }
    }
}

impl EventEmitter<DismissEvent> for AiDock {}

impl Focusable for AiDock {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ModalView for AiDock {}

impl Render for AiDock {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Make sure the MCP view exists if we opened directly to MCP.
        if matches!(self.tab, AiDockTab::Mcp) && self.mcp_view.is_none() {
            self.ensure_mcp_view(window, cx);
        }
        // Likewise, load usage if we opened directly to the Usage tab.
        if matches!(self.tab, AiDockTab::Usage) && self.usage_summary.is_none() {
            self.refresh_usage(cx);
        }

        let header = self.render_header(cx);
        let tab_switcher = self.render_tab_switcher(cx);
        let body = self.render_tab_body(window, cx);

        let (width, height) = if self.expanded {
            (rems(80.), rems(54.))
        } else {
            (rems(56.), rems(36.))
        };

        v_flex()
            .id("ai-dock")
            .key_context("AiDock")
            .elevation_3(cx)
            .w(width)
            .h(height)
            .track_focus(&self.focus_handle(cx))
            .on_action(cx.listener(Self::cancel))
            .on_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                this.focus_handle.focus(window, cx);
            }))
            .child(
                v_flex()
                    .p_4()
                    .gap_3()
                    .border_b_1()
                    .border_color(cx.theme().colors().border_variant)
                    .child(header)
                    .child(tab_switcher),
            )
            .child(
                div()
                    .id("ai-dock-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(body),
            )
    }
}

#[cfg(test)]
mod tests {
    use gpui::{TestAppContext, VisualTestContext};
    use paddleboard_actions::{McpServers, ai_dock};
    use project::Project;
    use workspace::{AppState, MultiWorkspace};

    use super::*;

    fn init_test(cx: &mut TestAppContext) -> Arc<AppState> {
        cx.update(|cx| {
            let app_state = AppState::test(cx);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
            workspace::init(app_state.clone(), cx);
            // The Agents tab reads the global client when it renders.
            client::Client::set_global(app_state.client.clone(), cx);
            crate::init(cx);
            app_state
        })
    }

    async fn open_workspace(
        cx: &mut TestAppContext,
    ) -> (Entity<Workspace>, &mut VisualTestContext) {
        let app_state = init_test(cx);
        let project = Project::test(app_state.fs.clone(), [], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
        let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());
        (workspace, cx)
    }

    fn dock_tab(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Option<AiDockTab> {
        workspace.update(cx, |workspace, cx| {
            workspace
                .active_modal::<AiDock>(cx)
                .map(|dock| dock.read(cx).tab)
        })
    }

    /// A stand-in for the command palette: some other modal that is active when
    /// a dock action runs, and that closes when asked.
    struct OtherModal(FocusHandle);

    impl EventEmitter<DismissEvent> for OtherModal {}
    impl ModalView for OtherModal {}

    impl Focusable for OtherModal {
        fn focus_handle(&self, _cx: &App) -> FocusHandle {
            self.0.clone()
        }
    }

    impl Render for OtherModal {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().track_focus(&self.focus_handle(cx))
        }
    }

    // #112 (was #200): running a dock action while the dock was already the
    // active modal dismissed it, so `ai dock: open` looked like it did nothing.
    #[gpui::test]
    async fn test_dock_actions_never_dismiss_an_open_dock(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace(cx).await;

        cx.dispatch_action(ai_dock::Open);
        cx.run_until_parked();
        assert_eq!(dock_tab(&workspace, cx), Some(AiDockTab::Agents));

        // The same action again keeps the dock open.
        cx.dispatch_action(ai_dock::Open);
        cx.run_until_parked();
        assert_eq!(dock_tab(&workspace, cx), Some(AiDockTab::Agents));

        // A different dock action switches the tab instead of closing.
        cx.dispatch_action(ai_dock::OpenPersonas);
        cx.run_until_parked();
        assert_eq!(dock_tab(&workspace, cx), Some(AiDockTab::Personas));

        cx.dispatch_action(ai_dock::Open);
        cx.run_until_parked();
        assert_eq!(dock_tab(&workspace, cx), Some(AiDockTab::Agents));
    }

    // The path from the command palette: another modal is active when the action
    // runs, and the dock replaces it.
    #[gpui::test]
    async fn test_dock_opens_over_another_modal(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace(cx).await;

        workspace.update_in(cx, |workspace, window, cx| {
            workspace.toggle_modal(window, cx, |_window, cx| OtherModal(cx.focus_handle()));
        });
        cx.run_until_parked();
        workspace.update(cx, |workspace, cx| {
            assert!(workspace.active_modal::<OtherModal>(cx).is_some());
        });

        cx.dispatch_action(McpServers);
        cx.run_until_parked();
        assert_eq!(dock_tab(&workspace, cx), Some(AiDockTab::Mcp));
        workspace.update(cx, |workspace, cx| {
            assert!(workspace.active_modal::<OtherModal>(cx).is_none());
        });
    }

    // Closing the dock and opening it again works, however many times.
    #[gpui::test]
    async fn test_dock_reopens_after_dismiss(cx: &mut TestAppContext) {
        let (workspace, cx) = open_workspace(cx).await;

        for _ in 0..3 {
            cx.dispatch_action(ai_dock::OpenPersonas);
            cx.run_until_parked();
            assert_eq!(dock_tab(&workspace, cx), Some(AiDockTab::Personas));

            workspace.update_in(cx, |workspace, window, cx| {
                workspace.hide_modal(window, cx);
            });
            cx.run_until_parked();
            assert_eq!(dock_tab(&workspace, cx), None);
        }
    }
}
