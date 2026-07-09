//! Store-backend selection for the RAG index.
//!
//! [`StoreConfig`] is the backend-agnostic request (built from settings by the
//! caller). [`Backend`] is the opened store the indexer talks to. Both backends
//! are synchronous/blocking — the sqlite one via `sqlez`, the pgvector one via
//! the sync `postgres` client — so they share the existing background-thread
//! path with no async plumbing.
//!
//! The pgvector variant of `StoreConfig` exists regardless of the `pgvector`
//! cargo feature, so a build compiled without the feature can detect that the
//! user asked for it and return a clear error instead of silently falling back.

use anyhow::Result;
use collections::{HashMap, HashSet};

use crate::chunk::Chunk;
use crate::store::{LocalStore, ScoredChunk};

/// Which store to use, resolved from settings by the caller (the connection
/// string is already resolved from its env var, not a var name).
#[derive(Debug, Clone)]
pub enum StoreConfig {
    /// Local sqlite + in-RAM cosine under `paths::embeddings_dir()` (default).
    Local,
    /// Bring-your-own Postgres + pgvector.
    Pgvector {
        /// Full libpq connection string (already resolved from its env var).
        url: String,
        /// Optional identifier prefix so several projects/tenants can share one
        /// database without table collisions.
        table_prefix: Option<String>,
        /// Whether to negotiate TLS (native-tls). Off only for trusted local
        /// links such as the Cloud SQL Auth Proxy on loopback.
        ssl: bool,
    },
}

/// An opened store. Methods mirror `LocalStore` so the indexer is
/// backend-agnostic.
pub enum Backend {
    Local(LocalStore),
    #[cfg(feature = "pgvector")]
    Pgvector(crate::pgvector_store::PgStore),
}

impl Backend {
    pub fn open(config: StoreConfig) -> Result<Self> {
        match config {
            StoreConfig::Local => Ok(Backend::Local(LocalStore::new()?)),
            StoreConfig::Pgvector {
                url,
                table_prefix,
                ssl,
            } => {
                #[cfg(feature = "pgvector")]
                {
                    Ok(Backend::Pgvector(crate::pgvector_store::PgStore::connect(
                        &url,
                        table_prefix.as_deref(),
                        ssl,
                    )?))
                }
                #[cfg(not(feature = "pgvector"))]
                {
                    let _ = (url, table_prefix, ssl);
                    anyhow::bail!(
                        "semantic search is configured to use a pgvector store, but this build \
                         of PaddleBoard was compiled without pgvector support. Use the local \
                         store or an official build."
                    )
                }
            }
        }
    }

    pub fn file_hashes(&self, root: &str) -> Result<HashMap<String, String>> {
        match self {
            Backend::Local(store) => store.file_hashes(root),
            #[cfg(feature = "pgvector")]
            Backend::Pgvector(store) => store.file_hashes(root),
        }
    }

    pub fn replace_file(
        &self,
        root: &str,
        rel_path: &str,
        content_hash: &str,
        chunks: &[(Chunk, Vec<f32>)],
        indexed_at: i64,
    ) -> Result<()> {
        match self {
            Backend::Local(store) => {
                store.replace_file(root, rel_path, content_hash, chunks, indexed_at)
            }
            #[cfg(feature = "pgvector")]
            Backend::Pgvector(store) => {
                store.replace_file(root, rel_path, content_hash, chunks, indexed_at)
            }
        }
    }

    pub fn prune_missing(&self, root: &str, present: &HashSet<String>) -> Result<()> {
        match self {
            Backend::Local(store) => store.prune_missing(root, present),
            #[cfg(feature = "pgvector")]
            Backend::Pgvector(store) => store.prune_missing(root, present),
        }
    }

    pub fn search(&self, root: &str, query: &[f32], limit: usize) -> Result<Vec<ScoredChunk>> {
        match self {
            Backend::Local(store) => store.search(root, query, limit),
            #[cfg(feature = "pgvector")]
            Backend::Pgvector(store) => store.search(root, query, limit),
        }
    }
}
