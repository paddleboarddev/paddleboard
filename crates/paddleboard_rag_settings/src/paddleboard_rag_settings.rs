// PaddleBoard: typed wrapper + registration for the local RAG / semantic-search
// setting. The deserializable schema lives in
// `settings_content::PaddleboardRagContent`.

use gpui::App;
use settings::{RegisterSetting, Settings};

/// Force-link the crate so the `RegisterSetting` inventory entry for
/// [`RagSettings`] is reachable.
pub fn init(_cx: &mut App) {}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, RegisterSetting)]
pub struct RagSettings {
    /// Whether local semantic search is active. Defaults to `false`; when
    /// enabled, the `semantic_search` tool is exposed to agents and indexes the
    /// project on demand using the built-in embedding model.
    pub enabled: bool,
}

impl Settings for RagSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let Some(content) = content.paddleboard_rag.as_ref() else {
            return Self::default();
        };
        Self {
            enabled: content.enabled.unwrap_or(false),
        }
    }
}
