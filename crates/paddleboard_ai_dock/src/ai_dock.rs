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

mod agents_tab;
mod mcp_tab;
mod skills_tab;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiDockTab {
    Agents,
    Skills,
    Mcp,
}

pub struct AiDock {
    focus_handle: FocusHandle,
    workspace: WeakEntity<Workspace>,
    tab: AiDockTab,
    catalog: Arc<Catalog>,
    mcp_view: Option<Entity<agent_ui::McpServersView>>,
    expanded: bool,
}

impl AiDock {
    pub fn toggle(
        workspace: &mut Workspace,
        tab: AiDockTab,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let weak_workspace = workspace.weak_handle();
        workspace.toggle_modal(window, cx, |_window, cx| AiDock {
            focus_handle: cx.focus_handle(),
            workspace: weak_workspace,
            tab,
            catalog: CatalogGlobal::get(cx),
            mcp_view: None,
            expanded: false,
        });
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
        self.mcp_view = Some(view);
    }

    fn render_tab_switcher(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected_index = match self.tab {
            AiDockTab::Agents => 0,
            AiDockTab::Skills => 1,
            AiDockTab::Mcp => 2,
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
                    "MCP Servers",
                    cx.listener(|this, _event, window, cx| {
                        this.switch_tab(AiDockTab::Mcp, window, cx);
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
                    .child(Headline::new("AI Dock").size(HeadlineSize::Large))
                    .child(
                        Label::new(format!(
                            "{} agents · {} skills · {} MCP servers",
                            counts.0, counts.1, counts.2
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
            AiDockTab::Mcp => mcp_tab::render(self, window, cx),
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
