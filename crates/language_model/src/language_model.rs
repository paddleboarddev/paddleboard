mod api_key;
mod model;
mod registry;
mod request;

#[cfg(any(test, feature = "test-support"))]
pub mod fake_provider;

pub use language_model_core::*;

use anyhow::Result;
use futures::FutureExt;
use futures::{StreamExt, future::BoxFuture, stream::BoxStream};
use gpui::{AnyView, App, AsyncApp, Task, Window};
use icons::IconName;
use parking_lot::Mutex;
use std::sync::Arc;

pub use crate::api_key::{ApiKey, ApiKeyState};
pub use crate::model::*;
pub use crate::registry::*;
pub use crate::request::{LanguageModelImageExt, gpui_size_to_image_size, image_size_to_gpui};
pub use env_var::{EnvVar, env_var};

pub fn init(cx: &mut App) {
    registry::init(cx);
}

<<<<<<< HEAD
#[derive(Clone, Debug)]
pub struct LanguageModelCacheConfiguration {
    pub max_cache_anchors: usize,
    pub should_speculate: bool,
    pub min_total_token: u64,
}

/// A completion event from a language model.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum LanguageModelCompletionEvent {
    Queued {
        position: usize,
    },
    Started,
    Stop(StopReason),
    Text(String),
    Thinking {
        text: String,
        signature: Option<String>,
    },
    RedactedThinking {
        data: String,
    },
    ToolUse(LanguageModelToolUse),
    ToolUseJsonParseError {
        id: LanguageModelToolUseId,
        tool_name: Arc<str>,
        raw_input: Arc<str>,
        json_parse_error: String,
    },
    StartMessage {
        message_id: String,
    },
    ReasoningDetails(serde_json::Value),
    UsageUpdate(TokenUsage),
}

impl LanguageModelCompletionEvent {
    pub fn from_completion_request_status(
        status: CompletionRequestStatus,
        upstream_provider: LanguageModelProviderName,
    ) -> Result<Option<Self>, LanguageModelCompletionError> {
        match status {
            CompletionRequestStatus::Queued { position } => {
                Ok(Some(LanguageModelCompletionEvent::Queued { position }))
            }
            CompletionRequestStatus::Started => Ok(Some(LanguageModelCompletionEvent::Started)),
            CompletionRequestStatus::Unknown | CompletionRequestStatus::StreamEnded => Ok(None),
            CompletionRequestStatus::Failed {
                code,
                message,
                request_id: _,
                retry_after,
            } => Err(LanguageModelCompletionError::from_cloud_failure(
                upstream_provider,
                code,
                message,
                retry_after.map(Duration::from_secs_f64),
            )),
        }
    }
}

#[derive(Error, Debug)]
pub enum LanguageModelCompletionError {
    #[error("prompt too large for context window")]
    PromptTooLarge { tokens: Option<u64> },
    #[error("missing {provider} API key")]
    NoApiKey { provider: LanguageModelProviderName },
    #[error("{provider}'s API rate limit exceeded")]
    RateLimitExceeded {
        provider: LanguageModelProviderName,
        retry_after: Option<Duration>,
    },
    #[error("{provider}'s API servers are overloaded right now")]
    ServerOverloaded {
        provider: LanguageModelProviderName,
        retry_after: Option<Duration>,
    },
    #[error("{provider}'s API server reported an internal server error: {message}")]
    ApiInternalServerError {
        provider: LanguageModelProviderName,
        message: String,
    },
    #[error("{message}")]
    UpstreamProviderError {
        message: String,
        status: StatusCode,
        retry_after: Option<Duration>,
    },
    #[error("HTTP response error from {provider}'s API: status {status_code} - {message:?}")]
    HttpResponseError {
        provider: LanguageModelProviderName,
        status_code: StatusCode,
        message: String,
    },

    // Client errors
    #[error("invalid request format to {provider}'s API: {message}")]
    BadRequestFormat {
        provider: LanguageModelProviderName,
        message: String,
    },
    #[error("authentication error with {provider}'s API: {message}")]
    AuthenticationError {
        provider: LanguageModelProviderName,
        message: String,
    },
    #[error("Permission error with {provider}'s API: {message}")]
    PermissionError {
        provider: LanguageModelProviderName,
        message: String,
    },
    #[error("language model provider API endpoint not found")]
    ApiEndpointNotFound { provider: LanguageModelProviderName },
    #[error("I/O error reading response from {provider}'s API")]
    ApiReadResponseError {
        provider: LanguageModelProviderName,
        #[source]
        error: io::Error,
    },
    #[error("error serializing request to {provider} API")]
    SerializeRequest {
        provider: LanguageModelProviderName,
        #[source]
        error: serde_json::Error,
    },
    #[error("error building request body to {provider} API")]
    BuildRequestBody {
        provider: LanguageModelProviderName,
        #[source]
        error: http::Error,
    },
    #[error("error sending HTTP request to {provider} API")]
    HttpSend {
        provider: LanguageModelProviderName,
        #[source]
        error: anyhow::Error,
    },
    #[error("error deserializing {provider} API response")]
    DeserializeResponse {
        provider: LanguageModelProviderName,
        #[source]
        error: serde_json::Error,
    },

    #[error("stream from {provider} ended unexpectedly")]
    StreamEndedUnexpectedly { provider: LanguageModelProviderName },

    // TODO: Ideally this would be removed in favor of having a comprehensive list of errors.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl LanguageModelCompletionError {
    fn parse_upstream_error_json(message: &str) -> Option<(StatusCode, String)> {
        let error_json = serde_json::from_str::<serde_json::Value>(message).ok()?;
        let upstream_status = error_json
            .get("upstream_status")
            .and_then(|v| v.as_u64())
            .and_then(|status| u16::try_from(status).ok())
            .and_then(|status| StatusCode::from_u16(status).ok())?;
        let inner_message = error_json
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or(message)
            .to_string();
        Some((upstream_status, inner_message))
    }

    pub fn from_cloud_failure(
        upstream_provider: LanguageModelProviderName,
        code: String,
        message: String,
        retry_after: Option<Duration>,
    ) -> Self {
        if let Some(tokens) = parse_prompt_too_long(&message) {
            // TODO: currently Anthropic PAYLOAD_TOO_LARGE response may cause INTERNAL_SERVER_ERROR
            // to be reported. This is a temporary workaround to handle this in the case where the
            // token limit has been exceeded.
            Self::PromptTooLarge {
                tokens: Some(tokens),
            }
        } else if code == "upstream_http_error" {
            if let Some((upstream_status, inner_message)) =
                Self::parse_upstream_error_json(&message)
            {
                return Self::from_http_status(
                    upstream_provider,
                    upstream_status,
                    inner_message,
                    retry_after,
                );
            }
            anyhow!("completion request failed, code: {code}, message: {message}").into()
        } else if let Some(status_code) = code
            .strip_prefix("upstream_http_")
            .and_then(|code| StatusCode::from_str(code).ok())
        {
            Self::from_http_status(upstream_provider, status_code, message, retry_after)
        } else if let Some(status_code) = code
            .strip_prefix("http_")
            .and_then(|code| StatusCode::from_str(code).ok())
        {
            Self::from_http_status(PADDLEBOARD_CLOUD_PROVIDER_NAME, status_code, message, retry_after)
        } else {
            anyhow!("completion request failed, code: {code}, message: {message}").into()
        }
    }

    pub fn from_http_status(
        provider: LanguageModelProviderName,
        status_code: StatusCode,
        message: String,
        retry_after: Option<Duration>,
    ) -> Self {
        match status_code {
            StatusCode::BAD_REQUEST => Self::BadRequestFormat { provider, message },
            StatusCode::UNAUTHORIZED => Self::AuthenticationError { provider, message },
            StatusCode::FORBIDDEN => Self::PermissionError { provider, message },
            StatusCode::NOT_FOUND => Self::ApiEndpointNotFound { provider },
            StatusCode::PAYLOAD_TOO_LARGE => Self::PromptTooLarge {
                tokens: parse_prompt_too_long(&message),
            },
            StatusCode::TOO_MANY_REQUESTS => Self::RateLimitExceeded {
                provider,
                retry_after,
            },
            StatusCode::INTERNAL_SERVER_ERROR => Self::ApiInternalServerError { provider, message },
            StatusCode::SERVICE_UNAVAILABLE => Self::ServerOverloaded {
                provider,
                retry_after,
            },
            _ if status_code.as_u16() == 529 => Self::ServerOverloaded {
                provider,
                retry_after,
            },
            _ => Self::HttpResponseError {
                provider,
                status_code,
                message,
            },
        }
    }
}

#[derive(Debug, PartialEq, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    ToolUse,
    Refusal,
}

#[derive(Debug, PartialEq, Clone, Copy, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    #[serde(default, skip_serializing_if = "is_default")]
    pub input_tokens: u64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub output_tokens: u64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub cache_creation_input_tokens: u64,
    #[serde(default, skip_serializing_if = "is_default")]
    pub cache_read_input_tokens: u64,
}

impl TokenUsage {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens
            + self.output_tokens
            + self.cache_read_input_tokens
            + self.cache_creation_input_tokens
    }
}

impl Add<TokenUsage> for TokenUsage {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            input_tokens: self.input_tokens + other.input_tokens,
            output_tokens: self.output_tokens + other.output_tokens,
            cache_creation_input_tokens: self.cache_creation_input_tokens
                + other.cache_creation_input_tokens,
            cache_read_input_tokens: self.cache_read_input_tokens + other.cache_read_input_tokens,
        }
    }
}

impl Sub<TokenUsage> for TokenUsage {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self {
            input_tokens: self.input_tokens - other.input_tokens,
            output_tokens: self.output_tokens - other.output_tokens,
            cache_creation_input_tokens: self.cache_creation_input_tokens
                - other.cache_creation_input_tokens,
            cache_read_input_tokens: self.cache_read_input_tokens - other.cache_read_input_tokens,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Serialize, Deserialize)]
pub struct LanguageModelToolUseId(Arc<str>);

impl fmt::Display for LanguageModelToolUseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<T> From<T> for LanguageModelToolUseId
where
    T: Into<Arc<str>>,
{
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Serialize, Deserialize)]
pub struct LanguageModelToolUse {
    pub id: LanguageModelToolUseId,
    pub name: Arc<str>,
    pub raw_input: String,
    pub input: serde_json::Value,
    pub is_input_complete: bool,
    /// Thought signature the model sent us. Some models require that this
    /// signature be preserved and sent back in conversation history for validation.
    pub thought_signature: Option<String>,
}

=======
>>>>>>> zed/main
pub struct LanguageModelTextStream {
    pub message_id: Option<String>,
    pub stream: BoxStream<'static, Result<String, LanguageModelCompletionError>>,
    // Has complete token usage after the stream has finished
    pub last_token_usage: Arc<Mutex<TokenUsage>>,
}

impl Default for LanguageModelTextStream {
    fn default() -> Self {
        Self {
            message_id: None,
            stream: Box::pin(futures::stream::empty()),
            last_token_usage: Arc::new(Mutex::new(TokenUsage::default())),
        }
    }
}

pub trait LanguageModel: Send + Sync {
    fn id(&self) -> LanguageModelId;
    fn name(&self) -> LanguageModelName;
    fn provider_id(&self) -> LanguageModelProviderId;
    fn provider_name(&self) -> LanguageModelProviderName;
    fn upstream_provider_id(&self) -> LanguageModelProviderId {
        self.provider_id()
    }
    fn upstream_provider_name(&self) -> LanguageModelProviderName {
        self.provider_name()
    }

    /// Returns whether this model is the "latest", so we can highlight it in the UI.
    fn is_latest(&self) -> bool {
        false
    }

    fn telemetry_id(&self) -> String;

    fn api_key(&self, _cx: &App) -> Option<String> {
        None
    }

    /// Information about the cost of using this model, if available.
    fn model_cost_info(&self) -> Option<LanguageModelCostInfo> {
        None
    }

    /// Whether this model supports thinking.
    fn supports_thinking(&self) -> bool {
        false
    }

    fn supports_fast_mode(&self) -> bool {
        false
    }

    /// Returns the list of supported effort levels that can be used when thinking.
    fn supported_effort_levels(&self) -> Vec<LanguageModelEffortLevel> {
        Vec::new()
    }

    /// Returns the default effort level to use when thinking.
    fn default_effort_level(&self) -> Option<LanguageModelEffortLevel> {
        self.supported_effort_levels()
            .into_iter()
            .find(|effort_level| effort_level.is_default)
    }

    /// Whether this model supports images
    fn supports_images(&self) -> bool;

    /// Whether this model supports tools.
    fn supports_tools(&self) -> bool;

    /// Whether this model supports choosing which tool to use.
    fn supports_tool_choice(&self, choice: LanguageModelToolChoice) -> bool;

    /// Returns whether this model or provider supports streaming tool calls;
    fn supports_streaming_tools(&self) -> bool {
        false
    }

    /// Returns whether this model/provider reports accurate split input/output token counts.
    /// When true, the UI may show separate input/output token indicators.
    fn supports_split_token_display(&self) -> bool {
        false
    }

    fn tool_input_format(&self) -> LanguageModelToolSchemaFormat {
        LanguageModelToolSchemaFormat::JsonSchema
    }

    fn max_token_count(&self) -> u64;
    fn max_output_tokens(&self) -> Option<u64> {
        None
    }

    fn stream_completion(
        &self,
        request: LanguageModelRequest,
        cx: &AsyncApp,
    ) -> BoxFuture<
        'static,
        Result<
            BoxStream<'static, Result<LanguageModelCompletionEvent, LanguageModelCompletionError>>,
            LanguageModelCompletionError,
        >,
    >;

    fn stream_completion_text(
        &self,
        request: LanguageModelRequest,
        cx: &AsyncApp,
    ) -> BoxFuture<'static, Result<LanguageModelTextStream, LanguageModelCompletionError>> {
        let future = self.stream_completion(request, cx);

        async move {
            let events = future.await?;
            let mut events = events.fuse();
            let mut message_id = None;
            let mut first_item_text = None;
            let last_token_usage = Arc::new(Mutex::new(TokenUsage::default()));

            if let Some(first_event) = events.next().await {
                match first_event {
                    Ok(LanguageModelCompletionEvent::StartMessage { message_id: id }) => {
                        message_id = Some(id);
                    }
                    Ok(LanguageModelCompletionEvent::Text(text)) => {
                        first_item_text = Some(text);
                    }
                    _ => (),
                }
            }

            let stream = futures::stream::iter(first_item_text.map(Ok))
                .chain(events.filter_map({
                    let last_token_usage = last_token_usage.clone();
                    move |result| {
                        let last_token_usage = last_token_usage.clone();
                        async move {
                            match result {
                                Ok(LanguageModelCompletionEvent::Queued { .. }) => None,
                                Ok(LanguageModelCompletionEvent::Started) => None,
                                Ok(LanguageModelCompletionEvent::StartMessage { .. }) => None,
                                Ok(LanguageModelCompletionEvent::Text(text)) => Some(Ok(text)),
                                Ok(LanguageModelCompletionEvent::Thinking { .. }) => None,
                                Ok(LanguageModelCompletionEvent::RedactedThinking { .. }) => None,
                                Ok(LanguageModelCompletionEvent::ReasoningDetails(_)) => None,
                                Ok(LanguageModelCompletionEvent::Stop(_)) => None,
                                Ok(LanguageModelCompletionEvent::ToolUse(_)) => None,
                                Ok(LanguageModelCompletionEvent::ToolUseJsonParseError {
                                    ..
                                }) => None,
                                Ok(LanguageModelCompletionEvent::UsageUpdate(token_usage)) => {
                                    *last_token_usage.lock() = token_usage;
                                    None
                                }
                                Err(err) => Some(Err(err)),
                            }
                        }
                    }
                }))
                .boxed();

            Ok(LanguageModelTextStream {
                message_id,
                stream,
                last_token_usage,
            })
        }
        .boxed()
    }

    fn stream_completion_tool(
        &self,
        request: LanguageModelRequest,
        cx: &AsyncApp,
    ) -> BoxFuture<'static, Result<LanguageModelToolUse, LanguageModelCompletionError>> {
        let future = self.stream_completion(request, cx);

        async move {
            let events = future.await?;
            let mut events = events.fuse();

            // Iterate through events until we find a complete ToolUse
            while let Some(event) = events.next().await {
                match event {
                    Ok(LanguageModelCompletionEvent::ToolUse(tool_use))
                        if tool_use.is_input_complete =>
                    {
                        return Ok(tool_use);
                    }
                    Err(err) => {
                        return Err(err);
                    }
                    _ => {}
                }
            }

            // Stream ended without a complete tool use
            Err(LanguageModelCompletionError::Other(anyhow::anyhow!(
                "Stream ended without receiving a complete tool use"
            )))
        }
        .boxed()
    }

    #[cfg(any(test, feature = "test-support"))]
    fn as_fake(&self) -> &fake_provider::FakeLanguageModel {
        unimplemented!()
    }
}

impl std::fmt::Debug for dyn LanguageModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("<dyn LanguageModel>")
            .field("id", &self.id())
            .field("name", &self.name())
            .field("provider_id", &self.provider_id())
            .field("provider_name", &self.provider_name())
            .field("upstream_provider_name", &self.upstream_provider_name())
            .field("upstream_provider_id", &self.upstream_provider_id())
            .field("upstream_provider_id", &self.upstream_provider_id())
            .field("supports_streaming_tools", &self.supports_streaming_tools())
            .finish()
    }
}

/// Either a built-in icon name or a path to an external SVG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IconOrSvg {
    /// A built-in icon from Zed's icon set.
    Icon(IconName),
    /// Path to a custom SVG icon file.
    Svg(SharedString),
}

impl Default for IconOrSvg {
    fn default() -> Self {
        Self::Icon(IconName::ZedAssistant)
    }
}

pub trait LanguageModelProvider: 'static {
    fn id(&self) -> LanguageModelProviderId;
    fn name(&self) -> LanguageModelProviderName;
    fn icon(&self) -> IconOrSvg {
        IconOrSvg::default()
    }
    fn default_model(&self, cx: &App) -> Option<Arc<dyn LanguageModel>>;
    fn default_fast_model(&self, cx: &App) -> Option<Arc<dyn LanguageModel>>;
    fn provided_models(&self, cx: &App) -> Vec<Arc<dyn LanguageModel>>;
    fn recommended_models(&self, _cx: &App) -> Vec<Arc<dyn LanguageModel>> {
        Vec::new()
    }
    fn is_authenticated(&self, cx: &App) -> bool;
    fn authenticate(&self, cx: &mut App) -> Task<Result<(), AuthenticateError>>;
    fn configuration_view(
        &self,
        target_agent: ConfigurationViewTargetAgent,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyView;
    fn reset_credentials(&self, cx: &mut App) -> Task<Result<()>>;
}

#[derive(Default, Clone, PartialEq, Eq)]
pub enum ConfigurationViewTargetAgent {
    #[default]
    ZedAgent,
    Other(SharedString),
}

pub trait LanguageModelProviderState: 'static {
    type ObservableEntity;

    fn observable_entity(&self) -> Option<gpui::Entity<Self::ObservableEntity>>;

    fn subscribe<T: 'static>(
        &self,
        cx: &mut gpui::Context<T>,
        callback: impl Fn(&mut T, &mut gpui::Context<T>) + 'static,
    ) -> Option<gpui::Subscription> {
        let entity = self.observable_entity()?;
        Some(cx.observe(&entity, move |this, _, cx| {
            callback(this, cx);
        }))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum LanguageModelCostInfo {
    /// Cost per 1,000 input and output tokens
    TokenCost {
        input_token_cost_per_1m: f64,
        output_token_cost_per_1m: f64,
    },
    /// Cost per request
    RequestCost { cost_per_request: f64 },
}

impl LanguageModelCostInfo {
    pub fn to_shared_string(&self) -> SharedString {
        match self {
            LanguageModelCostInfo::RequestCost { cost_per_request } => {
                let cost_str = format!("{}×", Self::cost_value_to_string(cost_per_request));
                SharedString::from(cost_str)
            }
            LanguageModelCostInfo::TokenCost {
                input_token_cost_per_1m,
                output_token_cost_per_1m,
            } => {
                let input_cost = Self::cost_value_to_string(input_token_cost_per_1m);
                let output_cost = Self::cost_value_to_string(output_token_cost_per_1m);
                SharedString::from(format!("{}$/{}$", input_cost, output_cost))
            }
        }
    }

    fn cost_value_to_string(cost: &f64) -> SharedString {
        if (cost.fract() - 0.0).abs() < std::f64::EPSILON {
            SharedString::from(format!("{:.0}", cost))
        } else {
            SharedString::from(format!("{:.2}", cost))
        }
    }
}
