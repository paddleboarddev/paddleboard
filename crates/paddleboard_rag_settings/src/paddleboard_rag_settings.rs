// PaddleBoard: typed wrapper + registration for the local RAG / semantic-search
// setting. The deserializable schema lives in
// `settings_content::PaddleboardRagContent`.

use gpui::App;
use settings::{RegisterSetting, Settings};

/// Force-link the crate so the `RegisterSetting` inventory entry for
/// [`RagSettings`] is reachable.
pub fn init(_cx: &mut App) {}

#[derive(Debug, Clone, Default, PartialEq, RegisterSetting)]
pub struct RagSettings {
    /// Whether local semantic search is active. Defaults to `false`; when
    /// enabled, the `semantic_search` tool is exposed to agents and indexes the
    /// project on demand using the built-in embedding model.
    pub enabled: bool,
    /// Which vector store backs the index.
    pub store: RagStoreSetting,
}

/// Resolved store backend. The pgvector variant carries the env-var *name* for
/// the connection string, not the string itself — the caller resolves it at run
/// time so no secret lives in settings.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum RagStoreSetting {
    #[default]
    Local,
    Pgvector {
        url_env: String,
        table_prefix: Option<String>,
        ssl: bool,
    },
}

impl Settings for RagSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let Some(content) = content.paddleboard_rag.as_ref() else {
            return Self::default();
        };
        let store = match content.store_backend.as_deref() {
            Some("pgvector") => RagStoreSetting::Pgvector {
                url_env: content.store_url_env.clone().unwrap_or_default(),
                table_prefix: content.store_table_prefix.clone(),
                ssl: content.store_ssl.unwrap_or(true),
            },
            _ => RagStoreSetting::Local,
        };
        Self {
            enabled: content.enabled.unwrap_or(false),
            store,
        }
    }
}
