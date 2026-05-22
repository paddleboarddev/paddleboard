use std::sync::Arc;

use gpui::{App, Global};
use serde::Deserialize;

/// In-repo catalog of installable agents, skills, and MCP servers. Loaded
/// once at startup from `assets/ai_dock/catalog.json` (embedded via
/// `include_str!`), so adding an entry means opening a PR.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Catalog {
    #[serde(default)]
    pub agents: Vec<AgentEntry>,
    #[serde(default)]
    pub skills: Vec<SkillEntry>,
    #[serde(default)]
    pub mcp_servers: Vec<McpEntry>,
}

/// One agent entry. `id` should match the corresponding `RegistryAgent` id
/// (so the Store can cross-reference `project::AgentRegistryStore` to learn
/// the install status without duplicating that logic).
#[derive(Debug, Clone, Deserialize)]
pub struct AgentEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub homepage: Option<String>,
    /// Pinned to the small "Featured" strip on the Welcome screen.
    #[serde(default)]
    pub featured: bool,
    /// Identifies the special-case Zed Agent. Renders the sign-in/plan flow
    /// instead of the generic agent-server install path.
    #[serde(default)]
    pub builtin_zed: bool,
}

/// One skill entry. Skills are markdown files dropped into `.claude/commands/`
/// (per-project) or `~/.claude/commands/` (per-user). Bundled skills carry
/// the markdown body inline so "Add" works without a network round-trip.
#[derive(Debug, Clone, Deserialize)]
pub struct SkillEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub featured: bool,
}

/// One MCP server entry. The AI Dock's MCP tab uses these to populate the
/// "Available" list; selecting one delegates to the absorbed
/// `McpServersView` (which knows how to actually register the server with
/// `context_server_store`).
#[derive(Debug, Clone, Deserialize)]
pub struct McpEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub featured: bool,
}

impl Catalog {
    /// Parse the bundled catalog. If parsing fails we fall back to an empty
    /// catalog and log; that keeps the AI Dock rendering instead of crashing
    /// the workspace on a malformed JSON edit.
    pub fn load() -> Self {
        const SOURCE: &str = include_str!("../../../assets/ai_dock/catalog.json");
        match serde_json::from_str::<Catalog>(SOURCE) {
            Ok(catalog) => catalog,
            Err(err) => {
                log::error!("paddleboard_ai_dock: failed to parse catalog.json: {err:#}");
                Catalog::default()
            }
        }
    }

    pub fn empty() -> Self {
        Self::default()
    }
}

#[derive(Clone)]
pub(crate) struct CatalogGlobal(pub(crate) Arc<Catalog>);

impl Global for CatalogGlobal {}

impl CatalogGlobal {
    pub(crate) fn get(cx: &App) -> Arc<Catalog> {
        cx.try_global::<CatalogGlobal>()
            .map(|g| g.0.clone())
            .unwrap_or_else(|| Arc::new(Catalog::empty()))
    }
}

pub fn catalog(cx: &App) -> Arc<Catalog> {
    CatalogGlobal::get(cx)
}
