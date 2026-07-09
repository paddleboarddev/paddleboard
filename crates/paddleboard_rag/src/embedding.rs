//! HTTP client for the managed llama.cpp embedding server's `/v1/embeddings`
//! endpoint. Mirrors the request/parse pattern in `llama_cpp` (no dedicated
//! HTTP crate — just `http_client`).

use std::sync::Arc;

use anyhow::{Context as _, Result};
use futures::AsyncReadExt as _;
use http_client::{AsyncBody, HttpClient, Method, http};
use serde::{Deserialize, Serialize};

use paddleboard_llama_manager::DEFAULT_EMBEDDING_MODEL_ID;

// EmbeddingGemma is trained with task-specific prompt prefixes; applying them
// for documents vs queries materially improves retrieval quality. These match
// the model card's "Retrieval" task and are centralized here so they're easy to
// tune if the pinned model changes.
const DOCUMENT_PREFIX: &str = "title: none | text: ";
const QUERY_PREFIX: &str = "task: search result | query: ";

/// Number of texts sent per request — bounds request body size while cutting
/// round-trips versus embedding one text at a time.
const BATCH_SIZE: usize = 16;

#[derive(Clone, Copy)]
pub enum EmbedKind {
    Document,
    Query,
}

impl EmbedKind {
    fn apply(self, text: &str) -> String {
        match self {
            EmbedKind::Document => format!("{DOCUMENT_PREFIX}{text}"),
            EmbedKind::Query => format!("{QUERY_PREFIX}{text}"),
        }
    }
}

pub struct EmbeddingClient {
    http_client: Arc<dyn HttpClient>,
    base_url: String,
}

#[derive(Serialize)]
struct EmbeddingRequest {
    model: &'static str,
    input: Vec<String>,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

impl EmbeddingClient {
    pub fn new(http_client: Arc<dyn HttpClient>, port: u16) -> Self {
        Self {
            http_client,
            base_url: format!("http://127.0.0.1:{port}"),
        }
    }

    /// Embed `texts`, preserving order. Batches internally.
    pub async fn embed(&self, kind: EmbedKind, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut embeddings = Vec::with_capacity(texts.len());
        for batch in texts.chunks(BATCH_SIZE) {
            embeddings.extend(self.embed_batch(kind, batch).await?);
        }
        Ok(embeddings)
    }

    async fn embed_batch(&self, kind: EmbedKind, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let request_body = serde_json::to_string(&EmbeddingRequest {
            model: DEFAULT_EMBEDDING_MODEL_ID,
            input: texts.iter().map(|text| kind.apply(text)).collect(),
        })?;
        let request = http::Request::builder()
            .method(Method::POST)
            .uri(format!("{}/v1/embeddings", self.base_url))
            .header("Content-Type", "application/json")
            .body(AsyncBody::from(request_body))?;

        let mut response = self.http_client.send(request).await?;
        let mut response_body = String::new();
        response.body_mut().read_to_string(&mut response_body).await?;
        anyhow::ensure!(
            response.status().is_success(),
            "embedding request failed: {} {}",
            response.status(),
            response_body
        );

        let parsed: EmbeddingResponse = serde_json::from_str(&response_body)
            .context("failed to parse /v1/embeddings response")?;
        anyhow::ensure!(
            parsed.data.len() == texts.len(),
            "embedding server returned {} vectors for {} inputs",
            parsed.data.len(),
            texts.len()
        );
        Ok(parsed.data.into_iter().map(|item| item.embedding).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_and_query_prefixes_differ() {
        assert!(EmbedKind::Document.apply("foo").starts_with(DOCUMENT_PREFIX));
        assert!(EmbedKind::Query.apply("foo").starts_with(QUERY_PREFIX));
        assert_ne!(EmbedKind::Document.apply("foo"), EmbedKind::Query.apply("foo"));
    }

    #[test]
    fn request_serializes_openai_shape() {
        let json = serde_json::to_string(&EmbeddingRequest {
            model: DEFAULT_EMBEDDING_MODEL_ID,
            input: vec!["a".into(), "b".into()],
        })
        .unwrap();
        assert!(json.contains(r#""model":"embeddinggemma-300m""#));
        assert!(json.contains(r#""input":["a","b"]"#));
    }

    #[test]
    fn response_parses_float_arrays() {
        let response: EmbeddingResponse =
            serde_json::from_str(r#"{"data":[{"embedding":[0.1,0.2]},{"embedding":[0.3,0.4]}]}"#)
                .unwrap();
        assert_eq!(response.data.len(), 2);
        assert_eq!(response.data[0].embedding, vec![0.1, 0.2]);
    }
}
