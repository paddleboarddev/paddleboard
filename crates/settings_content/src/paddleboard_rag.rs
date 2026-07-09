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

    /// Which vector store backs the index: `"local"` (default) for the built-in
    /// on-device sqlite store, or `"pgvector"` for a bring-your-own Postgres +
    /// pgvector database. The pgvector tier sends your vectors and chunk text to
    /// your own database (embeddings are still computed on-device).
    pub store_backend: Option<String>,

    /// Name of the environment variable holding the libpq connection string for
    /// the pgvector store (e.g. `"PADDLEBOARD_RAG_PGVECTOR_URL"`). The
    /// connection string is read from the environment at run time and never
    /// stored in settings, matching PaddleBoard's "names not values" pattern.
    pub store_url_env: Option<String>,

    /// Optional table-name prefix (ASCII letters/digits/underscores) so several
    /// projects or tenants can share one pgvector database without collisions.
    pub store_table_prefix: Option<String>,

    /// Whether to negotiate TLS to the pgvector host. Default `true`; set
    /// `false` only for a trusted local link such as the Cloud SQL Auth Proxy.
    pub store_ssl: Option<bool>,
}
