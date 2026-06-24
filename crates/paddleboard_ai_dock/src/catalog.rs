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
    /// Shell command to install the agent's CLI tool (e.g. `pip install google-adk`).
    /// When set, the Agents tab shows a terminal-based "Set Up" button instead of
    /// the registry-settings install path.
    #[serde(default)]
    pub install_command: Option<String>,
}

/// One skill entry. Skills are markdown files dropped into `.claude/commands/`
/// (per-project) or `~/.claude/commands/` (per-user). Bundled skills carry
/// the markdown body inline so "Add" works without a network round-trip.
///
/// Skills with `builtin: true` are provided by the Claude Code harness and
/// are always available — they don't need a `.claude/commands/` file.
#[derive(Debug, Clone, Deserialize)]
pub struct SkillEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub featured: bool,
    #[serde(default)]
    pub builtin: bool,
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

/// Bundled markdown body for catalog skills we ship in-tree. Returning `Some`
/// turns on the Skills tab's "Add to project" / "Add to user" buttons for
/// that entry; returning `None` falls back to the homepage / disabled state.
///
/// The canonical files live under `assets/ai_dock/skills/` — a path that ships
/// in the public source snapshot. (They used to live under `.claude/commands/`,
/// which publish-public.sh strips, so fresh clones of the public repo failed to
/// compile.) The `.claude/commands/*.md` slash commands are symlinks to these
/// files, so the command used in this repo and the bundled install copy still
/// can't drift.
pub fn bundled_skill_content(id: &str) -> Option<&'static str> {
    match id {
        "build" => Some(include_str!("../../../assets/ai_dock/skills/build.md")),
        "update-tour" => Some(include_str!("../../../assets/ai_dock/skills/update-tour.md")),
        "clippy" => Some(include_str!("../../../assets/ai_dock/skills/clippy.md")),
        "test" => Some(include_str!("../../../assets/ai_dock/skills/test.md")),
        "check-drift" => Some(include_str!("../../../assets/ai_dock/skills/check-drift.md")),
        "build-mcp" => Some(include_str!("../../../assets/ai_dock/skills/build-mcp.md")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_skill_content_returns_known_skills() {
        let build = bundled_skill_content("build").expect("build is bundled");
        assert!(
            build.contains("/build"),
            "bundled `build.md` should mention `/build`: got {build:?}"
        );

        let update_tour = bundled_skill_content("update-tour").expect("update-tour is bundled");
        assert!(
            update_tour.contains("WELCOME.md") || update_tour.contains("tour"),
            "bundled `update-tour.md` should reference WELCOME.md or `tour`: got {update_tour:?}"
        );

        let clippy = bundled_skill_content("clippy").expect("clippy is bundled");
        assert!(
            clippy.contains("script/clippy"),
            "bundled `clippy.md` should mention `script/clippy`: got {clippy:?}"
        );

        let test = bundled_skill_content("test").expect("test is bundled");
        assert!(
            test.contains("cargo test"),
            "bundled `test.md` should mention `cargo test`: got {test:?}"
        );

        let check_drift =
            bundled_skill_content("check-drift").expect("check-drift is bundled");
        assert!(
            check_drift.contains("check-upstream-drift"),
            "bundled `check-drift.md` should mention `check-upstream-drift`: got {check_drift:?}"
        );
    }

    #[test]
    fn bundled_skill_content_returns_none_for_unbundled() {
        assert!(bundled_skill_content("nonexistent").is_none());
    }

    #[test]
    fn builtin_skills_are_not_bundled() {
        let catalog = Catalog::load();
        for skill in &catalog.skills {
            if skill.builtin {
                assert!(
                    bundled_skill_content(&skill.id).is_none(),
                    "builtin skill `{}` should not have bundled content — \
                     it's provided by the harness, not a .claude/commands/ file",
                    skill.id
                );
            }
        }
    }

    #[test]
    fn every_bundled_id_is_in_catalog() {
        let catalog = Catalog::load();
        let bundled_ids = ["build", "update-tour", "clippy", "test", "check-drift"];
        for id in bundled_ids {
            assert!(
                catalog.skills.iter().any(|s| s.id == id),
                "bundled skill `{id}` must have a matching catalog entry; \
                 otherwise the install buttons never render"
            );
        }
    }
}
