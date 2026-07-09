//! Privacy-first local RAG for PaddleBoard: index the current project's files
//! with the built-in EmbeddingGemma server and answer natural-language queries
//! by brute-force cosine similarity. Nothing leaves the machine.
//!
//! The public entry point is [`search`], which the agent's `semantic_search`
//! tool calls. Indexing is incremental (keyed on a per-file content hash) and
//! happens lazily on demand, so the first query for a project pays the indexing
//! cost and later queries are fast.

mod backend;
mod chunk;
mod embedding;
#[cfg(feature = "pgvector")]
mod pgvector_store;
mod store;

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, anyhow, bail};
use collections::{HashMap, HashSet};
use gpui::{AppContext as _, AsyncApp, Entity, WeakEntity};
use http_client::HttpClient;
use project::Project;

use paddleboard_llama_manager::{DEFAULT_EMBEDDING_MODEL_ID, LlamaManager, ManagerStatus};

use crate::backend::Backend;
pub use crate::backend::StoreConfig;
use crate::embedding::{EmbedKind, EmbeddingClient};

/// Files larger than this are skipped (generated bundles, vendored blobs).
const MAX_FILE_BYTES: u64 = 512 * 1024;
/// Ceiling on files indexed per query, so a huge monorepo can't wedge a single
/// tool call. Overflow is reported in the tool result, never silently dropped.
pub const MAX_FILES: usize = 5000;
/// Length of the excerpt returned per hit.
const EXCERPT_BYTES: usize = 400;

/// How long to wait for the embedding model to become ready before returning a
/// "still preparing" message. The first run downloads ~330 MB, which can exceed
/// this — the tool tells the user to retry while the download continues in the
/// background.
const READY_TIMEOUT: Duration = Duration::from_secs(180);
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// One search result: a chunk of a file with its similarity score.
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub rel_path: String,
    pub start_line: usize,
    pub excerpt: String,
    pub score: f32,
}

/// Results plus a report of what indexing did, so the caller can surface skips
/// and caps rather than pretending everything was covered.
#[derive(Debug, Default, Clone)]
pub struct SearchOutcome {
    pub hits: Vec<SearchHit>,
    pub files_indexed: usize,
    pub files_skipped_large: usize,
    pub files_skipped_binary: usize,
    pub files_capped: usize,
}

/// Index the project (incrementally) and return the top `limit` chunks most
/// similar to `query`.
pub async fn search(
    project: WeakEntity<Project>,
    manager: Entity<LlamaManager>,
    http_client: Arc<dyn HttpClient>,
    store: StoreConfig,
    query: String,
    limit: usize,
    cx: &mut AsyncApp,
) -> Result<SearchOutcome> {
    let port = ensure_embedding_ready(&manager, cx).await?;
    let (files, files_capped) = collect_files(&project, cx)?;
    let roots = distinct_roots(&files);

    let mut outcome = cx
        .background_spawn(async move {
            index_and_search(http_client, port, store, files, roots, query, limit).await
        })
        .await?;
    outcome.files_capped = files_capped;
    Ok(outcome)
}

async fn ensure_embedding_ready(manager: &Entity<LlamaManager>, cx: &mut AsyncApp) -> Result<u16> {
    // `manager` is a strong `Entity`, so `update`/`read_with` return the value
    // directly (only the `WeakEntity` variants are fallible).
    manager.update(cx, |manager, cx| {
        manager.ensure_embedding_running(DEFAULT_EMBEDDING_MODEL_ID, cx)
    });

    let started = Instant::now();
    loop {
        let status = manager.read_with(cx, |manager, _| manager.embedding_status().clone());
        match status {
            ManagerStatus::Ready { port, .. } => return Ok(port),
            ManagerStatus::Error { message } => {
                bail!("local embedding model failed to start: {message}")
            }
            ManagerStatus::Unsupported => {
                bail!("local embedding models are not supported on this platform")
            }
            ManagerStatus::Downloading { received, total, .. } => {
                if started.elapsed() > READY_TIMEOUT {
                    let progress = match total {
                        Some(total) if total > 0 => {
                            format!(" ({}%)", received.saturating_mul(100) / total)
                        }
                        _ => String::new(),
                    };
                    bail!(
                        "the local embedding model is still downloading{progress}; \
                         re-run semantic_search in a moment"
                    );
                }
            }
            ManagerStatus::Idle | ManagerStatus::Preparing | ManagerStatus::Starting { .. } => {
                if started.elapsed() > READY_TIMEOUT {
                    bail!("the local embedding model is still preparing; re-run semantic_search in a moment");
                }
            }
        }
        cx.background_executor().timer(POLL_INTERVAL).await;
    }
}

struct FileRef {
    worktree_root: String,
    abs_path: PathBuf,
    rel_path: String,
}

/// Enumerate non-ignored files across the project's visible worktrees. Returns
/// the files (capped at [`MAX_FILES`]) and the count that exceeded the cap.
fn collect_files(project: &WeakEntity<Project>, cx: &mut AsyncApp) -> Result<(Vec<FileRef>, usize)> {
    project
        .read_with(cx, |project, cx| {
            let mut files = Vec::new();
            let mut capped = 0;
            for worktree in project.visible_worktrees(cx) {
                let worktree = worktree.read(cx);
                let root_path = worktree.abs_path();
                let worktree_root = root_path.to_string_lossy().to_string();
                let snapshot = worktree.snapshot();
                for entry in snapshot.files(false, 0) {
                    if files.len() >= MAX_FILES {
                        capped += 1;
                        continue;
                    }
                    files.push(FileRef {
                        worktree_root: worktree_root.clone(),
                        abs_path: root_path.join(entry.path.as_std_path()),
                        rel_path: entry.path.as_unix_str().to_string(),
                    });
                }
            }
            (files, capped)
        })
        .map_err(|error| anyhow!("failed to read project worktrees: {error}"))
}

fn distinct_roots(files: &[FileRef]) -> Vec<String> {
    let mut roots: Vec<String> = files.iter().map(|file| file.worktree_root.clone()).collect();
    roots.sort();
    roots.dedup();
    roots
}

async fn index_and_search(
    http_client: Arc<dyn HttpClient>,
    port: u16,
    store: StoreConfig,
    files: Vec<FileRef>,
    roots: Vec<String>,
    query: String,
    limit: usize,
) -> Result<SearchOutcome> {
    let store = Backend::open(store)?;
    let client = EmbeddingClient::new(http_client, port);

    let mut outcome = SearchOutcome::default();
    let mut present: HashMap<String, HashSet<String>> = HashMap::default();
    let mut existing: HashMap<String, HashMap<String, String>> = HashMap::default();
    for root in &roots {
        existing.insert(root.clone(), store.file_hashes(root)?);
    }

    for file in &files {
        present
            .entry(file.worktree_root.clone())
            .or_default()
            .insert(file.rel_path.clone());

        let Ok(metadata) = std::fs::metadata(&file.abs_path) else {
            continue;
        };
        if metadata.len() > MAX_FILE_BYTES {
            outcome.files_skipped_large += 1;
            continue;
        }
        let Ok(bytes) = std::fs::read(&file.abs_path) else {
            continue;
        };
        let Ok(text) = String::from_utf8(bytes) else {
            outcome.files_skipped_binary += 1;
            continue;
        };

        let content_hash = content_hash(&text);
        let unchanged = existing
            .get(&file.worktree_root)
            .and_then(|hashes| hashes.get(&file.rel_path))
            .map(|stored| stored == &content_hash)
            .unwrap_or(false);
        if unchanged {
            continue;
        }

        let chunks = chunk::chunk_text(&text);
        if chunks.is_empty() {
            store.replace_file(&file.worktree_root, &file.rel_path, &content_hash, &[], now_secs())?;
            continue;
        }
        let texts: Vec<String> = chunks.iter().map(|chunk| chunk.text.clone()).collect();
        let embeddings = client.embed(EmbedKind::Document, &texts).await?;
        let rows: Vec<(chunk::Chunk, Vec<f32>)> = chunks
            .into_iter()
            .zip(embeddings.into_iter().map(normalize))
            .collect();
        store.replace_file(&file.worktree_root, &file.rel_path, &content_hash, &rows, now_secs())?;
        outcome.files_indexed += 1;
    }

    for root in &roots {
        let present_in_root = present.get(root).cloned().unwrap_or_default();
        store.prune_missing(root, &present_in_root)?;
    }

    let query_embedding = client
        .embed(EmbedKind::Query, std::slice::from_ref(&query))
        .await?
        .into_iter()
        .next()
        .context("embedding server returned no vector for the query")?;
    let query_embedding = normalize(query_embedding);

    let mut hits = Vec::new();
    for root in &roots {
        for scored in store.search(root, &query_embedding, limit)? {
            hits.push(SearchHit {
                rel_path: scored.rel_path,
                start_line: scored.start_line,
                excerpt: excerpt(&scored.text),
                score: scored.score,
            });
        }
    }
    hits.sort_by(|a, b| b.score.total_cmp(&a.score));
    hits.truncate(limit);
    outcome.hits = hits;
    Ok(outcome)
}

fn content_hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn normalize(mut vector: Vec<f32>) -> Vec<f32> {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in &mut vector {
            *value /= norm;
        }
    }
    vector
}

fn excerpt(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.len() <= EXCERPT_BYTES {
        return trimmed.to_string();
    }
    let mut end = EXCERPT_BYTES;
    while end > 0 && !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &trimmed[..end])
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_produces_unit_vector() {
        let normalized = normalize(vec![3.0, 4.0]);
        let norm = normalized.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6);
    }

    #[test]
    fn normalize_leaves_zero_vector_alone() {
        assert_eq!(normalize(vec![0.0, 0.0]), vec![0.0, 0.0]);
    }

    #[test]
    fn excerpt_truncates_on_char_boundary() {
        let text = "é".repeat(500);
        let excerpt = excerpt(&text);
        assert!(excerpt.ends_with('…'));
        // Truncation must not panic and must stay within budget + ellipsis.
        assert!(excerpt.len() <= EXCERPT_BYTES + '…'.len_utf8());
    }

    #[test]
    fn content_hash_is_stable_and_sensitive() {
        assert_eq!(content_hash("abc"), content_hash("abc"));
        assert_ne!(content_hash("abc"), content_hash("abd"));
        assert_eq!(content_hash("abc").len(), 64);
    }
}
