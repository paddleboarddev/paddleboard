// PaddleBoard: settings schema for local RAG / semantic search. Lives in
// settings_content so the field deserializes like any other Zed setting; the
// typed wrapper + init lives in `paddleboard_rag_settings` to keep this file's
// drift surface small.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings_macros::{MergeFrom, with_fallible_options};

#[with_fallible_options]
#[derive(Debug, Default, PartialEq, Clone, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct PaddleboardRagContent {
    /// Whether local semantic search is enabled. When `true`, PaddleBoard
    /// exposes the `semantic_search` tool to agents, which indexes the current
    /// project on demand with the built-in local embedding model
    /// (EmbeddingGemma) and answers natural-language queries entirely on-device.
    ///
    /// Default: false
    pub enabled: Option<bool>,
}
