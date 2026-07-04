use crate::{
    CloudRequestTimeoutError, CurrentEditPrediction, DebugEvent, EditPredictionFinishedDebugEvent,
    EditPredictionId, EditPredictionInputs, EditPredictionModelInput,
    EditPredictionStartedDebugEvent, EditPredictionStore, PromptHistoryBoundary,
    ZedUpdateRequiredError, buffer_path_with_id_fallback,
    cursor_excerpt::{self, compute_cursor_excerpt, compute_syntax_ranges},
    data_collection::CapturedPredictionContext,
    prediction::EditPredictionResult,
    udiff::prediction_edits_for_single_file_diff,
};
use anyhow::{Context as _, Result};
use cloud_llm_client::{AcceptEditPredictionBody, predict_edits_v3::RawCompletionRequest};
use std::env;
use edit_prediction_types::PredictedCursorPosition;
use gpui::{App, AppContext as _, Entity, Task, TaskExt, WeakEntity, prelude::*};
use language::{
    Buffer, BufferSnapshot, DiagnosticSeverity, EditPredictionPromptFormat, OffsetRangeExt as _,
    ToOffset as _, ZetaVersion, language_settings::all_language_settings, text_diff,
};
use project::Project;
use release_channel::AppVersion;
use text::{Anchor, Bias, Point};
use ui::SharedString;
use workspace::notifications::simple_message_notification::MessageNotification;
use workspace::notifications::{NotificationId, show_app_notification};
use workspace::workspace_error::{ErrorAction, ErrorSeverity, WorkspaceError};

use std::{ops::Range, path::Path, sync::Arc};
use zeta_prompt::{
    FilePosition, ParsedOutput, Zeta2PromptInput, Zeta3PromptInput, ZetaFormat,
    excerpt_ranges_for_format, format_zeta_prompt, get_prefill, parse_zeta2_model_output,
    stop_tokens_for_format,
    zeta1::{self, EDITABLE_REGION_END_MARKER},
};

use crate::open_ai_compatible::{
    load_open_ai_compatible_api_key_if_needed, send_custom_server_request,
};

// PaddleBoard: Zeta (Zed's hosted edit-prediction LLM) is disabled.
// Calling it without Zed Cloud credentials spams the log with
// `no credentials provided` from cloud_api_client; we have no way
// to provide those credentials anyway since PaddleBoard isn't a
// hosted service. The function below is rewritten to immediately
// return `Ok(None)` so the caller treats it as "nothing to predict"
// and either falls back to other providers (Copilot, Codestral,
// Ollama, Mercury, FIM-via-OpenAI-compatible) or shows no inline
// suggestion. Function signature, name, and call sites match
// upstream so merges stay mechanical.
#[allow(unused_variables)]
pub(crate) fn request_prediction_with_zeta(
    store: &mut EditPredictionStore,
    input: EditPredictionModelInput,
    context_task: Option<Task<Result<CapturedPredictionContext>>>,
    prompt_history_boundary: Option<PromptHistoryBoundary>,
    repo_url: Option<String>,
    cx: &mut Context<EditPredictionStore>,
) -> Task<Result<Option<EditPredictionResult>>> {
    cx.background_spawn(async move { anyhow::Ok(None) })
}

// PaddleBoard: the original Zeta request implementation was removed.
// The active utility functions below are still used by Mercury, FIM,
// and other providers.

fn handle_api_response<T>(
    this: &WeakEntity<EditPredictionStore>,
    response: Result<(T, Option<client::EditPredictionUsage>)>,
    cx: &mut gpui::AsyncApp,
) -> Result<T> {
    match response {
        Ok((data, usage)) => {
            if let Some(usage) = usage {
                this.update(cx, |this, cx| {
                    this.user_store.update(cx, |user_store, cx| {
                        user_store.update_edit_prediction_usage(usage, cx);
                    });
                })
                .ok();
            }
            Ok(data)
        }
        Err(err) => {
            if err.is::<CloudRequestTimeoutError>() {
                this.update(cx, |this, cx| this.back_off_requests_after_timeout(cx))
                    .ok();
            }

            if err.is::<ZedUpdateRequiredError>() {
                cx.update(|cx| {
                    this.update(cx, |this, _cx| {
                        this.update_required = true;
                    })
                    .ok();

                    let message: SharedString = err.to_string().into();

                    struct UpdateRequiredError {
                        message: SharedString,
                    }
                    impl WorkspaceError for UpdateRequiredError {
                        fn primary_message(&self) -> SharedString {
                            self.message.clone()
                        }
                        fn severity(&self) -> ErrorSeverity {
                            ErrorSeverity::Critical
                        }
                        fn primary_action(&self) -> ErrorAction {
                            ErrorAction::link("Update Zed", "https://zed.dev/releases")
                        }
                    }

                    show_app_notification(
                        NotificationId::unique::<ZedUpdateRequiredError>(),
                        cx,
                        move |cx| {
                            cx.new({
                                let message = message.clone();
                                move |cx| {
                                    let error = UpdateRequiredError { message };
                                    MessageNotification::from_workspace_error(error, cx)
                                }
                            })
                        },
                    );
                });
            }
            Err(err)
        }
    }
}

const ACTIVE_BUFFER_DIAGNOSTIC_ADDITIONAL_CONTEXT_TOKEN_COUNT: usize = 100;
const MAX_ACTIVE_BUFFER_DIAGNOSTICS_TO_COLLECT: usize = 20;
pub(crate) const MAX_ACTIVE_BUFFER_DIAGNOSTIC_MESSAGE_TOKENS_TO_COLLECT: usize = 512;
pub(crate) const MAX_ACTIVE_BUFFER_DIAGNOSTIC_SNIPPET_TOKENS_TO_COLLECT: usize = 512;

pub(crate) fn active_buffer_diagnostics(
    snapshot: &language::BufferSnapshot,
    diagnostic_search_range: Range<Point>,
    cursor_row: u32,
    additional_context_token_count: usize,
) -> Vec<zeta_prompt::ActiveBufferDiagnostic> {
    let mut diagnostics = snapshot
        .diagnostics_in_range::<Point, Point>(diagnostic_search_range, false)
        .collect::<Vec<_>>();
    diagnostics.sort_by_key(|entry| {
        cursor_row.abs_diff(entry.range.start.row) + cursor_row.abs_diff(entry.range.end.row)
    });

    diagnostics
        .into_iter()
        .map(|entry| {
            let diagnostic_point_range = entry.range.clone();
            let snippet_point_range = cursor_excerpt::expand_context_syntactically_then_linewise(
                snapshot,
                diagnostic_point_range.clone(),
                additional_context_token_count,
            );

            let severity = match entry.diagnostic.severity {
                DiagnosticSeverity::ERROR => Some(1),
                DiagnosticSeverity::WARNING => Some(2),
                DiagnosticSeverity::INFORMATION => Some(3),
                DiagnosticSeverity::HINT => Some(4),
                _ => None,
            };
            (
                severity,
                zeta_prompt::clamp_text_to_token_count(
                    &entry.diagnostic.message,
                    MAX_ACTIVE_BUFFER_DIAGNOSTIC_MESSAGE_TOKENS_TO_COLLECT,
                )
                .to_string(),
                diagnostic_point_range,
                snippet_point_range,
            )
        })
        .take(MAX_ACTIVE_BUFFER_DIAGNOSTICS_TO_COLLECT)
        .map(
            |(severity, message, diagnostic_point_range, snippet_point_range)| {
                let (snippet, diagnostic_range_in_snippet) = if snippet_point_range.start
                    == Point::new(0, 0)
                    && snippet_point_range.end == snapshot.max_point()
                {
                    (String::new(), 0..0)
                } else {
                    let snippet = snapshot
                        .text_for_range(snippet_point_range.clone())
                        .collect::<String>();
                    let snippet = zeta_prompt::clamp_text_to_token_count(
                        &snippet,
                        MAX_ACTIVE_BUFFER_DIAGNOSTIC_SNIPPET_TOKENS_TO_COLLECT,
                    )
                    .to_string();
                    let snippet_start_offset = snippet_point_range.start.to_offset(snapshot);
                    let diagnostic_offset_range = diagnostic_point_range.to_offset(snapshot);
                    let diagnostic_range_start = diagnostic_offset_range
                        .start
                        .saturating_sub(snippet_start_offset)
                        .min(snippet.len());
                    let diagnostic_range_end = diagnostic_offset_range
                        .end
                        .saturating_sub(snippet_start_offset)
                        .min(snippet.len());
                    (snippet, diagnostic_range_start..diagnostic_range_end)
                };
                zeta_prompt::ActiveBufferDiagnostic {
                    severity,
                    message,
                    snippet,
                    snippet_buffer_row_range: diagnostic_point_range.start.row
                        ..diagnostic_point_range.end.row,
                    diagnostic_range_in_snippet,
                }
            },
        )
        .collect()
}

pub fn zeta2_prompt_input(
    snapshot: &language::BufferSnapshot,
    mut related_files: Vec<zeta_prompt::RelatedFile>,
    events: Vec<Arc<zeta_prompt::Event>>,
    diagnostic_search_range: Range<Point>,
    excerpt_path: Arc<Path>,
    cursor_offset: usize,
    is_open_source: bool,
    can_collect_data: bool,
    repo_url: Option<String>,
) -> (Range<usize>, zeta_prompt::Zeta2PromptInput) {
    let (excerpt_point_range, excerpt_offset_range, cursor_offset_in_excerpt) =
        compute_cursor_excerpt(snapshot, cursor_offset);

    let cursor_excerpt: Arc<str> = snapshot
        .text_for_range(excerpt_point_range.clone())
        .collect::<String>()
        .into();
    let syntax_ranges = compute_syntax_ranges(snapshot, cursor_offset, &excerpt_offset_range);
    let excerpt_ranges = zeta_prompt::compute_legacy_excerpt_ranges(
        &cursor_excerpt,
        cursor_offset_in_excerpt,
        &syntax_ranges,
    );

    let active_buffer_diagnostics = active_buffer_diagnostics(
        snapshot,
        diagnostic_search_range,
        snapshot.offset_to_point(cursor_offset).row,
        ACTIVE_BUFFER_DIAGNOSTIC_ADDITIONAL_CONTEXT_TOKEN_COUNT,
    );
    for file in &mut related_files {
        for excerpt in &mut file.excerpts {
            excerpt.context_source = zeta_prompt::ContextSource::Lsp;
        }
    }

    let prompt_input = zeta_prompt::Zeta2PromptInput {
        cursor_path: excerpt_path,
        cursor_excerpt,
        cursor_offset_in_excerpt,
        excerpt_start_row: Some(excerpt_point_range.start.row),
        events,
        related_files: Some(related_files),
        active_buffer_diagnostics,
        excerpt_ranges,
        syntax_ranges: Some(syntax_ranges),
        in_open_source_repo: is_open_source,
        can_collect_data,
        repo_url,
    };
    (excerpt_offset_range, prompt_input)
}

pub(crate) fn edit_prediction_accepted(
    store: &EditPredictionStore,
    current_prediction: CurrentEditPrediction,
    cx: &App,
) {
    let custom_accept_url = env::var("PADDLEBOARD_ACCEPT_PREDICTION_URL").ok();
    if store.zeta2_raw_config().is_some() && custom_accept_url.is_none() {
        return;
    }

    let request_id = current_prediction.prediction.id.to_string();
    let model_version = current_prediction.prediction.model_version;
    let e2e_latency = current_prediction.e2e_latency;
    let client = store.client.clone();
    let llm_token = store.llm_token.clone();
    let organization_id = store
        .user_store
        .read(cx)
        .current_organization()
        .map(|organization| organization.id.clone());
    let app_version = AppVersion::global(cx);

    cx.background_spawn(async move {
        let body = serde_json::to_string(&AcceptEditPredictionBody {
            request_id,
            model_version,
            e2e_latency_ms: Some(e2e_latency.as_millis()),
        })?;

        let url = client
            .http_client()
            .build_zed_llm_url("/predict_edits/accept", &[])?;
        EditPredictionStore::send_api_request::<()>(
            move |builder| Ok(builder.uri(url.as_ref()).body(body.clone().into())?),
            client,
            llm_token,
            organization_id,
            app_version,
        )
        .await?;
        anyhow::Ok(())
    })
    .detach_and_log_err(cx);
}

pub fn compute_edits(
    old_text: String,
    new_text: &str,
    offset: usize,
    snapshot: &BufferSnapshot,
) -> Vec<(Range<Anchor>, Arc<str>)> {
    compute_edits_and_cursor_position(old_text, new_text, offset, None, snapshot).0
}

pub fn compute_edits_and_cursor_position(
    old_text: String,
    new_text: &str,
    offset: usize,
    cursor_offset_in_new_text: Option<usize>,
    snapshot: &BufferSnapshot,
) -> (
    Vec<(Range<Anchor>, Arc<str>)>,
    Option<PredictedCursorPosition>,
) {
    let diffs = text_diff(&old_text, new_text);

    // Delta represents the cumulative change in byte count from all preceding edits.
    // new_offset = old_offset + delta, so old_offset = new_offset - delta
    let mut delta: isize = 0;
    let mut cursor_position: Option<PredictedCursorPosition> = None;
    let buffer_len = snapshot.len();

    let edits = diffs
        .iter()
        .map(|(raw_old_range, new_text)| {
            // Compute cursor position if it falls within or before this edit.
            if let (Some(cursor_offset), None) = (cursor_offset_in_new_text, cursor_position) {
                let edit_start_in_new = (raw_old_range.start as isize + delta) as usize;
                let edit_end_in_new = edit_start_in_new + new_text.len();

                if cursor_offset < edit_start_in_new {
                    let cursor_in_old = (cursor_offset as isize - delta) as usize;
                    let buffer_offset = (offset + cursor_in_old).min(buffer_len);
                    cursor_position = Some(PredictedCursorPosition::at_anchor(
                        snapshot.anchor_after(buffer_offset),
                    ));
                } else if cursor_offset < edit_end_in_new {
                    let buffer_offset = (offset + raw_old_range.start).min(buffer_len);
                    let offset_within_insertion = cursor_offset - edit_start_in_new;
                    cursor_position = Some(PredictedCursorPosition::new(
                        snapshot.anchor_before(buffer_offset),
                        offset_within_insertion,
                    ));
                }

                delta += new_text.len() as isize - raw_old_range.len() as isize;
            }

            // Compute the edit with prefix/suffix trimming.
            let mut old_range = raw_old_range.clone();
            let old_slice = &old_text[old_range.clone()];

            let prefix_len = common_prefix(old_slice.chars(), new_text.chars());
            let suffix_len = common_prefix(
                old_slice[prefix_len..].chars().rev(),
                new_text[prefix_len..].chars().rev(),
            );

            old_range.start += offset;
            old_range.end += offset;
            old_range.start += prefix_len;
            old_range.end -= suffix_len;

            old_range.start = old_range.start.min(buffer_len);
            old_range.end = old_range.end.min(buffer_len);

            let new_text = new_text[prefix_len..new_text.len() - suffix_len].into();
            let range = if old_range.is_empty() {
                let anchor = snapshot.anchor_after(old_range.start);
                anchor..anchor
            } else {
                snapshot.anchor_after(old_range.start)..snapshot.anchor_before(old_range.end)
            };
            (range, new_text)
        })
        .collect();

    if let (Some(cursor_offset), None) = (cursor_offset_in_new_text, cursor_position) {
        let cursor_in_old = (cursor_offset as isize - delta) as usize;
        let buffer_offset = snapshot.clip_offset(offset + cursor_in_old, Bias::Right);
        cursor_position = Some(PredictedCursorPosition::at_anchor(
            snapshot.anchor_after(buffer_offset),
        ));
    }

    (edits, cursor_position)
}

fn common_prefix<T1: Iterator<Item = char>, T2: Iterator<Item = char>>(a: T1, b: T2) -> usize {
    a.zip(b)
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum()
}
