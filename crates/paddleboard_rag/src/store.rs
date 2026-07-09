//! SQLite persistence for the RAG index, rooted at `paths::embeddings_dir()`.
//!
//! Uses `sqlez` directly (a plain `Connection`, which is `Send`) rather than the
//! shared app database, so the embedding BLOBs live under the reserved
//! embeddings directory. The store is stateless: each call opens a connection,
//! does its work, and drops it. SQLite's own file locking (WAL) serializes
//! concurrent access, so the `!Sync` connection never crosses a thread boundary.
//!
//! Rows are namespaced by `worktree_root` (a worktree's absolute path) so one
//! database can hold indexes for every project the user opens.

use std::path::PathBuf;

use anyhow::Result;
use collections::{HashMap, HashSet};
use sqlez::connection::Connection;

use crate::chunk::Chunk;

const CREATE_FILES: &str = "\
CREATE TABLE IF NOT EXISTS rag_files (
    worktree_root TEXT NOT NULL,
    rel_path TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    indexed_at INTEGER NOT NULL,
    PRIMARY KEY (worktree_root, rel_path)
) STRICT";

const CREATE_CHUNKS: &str = "\
CREATE TABLE IF NOT EXISTS rag_chunks (
    worktree_root TEXT NOT NULL,
    rel_path TEXT NOT NULL,
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    start_line INTEGER NOT NULL,
    text TEXT NOT NULL,
    embedding BLOB NOT NULL
) STRICT";

const CREATE_CHUNKS_INDEX: &str =
    "CREATE INDEX IF NOT EXISTS rag_chunks_by_file ON rag_chunks (worktree_root, rel_path)";

/// One chunk returned by a similarity search, with its cosine score.
#[derive(Debug, Clone)]
pub struct ScoredChunk {
    pub rel_path: String,
    pub start_line: usize,
    pub text: String,
    pub score: f32,
}

pub struct LocalStore {
    db_path: PathBuf,
}

impl LocalStore {
    /// Open (creating the directory if needed) the shared index database under
    /// `paths::embeddings_dir()`.
    pub fn new() -> Result<Self> {
        let directory = paths::embeddings_dir();
        std::fs::create_dir_all(directory)?;
        Ok(Self::at(directory.join("paddleboard_rag.db")))
    }

    fn at(db_path: PathBuf) -> Self {
        Self { db_path }
    }

    fn open(&self) -> Result<Connection> {
        let connection = Connection::open_file(&self.db_path.to_string_lossy());
        connection.exec(CREATE_FILES)?()?;
        connection.exec(CREATE_CHUNKS)?()?;
        connection.exec(CREATE_CHUNKS_INDEX)?()?;
        Ok(connection)
    }

    /// Map of `rel_path -> content_hash` for every indexed file under `root`.
    pub fn file_hashes(&self, root: &str) -> Result<HashMap<String, String>> {
        let connection = self.open()?;
        let rows = connection.select_bound::<String, (String, String)>(
            "SELECT rel_path, content_hash FROM rag_files WHERE worktree_root = ?",
        )?(root.to_string())?;
        Ok(rows.into_iter().collect())
    }

    /// Replace all stored chunks for one file (delete + reinsert) in a single
    /// transaction. `chunks` may be empty to record a file that produced no
    /// chunks without leaving stale rows.
    pub fn replace_file(
        &self,
        root: &str,
        rel_path: &str,
        content_hash: &str,
        chunks: &[(Chunk, Vec<f32>)],
        indexed_at: i64,
    ) -> Result<()> {
        let connection = self.open()?;
        connection.exec("BEGIN")?()?;
        let result = self.write_file(&connection, root, rel_path, content_hash, chunks, indexed_at);
        match result {
            Ok(()) => {
                connection.exec("COMMIT")?()?;
                Ok(())
            }
            Err(error) => {
                if let Err(rollback_error) = connection.exec("ROLLBACK").and_then(|mut run| run()) {
                    log::error!("paddleboard_rag: failed to roll back transaction: {rollback_error}");
                }
                Err(error)
            }
        }
    }

    fn write_file(
        &self,
        connection: &Connection,
        root: &str,
        rel_path: &str,
        content_hash: &str,
        chunks: &[(Chunk, Vec<f32>)],
        indexed_at: i64,
    ) -> Result<()> {
        connection.exec_bound::<(String, String)>(
            "DELETE FROM rag_chunks WHERE worktree_root = ? AND rel_path = ?",
        )?((root.to_string(), rel_path.to_string()))?;
        connection.exec_bound::<(String, String, String, i64)>(
            "INSERT OR REPLACE INTO rag_files (worktree_root, rel_path, content_hash, indexed_at) \
             VALUES (?, ?, ?, ?)",
        )?((root.to_string(), rel_path.to_string(), content_hash.to_string(), indexed_at))?;

        let mut insert = connection.exec_bound::<(String, String, i64, i64, i64, String, Vec<u8>)>(
            "INSERT INTO rag_chunks \
             (worktree_root, rel_path, start_byte, end_byte, start_line, text, embedding) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )?;
        for (chunk, embedding) in chunks {
            insert((
                root.to_string(),
                rel_path.to_string(),
                chunk.start_byte as i64,
                chunk.end_byte as i64,
                chunk.start_line as i64,
                chunk.text.clone(),
                encode_embedding(embedding),
            ))?;
        }
        Ok(())
    }

    /// Delete every file (and its chunks) under `root` whose `rel_path` is not
    /// in `present`.
    pub fn prune_missing(&self, root: &str, present: &HashSet<String>) -> Result<()> {
        let connection = self.open()?;
        let stored = connection.select_bound::<String, String>(
            "SELECT rel_path FROM rag_files WHERE worktree_root = ?",
        )?(root.to_string())?;
        for rel_path in stored {
            if !present.contains(&rel_path) {
                connection.exec_bound::<(String, String)>(
                    "DELETE FROM rag_chunks WHERE worktree_root = ? AND rel_path = ?",
                )?((root.to_string(), rel_path.clone()))?;
                connection.exec_bound::<(String, String)>(
                    "DELETE FROM rag_files WHERE worktree_root = ? AND rel_path = ?",
                )?((root.to_string(), rel_path.clone()))?;
            }
        }
        Ok(())
    }

    /// Brute-force cosine search over every chunk under `root`. `query` and the
    /// stored embeddings are expected to be L2-normalized, so the dot product is
    /// the cosine similarity.
    pub fn search(&self, root: &str, query: &[f32], limit: usize) -> Result<Vec<ScoredChunk>> {
        let connection = self.open()?;
        let rows = connection.select_bound::<String, (String, i64, String, Vec<u8>)>(
            "SELECT rel_path, start_line, text, embedding FROM rag_chunks WHERE worktree_root = ?",
        )?(root.to_string())?;

        let mut scored: Vec<ScoredChunk> = rows
            .into_iter()
            .filter_map(|(rel_path, start_line, text, blob)| {
                let embedding = decode_embedding(&blob);
                if embedding.len() != query.len() {
                    return None;
                }
                Some(ScoredChunk {
                    rel_path,
                    start_line: start_line.max(0) as usize,
                    text,
                    score: dot(query, &embedding),
                })
            })
            .collect();
        scored.sort_by(|a, b| b.score.total_cmp(&a.score));
        scored.truncate(limit);
        Ok(scored)
    }
}

fn encode_embedding(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|value| value.to_le_bytes()).collect()
}

fn decode_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(start_line: usize, text: &str) -> Chunk {
        Chunk {
            start_byte: 0,
            end_byte: text.len(),
            start_line,
            text: text.to_string(),
        }
    }

    #[test]
    fn embedding_round_trips_through_blob() {
        let original = vec![1.0f32, -0.5, 0.25, 0.0];
        assert_eq!(decode_embedding(&encode_embedding(&original)), original);
    }

    #[test]
    fn replace_search_and_prune() {
        let directory = tempfile::tempdir().unwrap();
        let store = LocalStore::at(directory.path().join("index.db"));
        let root = "/tmp/project";

        // Two files, each one chunk. Embeddings chosen so a query near [1,0]
        // ranks the first file's chunk higher.
        store
            .replace_file(root, "a.rs", "hash-a", &[(chunk(1, "alpha"), vec![1.0, 0.0])], 1)
            .unwrap();
        store
            .replace_file(root, "b.rs", "hash-b", &[(chunk(3, "beta"), vec![0.0, 1.0])], 1)
            .unwrap();

        assert_eq!(store.file_hashes(root).unwrap().len(), 2);

        let hits = store.search(root, &[1.0, 0.0], 10).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].rel_path, "a.rs");
        assert!(hits[0].score > hits[1].score);

        // Re-indexing a file replaces its chunks rather than duplicating them.
        store
            .replace_file(root, "a.rs", "hash-a2", &[(chunk(1, "alpha2"), vec![1.0, 0.0])], 2)
            .unwrap();
        let hits = store.search(root, &[1.0, 0.0], 10).unwrap();
        assert_eq!(hits.len(), 2);

        // Pruning drops files no longer present.
        let mut present = HashSet::default();
        present.insert("a.rs".to_string());
        store.prune_missing(root, &present).unwrap();
        assert_eq!(store.file_hashes(root).unwrap().len(), 1);
        let hits = store.search(root, &[1.0, 0.0], 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].rel_path, "a.rs");
    }

    #[test]
    fn dimension_mismatch_is_skipped() {
        let directory = tempfile::tempdir().unwrap();
        let store = LocalStore::at(directory.path().join("index.db"));
        let root = "/tmp/project";
        store
            .replace_file(root, "a.rs", "h", &[(chunk(1, "x"), vec![1.0, 0.0, 0.0])], 1)
            .unwrap();
        // Query has a different dimension than the stored vector.
        assert!(store.search(root, &[1.0, 0.0], 10).unwrap().is_empty());
    }
}
