//! Bring-your-own vector store: a user-provided Postgres + pgvector database.
//!
//! Compiled only under the `pgvector` cargo feature. Uses the **synchronous**
//! `postgres` client (it owns an internal runtime), so it blocks on the same
//! background thread as the sqlite store — no async/tokio plumbing in the
//! smol-based app. Similarity runs **server-side** via an hnsw index and the
//! `<=>` cosine operator, which is the whole point of this tier: ANN at corpus
//! scale, versus the local store's in-RAM brute force.
//!
//! Embeddings are still computed locally by EmbeddingGemma; only the vectors and
//! chunk text are sent to the user's own database.

use std::cell::RefCell;

use anyhow::{Context as _, Result};
use collections::{HashMap, HashSet};

use crate::chunk::Chunk;
use crate::store::ScoredChunk;

/// pgvector column dimensionality. Must match the embedding model's output —
/// EmbeddingGemma-300M emits 768-dim vectors. Changing the model means a schema
/// migration on the remote store.
const EMBEDDING_DIM: usize = 768;

pub struct PgStore {
    // `postgres::Client` needs `&mut self` per call; the store is used from a
    // single background task, so a `RefCell` (Send, not Sync) is enough and
    // keeps the `&self` method shape shared with `LocalStore`.
    client: RefCell<postgres::Client>,
    files_table: String,
    chunks_table: String,
}

impl PgStore {
    pub fn connect(url: &str, table_prefix: Option<&str>, ssl: bool) -> Result<Self> {
        let prefix = sanitize_prefix(table_prefix)?;
        let files_table = format!("{prefix}rag_files");
        let chunks_table = format!("{prefix}rag_chunks");

        let mut client = if ssl {
            let connector =
                postgres_native_tls::MakeTlsConnector::new(native_tls::TlsConnector::new()?);
            postgres::Client::connect(url, connector)
                .context("failed to connect to the pgvector database")?
        } else {
            postgres::Client::connect(url, postgres::NoTls)
                .context("failed to connect to the pgvector database")?
        };

        client
            .batch_execute(&migrations(&files_table, &chunks_table))
            .context("failed to initialize the pgvector schema (is the `vector` extension available?)")?;

        Ok(Self {
            client: RefCell::new(client),
            files_table,
            chunks_table,
        })
    }

    pub fn file_hashes(&self, root: &str) -> Result<HashMap<String, String>> {
        let mut client = self.client.borrow_mut();
        let rows = client.query(
            &format!(
                "SELECT rel_path, content_hash FROM {} WHERE worktree_root = $1",
                self.files_table
            ),
            &[&root],
        )?;
        Ok(rows
            .iter()
            .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1)))
            .collect())
    }

    pub fn replace_file(
        &self,
        root: &str,
        rel_path: &str,
        content_hash: &str,
        chunks: &[(Chunk, Vec<f32>)],
        indexed_at: i64,
    ) -> Result<()> {
        let mut client = self.client.borrow_mut();
        let mut transaction = client.transaction()?;

        transaction.execute(
            &format!(
                "DELETE FROM {} WHERE worktree_root = $1 AND rel_path = $2",
                self.chunks_table
            ),
            &[&root, &rel_path],
        )?;
        transaction.execute(
            &format!(
                "INSERT INTO {} (worktree_root, rel_path, content_hash, indexed_at) \
                 VALUES ($1, $2, $3, $4) \
                 ON CONFLICT (worktree_root, rel_path) \
                 DO UPDATE SET content_hash = EXCLUDED.content_hash, indexed_at = EXCLUDED.indexed_at",
                self.files_table
            ),
            &[&root, &rel_path, &content_hash, &indexed_at],
        )?;

        let insert = transaction.prepare(&format!(
            "INSERT INTO {} \
             (worktree_root, rel_path, start_byte, end_byte, start_line, text, embedding) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            self.chunks_table
        ))?;
        for (chunk, embedding) in chunks {
            anyhow::ensure!(
                embedding.len() == EMBEDDING_DIM,
                "embedding dimension {} does not match the pgvector column vector({EMBEDDING_DIM})",
                embedding.len()
            );
            let vector = pgvector::Vector::from(embedding.clone());
            transaction.execute(
                &insert,
                &[
                    &root,
                    &rel_path,
                    &(chunk.start_byte as i64),
                    &(chunk.end_byte as i64),
                    &(chunk.start_line as i64),
                    &chunk.text,
                    &vector,
                ],
            )?;
        }

        transaction.commit()?;
        Ok(())
    }

    pub fn prune_missing(&self, root: &str, present: &HashSet<String>) -> Result<()> {
        let mut client = self.client.borrow_mut();
        let stored: Vec<String> = client
            .query(
                &format!(
                    "SELECT rel_path FROM {} WHERE worktree_root = $1",
                    self.files_table
                ),
                &[&root],
            )?
            .iter()
            .map(|row| row.get::<_, String>(0))
            .collect();

        let mut transaction = client.transaction()?;
        for rel_path in stored {
            if !present.contains(&rel_path) {
                transaction.execute(
                    &format!(
                        "DELETE FROM {} WHERE worktree_root = $1 AND rel_path = $2",
                        self.chunks_table
                    ),
                    &[&root, &rel_path],
                )?;
                transaction.execute(
                    &format!(
                        "DELETE FROM {} WHERE worktree_root = $1 AND rel_path = $2",
                        self.files_table
                    ),
                    &[&root, &rel_path],
                )?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn search(&self, root: &str, query: &[f32], limit: usize) -> Result<Vec<ScoredChunk>> {
        let mut client = self.client.borrow_mut();
        let vector = pgvector::Vector::from(query.to_vec());
        let limit = limit as i64;
        // `<=>` is pgvector's cosine distance; `1 - distance` is cosine
        // similarity, matching the local store's normalized dot-product score.
        // The hnsw index serves the ORDER BY, so this is an ANN query.
        let rows = client.query(
            &format!(
                "SELECT rel_path, start_line, text, 1 - (embedding <=> $1) AS score \
                 FROM {} WHERE worktree_root = $2 \
                 ORDER BY embedding <=> $1 LIMIT $3",
                self.chunks_table
            ),
            &[&vector, &root, &limit],
        )?;
        Ok(rows
            .iter()
            .map(|row| ScoredChunk {
                rel_path: row.get::<_, String>(0),
                start_line: row.get::<_, i64>(1).max(0) as usize,
                text: row.get::<_, String>(2),
                score: row.get::<_, f64>(3) as f32,
            })
            .collect())
    }
}

fn migrations(files_table: &str, chunks_table: &str) -> String {
    format!(
        "CREATE EXTENSION IF NOT EXISTS vector;
         CREATE TABLE IF NOT EXISTS {files_table} (
             worktree_root text NOT NULL,
             rel_path text NOT NULL,
             content_hash text NOT NULL,
             indexed_at bigint NOT NULL,
             PRIMARY KEY (worktree_root, rel_path)
         );
         CREATE TABLE IF NOT EXISTS {chunks_table} (
             id bigserial PRIMARY KEY,
             worktree_root text NOT NULL,
             rel_path text NOT NULL,
             start_byte bigint NOT NULL,
             end_byte bigint NOT NULL,
             start_line bigint NOT NULL,
             text text NOT NULL,
             embedding vector({EMBEDDING_DIM}) NOT NULL
         );
         CREATE INDEX IF NOT EXISTS {chunks_table}_by_file ON {chunks_table} (worktree_root, rel_path);
         CREATE INDEX IF NOT EXISTS {chunks_table}_hnsw ON {chunks_table} USING hnsw (embedding vector_cosine_ops);"
    )
}

/// Table names are interpolated into SQL (identifiers can't be parameterized),
/// so a caller-supplied prefix must be a strict identifier. Returns the prefix
/// with a trailing `_`, or empty.
fn sanitize_prefix(table_prefix: Option<&str>) -> Result<String> {
    match table_prefix {
        None | Some("") => Ok(String::new()),
        Some(prefix)
            if prefix
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_') =>
        {
            Ok(format!("{prefix}_"))
        }
        Some(prefix) => anyhow::bail!(
            "invalid table_prefix {prefix:?}: only ASCII letters, digits, and underscores are allowed"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_sanitization() {
        assert_eq!(sanitize_prefix(None).unwrap(), "");
        assert_eq!(sanitize_prefix(Some("")).unwrap(), "");
        assert_eq!(sanitize_prefix(Some("team42")).unwrap(), "team42_");
        assert!(sanitize_prefix(Some("bad-name")).is_err());
        assert!(sanitize_prefix(Some("drop;table")).is_err());
    }

    #[test]
    fn migrations_reference_both_tables_and_dim() {
        let sql = migrations("p_rag_files", "p_rag_chunks");
        assert!(sql.contains("vector(768)"));
        assert!(sql.contains("USING hnsw"));
        assert!(sql.contains("p_rag_files"));
        assert!(sql.contains("p_rag_chunks_hnsw"));
    }
}
