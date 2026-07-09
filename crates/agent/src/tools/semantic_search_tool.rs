use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use anyhow::Result;
use gpui::{App, Entity, SharedString, Task};
use language_model::LanguageModelToolResultContent;
use project::Project;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings::Settings as _;

use crate::{AgentTool, ToolCallEventStream, ToolInput};

// PaddleBoard: privacy-first local semantic search. Indexes the current project
// with the built-in EmbeddingGemma server and ranks chunks by cosine
// similarity — nothing leaves the machine. Registered only while
// `paddleboard_rag.enabled` is set.

/// Search the current project semantically for code and text relevant to a
/// natural-language query, using PaddleBoard's built-in local embedding model.
/// Nothing leaves the machine.
///
/// Prefer this over guessing file locations when you need to find where a
/// concept, behavior, or feature lives and exact search terms are unknown
/// (complements the literal `grep` tool). The first call for a project indexes
/// it — and downloads the embedding model once — which can take a moment; later
/// calls are fast and incremental. Results are chunks with a file path, a
/// starting line, and an excerpt.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SemanticSearchToolInput {
    /// Natural-language description of what you're looking for.
    pub query: String,
    /// Maximum number of results to return. Defaults to 8.
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticSearchResult {
    pub path: String,
    pub start_line: usize,
    pub score: f32,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticSearchToolOutput {
    pub results: Vec<SemanticSearchResult>,
    pub summary: String,
}

impl From<SemanticSearchToolOutput> for LanguageModelToolResultContent {
    fn from(output: SemanticSearchToolOutput) -> Self {
        serde_json::to_string(&output)
            .unwrap_or_else(|error| format!("Failed to serialize semantic_search output: {error}"))
            .into()
    }
}

pub struct SemanticSearchTool {
    project: Entity<Project>,
}

impl SemanticSearchTool {
    pub fn new(project: Entity<Project>) -> Self {
        Self { project }
    }
}

fn error_output(message: impl Into<String>) -> SemanticSearchToolOutput {
    SemanticSearchToolOutput {
        results: Vec::new(),
        summary: message.into(),
    }
}

const DEFAULT_LIMIT: usize = 8;

impl AgentTool for SemanticSearchTool {
    type Input = SemanticSearchToolInput;
    type Output = SemanticSearchToolOutput;

    const NAME: &'static str = "semantic_search";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Search
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        match input {
            Ok(SemanticSearchToolInput { query, .. }) => {
                format!("Semantic search: {query}").into()
            }
            _ => "Semantic search".into(),
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        _event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        let Some(manager) = paddleboard_llama_manager::manager(cx) else {
            return Task::ready(Err(error_output(
                "The local model manager is unavailable, so semantic search can't run.",
            )));
        };
        let store = match &paddleboard_rag_settings::RagSettings::get_global(cx).store {
            paddleboard_rag_settings::RagStoreSetting::Local => paddleboard_rag::StoreConfig::Local,
            paddleboard_rag_settings::RagStoreSetting::Pgvector {
                url_env,
                table_prefix,
                ssl,
            } => match std::env::var(url_env) {
                Ok(url) => paddleboard_rag::StoreConfig::Pgvector {
                    url,
                    table_prefix: table_prefix.clone(),
                    ssl: *ssl,
                },
                Err(_) => {
                    return Task::ready(Err(error_output(format!(
                        "Semantic search is set to use a pgvector store, but the connection-string \
                         environment variable {url_env} is not set."
                    ))));
                }
            },
        };
        let http_client = cx.http_client();
        let project = self.project.downgrade();

        cx.spawn(async move |cx| {
            let input = input.recv().await.map_err(|error| error_output(error.to_string()))?;
            let query = input.query.trim().to_string();
            if query.is_empty() {
                return Err(error_output("Provide a non-empty query."));
            }
            let limit = input.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, 50);

            let outcome =
                paddleboard_rag::search(project, manager, http_client, store, query, limit, cx)
                    .await
                    .map_err(|error| error_output(error.to_string()))?;

            let results = outcome
                .hits
                .into_iter()
                .map(|hit| SemanticSearchResult {
                    path: hit.rel_path,
                    start_line: hit.start_line,
                    score: hit.score,
                    excerpt: hit.excerpt,
                })
                .collect::<Vec<_>>();

            let mut summary = format!("{} result(s).", results.len());
            if outcome.files_indexed > 0 {
                summary.push_str(&format!(" Indexed {} changed file(s).", outcome.files_indexed));
            }
            if outcome.files_skipped_large > 0 || outcome.files_skipped_binary > 0 {
                summary.push_str(&format!(
                    " Skipped {} large and {} binary file(s).",
                    outcome.files_skipped_large, outcome.files_skipped_binary
                ));
            }
            if outcome.files_capped > 0 {
                summary.push_str(&format!(
                    " Reached the {}-file index cap ({} file(s) not indexed).",
                    paddleboard_rag::MAX_FILES,
                    outcome.files_capped
                ));
            }

            Ok(SemanticSearchToolOutput { results, summary })
        })
    }
}
