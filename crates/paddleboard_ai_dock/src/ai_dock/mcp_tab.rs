use gpui::AnyElement;
use ui::prelude::*;

use crate::ai_dock::AiDock;

pub(super) fn render(
    modal: &mut AiDock,
    _window: &mut Window,
    _cx: &mut Context<AiDock>,
) -> AnyElement {
    // The MCP tab hosts an absorbed `agent_ui::McpServersView` — the same
    // surface that lived as a standalone workspace pane item before the
    // AI Dock consolidation. The view is created lazily by
    // `AiDock::ensure_mcp_view` when the user first switches to this tab (or
    // when the dock opens directly to MCP via the legacy
    // `paddleboard_actions::McpServers` action).
    match modal.mcp_view.as_ref() {
        Some(view) => div()
            .size_full()
            .child(view.clone())
            .into_any_element(),
        None => v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .child(
                Label::new("Loading MCP servers…")
                    .color(Color::Muted)
                    .size(LabelSize::Small),
            )
            .into_any_element(),
    }
}
