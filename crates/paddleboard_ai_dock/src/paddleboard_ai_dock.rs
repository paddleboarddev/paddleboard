//! PaddleBoard AI Dock: a single browse-surface for Agents, Skills, and MCP
//! servers. Replaces the hardcoded "Agent Setup" row on the Welcome screen
//! and absorbs the standalone MCP Servers page.
//!
//! Entry point: the `paddleboard_actions::ai_dock::Open` action toggles an
//! `AiDock` on the active workspace. The dock owns three tab views and a
//! shared `Catalog` loaded once at startup from the in-repo JSON.

use std::sync::Arc;

use gpui::App;
use workspace::Workspace;

mod ai_dock;
pub mod catalog;

pub use ai_dock::{AiDock, AiDockTab};
pub use catalog::{AgentEntry, Catalog, McpEntry, SkillEntry};

/// Initialize the AI Dock: load the catalog into a global and wire the
/// `Open` action onto every workspace.
pub fn init(cx: &mut App) {
    let catalog = Arc::new(Catalog::load());
    cx.set_global(catalog::CatalogGlobal(catalog));

    cx.observe_new(|workspace: &mut Workspace, _window, _cx| {
        workspace.register_action(
            |workspace, _: &paddleboard_actions::ai_dock::Open, window, cx| {
                AiDock::toggle(workspace, AiDockTab::Agents, window, cx);
            },
        );
        workspace.register_action(
            |workspace, _: &paddleboard_actions::ai_dock::OpenPersonas, window, cx| {
                AiDock::toggle(workspace, AiDockTab::Personas, window, cx);
            },
        );
        workspace.register_action(
            |workspace, _: &paddleboard_actions::McpServers, window, cx| {
                AiDock::toggle(workspace, AiDockTab::Mcp, window, cx);
            },
        );
    })
    .detach();
}
