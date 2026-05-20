use ai_onboarding::YoungAccountBanner;
use anthropic::AnthropicModelMode;
use anyhow::{Result, anyhow};
use client::{
    Client, NeedsLlmTokenRefresh, RefreshLlmTokenListener, UserStore, global_llm_token, zed_urls,
};
use cloud_api_client::LlmApiToken;
use cloud_api_types::{OrganizationId, Plan};
use cloud_llm_client::{
    CLIENT_SUPPORTS_STATUS_MESSAGES_HEADER_NAME, CLIENT_SUPPORTS_STATUS_STREAM_ENDED_HEADER_NAME,
    CompletionBody, CompletionEvent, CompletionRequestStatus,
    PADDLEBOARD_VERSION_HEADER_NAME, SERVER_SUPPORTS_STATUS_MESSAGES_HEADER_NAME,
};
use futures::{
    AsyncBufReadExt, AsyncReadExt, FutureExt, Stream, StreamExt,
    future::BoxFuture,
    io::BufReader,
    stream::{self, BoxStream},
};
use google_ai::GoogleModelMode;
use gpui::{
    AnyElement, AnyView, App, AppContext, AsyncApp, Context, Entity, Subscription, Task, TaskExt,
};
use http_client::http::{HeaderMap, HeaderValue};
use http_client::{AsyncBody, HttpClient, HttpRequestExt, Method, Response, StatusCode};
use language_model::{
    ANTHROPIC_PROVIDER_ID, ANTHROPIC_PROVIDER_NAME, AuthenticateError, GOOGLE_PROVIDER_ID,
    GOOGLE_PROVIDER_NAME, IconOrSvg, LanguageModel, LanguageModelCompletionError,
    LanguageModelCompletionEvent, LanguageModelEffortLevel, LanguageModelId, LanguageModelName,
    LanguageModelProvider, LanguageModelProviderId, LanguageModelProviderName,
    LanguageModelProviderState, LanguageModelRequest, LanguageModelToolChoice,
    LanguageModelToolSchemaFormat, OPEN_AI_PROVIDER_ID, OPEN_AI_PROVIDER_NAME,
    PaymentRequiredError, RateLimiter, X_AI_PROVIDER_ID, X_AI_PROVIDER_NAME,
    PADDLEBOARD_CLOUD_PROVIDER_ID, PADDLEBOARD_CLOUD_PROVIDER_NAME,
};
use language_models_cloud::{CloudLlmTokenProvider, CloudModelProvider};
use release_channel::AppVersion;
use semver::Version;
use serde::de::DeserializeOwned;

use settings::SettingsStore;
pub use settings::ZedDotDevAvailableModel as AvailableModel;
pub use settings::ZedDotDevAvailableProvider as AvailableProvider;
use serde::Deserialize;
use std::collections::VecDeque;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;
use thiserror::Error;
use ui::{TintColor, prelude::*};

use crate::provider::anthropic::{AnthropicEventMapper, AnthropicPromptCacheMode, into_anthropic};
use crate::provider::google::{GoogleEventMapper, into_google};
use crate::provider::open_ai::{
    OpenAiEventMapper, OpenAiResponseEventMapper, into_open_ai, into_open_ai_response,
};

const PROVIDER_ID: LanguageModelProviderId = PADDLEBOARD_CLOUD_PROVIDER_ID;
const PROVIDER_NAME: LanguageModelProviderName = PADDLEBOARD_CLOUD_PROVIDER_NAME;

struct ClientTokenProvider {
    client: Arc<Client>,
    llm_api_token: LlmApiToken,
    user_store: Entity<UserStore>,
}

impl CloudLlmTokenProvider for ClientTokenProvider {
    type AuthContext = Option<OrganizationId>;

    fn auth_context(&self, cx: &impl AppContext) -> Self::AuthContext {
        self.user_store.read_with(cx, |user_store, _| {
            user_store
                .current_organization()
                .map(|organization| organization.id.clone())
        })
    }

    fn cached_token(
        &self,
        organization_id: Self::AuthContext,
    ) -> BoxFuture<'static, Result<String>> {
        let client = self.client.clone();
        let llm_api_token = self.llm_api_token.clone();
        Box::pin(async move {
            client
                .cached_llm_token(&llm_api_token, organization_id)
                .await
        })
    }

    fn refresh_token(
        &self,
        organization_id: Self::AuthContext,
    ) -> BoxFuture<'static, Result<String>> {
        let client = self.client.clone();
        let llm_api_token = self.llm_api_token.clone();
        Box::pin(async move {
            client
                .refresh_llm_token(&llm_api_token, organization_id)
                .await
        })
    }
}

#[derive(Default, Clone, Debug, PartialEq)]
pub struct ZedDotDevSettings {
    pub available_models: Vec<AvailableModel>,
}

pub struct CloudLanguageModelProvider {
    state: Entity<State>,
    _maintain_client_status: Task<()>,
}

pub struct State {
    client: Arc<Client>,
    user_store: Entity<UserStore>,
    status: client::Status,
    provider: Entity<CloudModelProvider<ClientTokenProvider>>,
    _user_store_subscription: Subscription,
    _settings_subscription: Subscription,
    _llm_token_subscription: Subscription,
    _provider_subscription: Subscription,
}

impl State {
    fn new(
        client: Arc<Client>,
        user_store: Entity<UserStore>,
        status: client::Status,
        cx: &mut Context<Self>,
    ) -> Self {
        let refresh_llm_token_listener = RefreshLlmTokenListener::global(cx);
        let token_provider = Arc::new(ClientTokenProvider {
            client: client.clone(),
            llm_api_token: global_llm_token(cx),
            user_store: user_store.clone(),
        });

        let provider = cx.new(|cx| {
            CloudModelProvider::new(
                token_provider.clone(),
                client.http_client(),
                Some(AppVersion::global(cx)),
            )
        });

        Self {
            client: client.clone(),
            user_store: user_store.clone(),
            status,
            _provider_subscription: cx.observe(&provider, |_, _, cx| cx.notify()),
            provider,
            _user_store_subscription: cx.subscribe(
                &user_store,
                move |this, _user_store, event, cx| match event {
                    client::user::Event::PrivateUserInfoUpdated => {
                        let status = *client.status().borrow();
                        if status.is_signed_out() {
                            return;
                        }

                        this.refresh_models(cx);
                    }
                    _ => {}
                },
            ),
            _settings_subscription: cx.observe_global::<SettingsStore>(|_, cx| {
                cx.notify();
            }),
            _llm_token_subscription: cx.subscribe(
                &refresh_llm_token_listener,
                move |this, _listener, _event, cx| {
                    this.refresh_models(cx);
                },
            ),
        }
    }

    fn is_signed_out(&self, cx: &App) -> bool {
        self.user_store.read(cx).current_user().is_none()
    }

    fn sign_in(&self, cx: &mut Context<Self>) -> Task<Result<()>> {
        let client = self.client.clone();
        let mut current_user = self.user_store.read(cx).watch_current_user();
        cx.spawn(async move |state, cx| {
            client.sign_in_with_optional_connect(true, cx).await?;
            while current_user.borrow().is_none() {
                current_user.next().await;
            }
            state.update(cx, |_, cx| {
                cx.notify();
            })
        })
    }

    fn refresh_models(&mut self, cx: &mut Context<Self>) {
        self.provider.update(cx, |provider, cx| {
            provider.refresh_models(cx).detach_and_log_err(cx);
        });
    }
}

impl CloudLanguageModelProvider {
    pub fn new(user_store: Entity<UserStore>, client: Arc<Client>, cx: &mut App) -> Self {
        let mut status_rx = client.status();
        let status = *status_rx.borrow();

        let state = cx.new(|cx| State::new(client.clone(), user_store.clone(), status, cx));

        let state_ref = state.downgrade();
        let maintain_client_status = cx.spawn(async move |cx| {
            while let Some(status) = status_rx.next().await {
                if let Some(this) = state_ref.upgrade() {
                    _ = this.update(cx, |this, cx| {
                        if this.status != status {
                            this.status = status;
                            cx.notify();
                        }
                    });
                } else {
                    break;
                }
            }
        });

        Self {
            state,
            _maintain_client_status: maintain_client_status,
        }
    }
}

impl LanguageModelProviderState for CloudLanguageModelProvider {
    type ObservableEntity = State;

    fn observable_entity(&self) -> Option<Entity<Self::ObservableEntity>> {
        Some(self.state.clone())
    }
}

impl LanguageModelProvider for CloudLanguageModelProvider {
    fn id(&self) -> LanguageModelProviderId {
        PROVIDER_ID
    }

    fn name(&self) -> LanguageModelProviderName {
        PROVIDER_NAME
    }

    fn icon(&self) -> IconOrSvg {
        IconOrSvg::Icon(IconName::AiZed)
    }

    fn default_model(&self, cx: &App) -> Option<Arc<dyn LanguageModel>> {
        let state = self.state.read(cx);
        let provider = state.provider.read(cx);
        let model = provider.default_model()?;
        Some(provider.create_model(model))
    }

    fn default_fast_model(&self, cx: &App) -> Option<Arc<dyn LanguageModel>> {
        let state = self.state.read(cx);
        let provider = state.provider.read(cx);
        let model = provider.default_fast_model()?;
        Some(provider.create_model(model))
    }

    fn recommended_models(&self, cx: &App) -> Vec<Arc<dyn LanguageModel>> {
        let state = self.state.read(cx);
        let provider = state.provider.read(cx);
        provider
            .recommended_models()
            .iter()
            .map(|model| provider.create_model(model))
            .collect()
    }

    fn provided_models(&self, cx: &App) -> Vec<Arc<dyn LanguageModel>> {
        let state = self.state.read(cx);
        let provider = state.provider.read(cx);
        provider
            .models()
            .iter()
            .map(|model| provider.create_model(model))
            .collect()
    }

    fn is_authenticated(&self, cx: &App) -> bool {
        let state = self.state.read(cx);
        !state.is_signed_out(cx)
    }

    fn authenticate(&self, cx: &mut App) -> Task<Result<(), AuthenticateError>> {
        if self.is_authenticated(cx) {
            return Task::ready(Ok(()));
        }
        let mut status = self.state.read(cx).client.status();
        let mut current_user = self.state.read(cx).user_store.read(cx).watch_current_user();
        if !status.borrow().is_signing_in() {
            return Task::ready(Ok(()));
        }
        cx.background_spawn(async move {
            while status.borrow().is_signing_in() {
                status.next().await;
            }
            while current_user.borrow().is_none() {
                let current_status = *status.borrow();
                if !matches!(
                    current_status,
                    client::Status::Authenticated
                        | client::Status::Reauthenticated
                        | client::Status::Connected { .. }
                ) {
                    return Err(AuthenticateError::Other(anyhow::anyhow!(
                        "sign-in did not complete: {current_status:?}"
                    )));
                }
                futures::select_biased! {
                    _ = current_user.next().fuse() => {},
                    _ = status.next().fuse() => {},
                }
            }
            Ok(())
        })
    }

    fn configuration_view(
        &self,
        _target_agent: language_model::ConfigurationViewTargetAgent,
        _: &mut Window,
        cx: &mut App,
    ) -> AnyView {
        cx.new(|_| ConfigurationView::new(self.state.clone()))
            .into()
    }

    fn reset_credentials(&self, _cx: &mut App) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }
}

pub struct CloudLanguageModel {
    id: LanguageModelId,
    model: Arc<cloud_llm_client::LanguageModel>,
    llm_api_token: LlmApiToken,
    user_store: Entity<UserStore>,
    client: Arc<Client>,
    request_limiter: RateLimiter,
}

struct PerformLlmCompletionResponse {
    response: Response<AsyncBody>,
    includes_status_messages: bool,
}

impl CloudLanguageModel {
    async fn perform_llm_completion(
        client: Arc<Client>,
        llm_api_token: LlmApiToken,
        organization_id: Option<OrganizationId>,
        app_version: Option<Version>,
        body: CompletionBody,
    ) -> Result<PerformLlmCompletionResponse> {
        let http_client = &client.http_client();

        let mut token = client
            .cached_llm_token(&llm_api_token, organization_id.clone())
            .await?;
        let mut refreshed_token = false;

        loop {
            let request = http_client::Request::builder()
                .method(Method::POST)
                .uri(http_client.build_zed_llm_url("/completions", &[])?.as_ref())
                .when_some(app_version.as_ref(), |builder, app_version| {
                    builder.header(PADDLEBOARD_VERSION_HEADER_NAME, app_version.to_string())
                })
                .header("Content-Type", "application/json")
                .header("Authorization", format!("Bearer {token}"))
                .header(CLIENT_SUPPORTS_STATUS_MESSAGES_HEADER_NAME, "true")
                .header(CLIENT_SUPPORTS_STATUS_STREAM_ENDED_HEADER_NAME, "true")
                .body(serde_json::to_string(&body)?.into())?;

            let mut response = http_client.send(request).await?;
            let status = response.status();
            if status.is_success() {
                let includes_status_messages = response
                    .headers()
                    .get(SERVER_SUPPORTS_STATUS_MESSAGES_HEADER_NAME)
                    .is_some();

                return Ok(PerformLlmCompletionResponse {
                    response,
                    includes_status_messages,
                });
            }

            if !refreshed_token && response.needs_llm_token_refresh() {
                token = client
                    .refresh_llm_token(&llm_api_token, organization_id.clone())
                    .await?;
                refreshed_token = true;
                continue;
            }

            if status == StatusCode::PAYMENT_REQUIRED {
                return Err(anyhow!(PaymentRequiredError));
            }

            let mut body = String::new();
            let headers = response.headers().clone();
            response.body_mut().read_to_string(&mut body).await?;
            return Err(anyhow!(ApiError {
                status,
                body,
                headers
            }));
        }
    }
}

#[derive(Debug, Error)]
#[error("cloud language model request failed with status {status}: {body}")]
struct ApiError {
    status: StatusCode,
    body: String,
    headers: HeaderMap<HeaderValue>,
}

/// Represents error responses from Zed's cloud API.
///
/// Example JSON for an upstream HTTP error:
/// ```json
/// {
///   "code": "upstream_http_error",
///   "message": "Received an error from the Anthropic API: upstream connect error or disconnect/reset before headers, reset reason: connection timeout",
///   "upstream_status": 503
/// }
/// ```
#[derive(Debug, serde::Deserialize)]
struct CloudApiError {
    code: String,
    message: String,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_optional_status_code")]
    upstream_status: Option<StatusCode>,
    #[serde(default)]
    retry_after: Option<f64>,
}

fn deserialize_optional_status_code<'de, D>(deserializer: D) -> Result<Option<StatusCode>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<u16> = Option::deserialize(deserializer)?;
    Ok(opt.and_then(|code| StatusCode::from_u16(code).ok()))
}

impl From<ApiError> for LanguageModelCompletionError {
    fn from(error: ApiError) -> Self {
        if let Ok(cloud_error) = serde_json::from_str::<CloudApiError>(&error.body) {
            if cloud_error.code.starts_with("upstream_http_") {
                let status = if let Some(status) = cloud_error.upstream_status {
                    status
                } else if cloud_error.code.ends_with("_error") {
                    error.status
                } else {
                    // If there's a status code in the code string (e.g. "upstream_http_429")
                    // then use that; otherwise, see if the JSON contains a status code.
                    cloud_error
                        .code
                        .strip_prefix("upstream_http_")
                        .and_then(|code_str| code_str.parse::<u16>().ok())
                        .and_then(|code| StatusCode::from_u16(code).ok())
                        .unwrap_or(error.status)
                };

                return LanguageModelCompletionError::UpstreamProviderError {
                    message: cloud_error.message,
                    status,
                    retry_after: cloud_error.retry_after.map(Duration::from_secs_f64),
                };
            }

            return LanguageModelCompletionError::from_http_status(
                PROVIDER_NAME,
                error.status,
                cloud_error.message,
                None,
            );
        }

        let retry_after = None;
        LanguageModelCompletionError::from_http_status(
            PROVIDER_NAME,
            error.status,
            error.body,
            retry_after,
        )
    }
}

impl LanguageModel for CloudLanguageModel {
    fn id(&self) -> LanguageModelId {
        self.id.clone()
    }

    fn name(&self) -> LanguageModelName {
        LanguageModelName::from(self.model.display_name.clone())
    }

    fn provider_id(&self) -> LanguageModelProviderId {
        PROVIDER_ID
    }

    fn provider_name(&self) -> LanguageModelProviderName {
        PROVIDER_NAME
    }

    fn upstream_provider_id(&self) -> LanguageModelProviderId {
        use cloud_llm_client::LanguageModelProvider::*;
        match self.model.provider {
            Anthropic => ANTHROPIC_PROVIDER_ID,
            OpenAi => OPEN_AI_PROVIDER_ID,
            Google => GOOGLE_PROVIDER_ID,
            XAi => X_AI_PROVIDER_ID,
        }
    }

    fn upstream_provider_name(&self) -> LanguageModelProviderName {
        use cloud_llm_client::LanguageModelProvider::*;
        match self.model.provider {
            Anthropic => ANTHROPIC_PROVIDER_NAME,
            OpenAi => OPEN_AI_PROVIDER_NAME,
            Google => GOOGLE_PROVIDER_NAME,
            XAi => X_AI_PROVIDER_NAME,
        }
    }

    fn is_latest(&self) -> bool {
        self.model.is_latest
    }

    fn supports_tools(&self) -> bool {
        self.model.supports_tools
    }

    fn supports_images(&self) -> bool {
        self.model.supports_images
    }

    fn supports_thinking(&self) -> bool {
        self.model.supports_thinking
    }

    fn supports_fast_mode(&self) -> bool {
        self.model.supports_fast_mode
    }

    fn supported_effort_levels(&self) -> Vec<LanguageModelEffortLevel> {
        self.model
            .supported_effort_levels
            .iter()
            .map(|effort_level| LanguageModelEffortLevel {
                name: effort_level.name.clone().into(),
                value: effort_level.value.clone().into(),
                is_default: effort_level.is_default.unwrap_or(false),
            })
            .collect()
    }

    fn supports_streaming_tools(&self) -> bool {
        self.model.supports_streaming_tools
    }

    fn supports_tool_choice(&self, choice: LanguageModelToolChoice) -> bool {
        match choice {
            LanguageModelToolChoice::Auto
            | LanguageModelToolChoice::Any
            | LanguageModelToolChoice::None => true,
        }
    }

    fn supports_split_token_display(&self) -> bool {
        use cloud_llm_client::LanguageModelProvider::*;
        matches!(self.model.provider, OpenAi | XAi)
    }

    fn telemetry_id(&self) -> String {
        format!("zed.dev/{}", self.model.id)
    }

    fn tool_input_format(&self) -> LanguageModelToolSchemaFormat {
        match self.model.provider {
            cloud_llm_client::LanguageModelProvider::Anthropic
            | cloud_llm_client::LanguageModelProvider::OpenAi => {
                LanguageModelToolSchemaFormat::JsonSchema
            }
            cloud_llm_client::LanguageModelProvider::Google
            | cloud_llm_client::LanguageModelProvider::XAi => {
                LanguageModelToolSchemaFormat::JsonSchemaSubset
            }
        }
    }

    fn max_token_count(&self) -> u64 {
        self.model.max_token_count as u64
    }

    fn max_output_tokens(&self) -> Option<u64> {
        Some(self.model.max_output_tokens as u64)
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
    > {
        let thread_id = request.thread_id.clone();
        let prompt_id = request.prompt_id.clone();
        let app_version = Some(cx.update(|cx| AppVersion::global(cx)));
        let user_store = self.user_store.clone();
        let organization_id = cx.update(|cx| {
            user_store
                .read(cx)
                .current_organization()
                .map(|organization| organization.id.clone())
        });
        let thinking_allowed = request.thinking_allowed;
        let enable_thinking = thinking_allowed && self.model.supports_thinking;
        let provider_name = provider_name(&self.model.provider);
        match self.model.provider {
            cloud_llm_client::LanguageModelProvider::Anthropic => {
                let effort = request
                    .thinking_effort
                    .as_ref()
                    .and_then(|effort| anthropic::Effort::from_str(effort).ok());

                let mut request = into_anthropic(
                    request,
                    self.model.id.to_string(),
                    1.0,
                    self.model.max_output_tokens as u64,
                    if enable_thinking {
                        AnthropicModelMode::Thinking {
                            budget_tokens: Some(4_096),
                        }
                    } else {
                        AnthropicModelMode::Default
                    },
                    AnthropicPromptCacheMode::Automatic,
                );

                if enable_thinking && effort.is_some() {
                    request.thinking = Some(anthropic::Thinking::Adaptive {
                        display: Some(anthropic::AdaptiveThinkingDisplay::Summarized),
                    });
                    request.output_config = Some(anthropic::OutputConfig { effort });
                }

                let client = self.client.clone();
                let llm_api_token = self.llm_api_token.clone();
                let organization_id = organization_id.clone();
                let future = self.request_limiter.stream(async move {
                    let PerformLlmCompletionResponse {
                        response,
                        includes_status_messages,
                    } = Self::perform_llm_completion(
                        client.clone(),
                        llm_api_token,
                        organization_id,
                        app_version,
                        CompletionBody {
                            thread_id,
                            prompt_id,
                            provider: cloud_llm_client::LanguageModelProvider::Anthropic,
                            model: request.model.clone(),
                            provider_request: serde_json::to_value(&request)
                                .map_err(|e| anyhow!(e))?,
                        },
                    )
                    .await
                    .map_err(|err| match err.downcast::<ApiError>() {
                        Ok(api_err) => anyhow!(LanguageModelCompletionError::from(api_err)),
                        Err(err) => anyhow!(err),
                    })?;

                    let mut mapper = AnthropicEventMapper::new();
                    Ok(map_cloud_completion_events(
                        Box::pin(response_lines(response, includes_status_messages)),
                        &provider_name,
                        move |event| mapper.map_event(event),
                    ))
                });
                async move { Ok(future.await?.boxed()) }.boxed()
            }
            cloud_llm_client::LanguageModelProvider::OpenAi => {
                let client = self.client.clone();
                let llm_api_token = self.llm_api_token.clone();
                let organization_id = organization_id.clone();
                let effort = request
                    .thinking_effort
                    .as_ref()
                    .and_then(|effort| open_ai::ReasoningEffort::from_str(effort).ok())
                    .filter(|effort| *effort != open_ai::ReasoningEffort::None);
                let supports_none_reasoning_effort =
                    self.model.supported_effort_levels.iter().any(|effort| {
                        open_ai::ReasoningEffort::from_str(&effort.value)
                            .is_ok_and(|effort| effort == open_ai::ReasoningEffort::None)
                    });

                let mut request = into_open_ai_response(
                    request,
                    &self.model.id.0,
                    self.model.supports_parallel_tool_calls,
                    true,
                    None,
                    None,
                    supports_none_reasoning_effort,
                );

                if enable_thinking && let Some(effort) = effort {
                    request.reasoning = Some(open_ai::responses::ReasoningConfig {
                        effort,
                        summary: Some(open_ai::responses::ReasoningSummaryMode::Auto),
                    });
                }

                let future = self.request_limiter.stream(async move {
                    let PerformLlmCompletionResponse {
                        response,
                        includes_status_messages,
                    } = Self::perform_llm_completion(
                        client.clone(),
                        llm_api_token,
                        organization_id,
                        app_version,
                        CompletionBody {
                            thread_id,
                            prompt_id,
                            provider: cloud_llm_client::LanguageModelProvider::OpenAi,
                            model: request.model.clone(),
                            provider_request: serde_json::to_value(&request)
                                .map_err(|e| anyhow!(e))?,
                        },
                    )
                    .await?;

                    let mut mapper = OpenAiResponseEventMapper::new();
                    Ok(map_cloud_completion_events(
                        Box::pin(response_lines(response, includes_status_messages)),
                        &provider_name,
                        move |event| mapper.map_event(event),
                    ))
                });
                async move { Ok(future.await?.boxed()) }.boxed()
            }
            cloud_llm_client::LanguageModelProvider::XAi => {
                let client = self.client.clone();
                let request = into_open_ai(
                    request,
                    &self.model.id.0,
                    self.model.supports_parallel_tool_calls,
                    false,
                    None,
                    None,
                    false,
                );
                let llm_api_token = self.llm_api_token.clone();
                let organization_id = organization_id.clone();
                let future = self.request_limiter.stream(async move {
                    let PerformLlmCompletionResponse {
                        response,
                        includes_status_messages,
                    } = Self::perform_llm_completion(
                        client.clone(),
                        llm_api_token,
                        organization_id,
                        app_version,
                        CompletionBody {
                            thread_id,
                            prompt_id,
                            provider: cloud_llm_client::LanguageModelProvider::XAi,
                            model: request.model.clone(),
                            provider_request: serde_json::to_value(&request)
                                .map_err(|e| anyhow!(e))?,
                        },
                    )
                    .await?;

                    let mut mapper = OpenAiEventMapper::new();
                    Ok(map_cloud_completion_events(
                        Box::pin(response_lines(response, includes_status_messages)),
                        &provider_name,
                        move |event| mapper.map_event(event),
                    ))
                });
                async move { Ok(future.await?.boxed()) }.boxed()
            }
            cloud_llm_client::LanguageModelProvider::Google => {
                let client = self.client.clone();
                let request =
                    into_google(request, self.model.id.to_string(), GoogleModelMode::Default);
                let llm_api_token = self.llm_api_token.clone();
                let future = self.request_limiter.stream(async move {
                    let PerformLlmCompletionResponse {
                        response,
                        includes_status_messages,
                    } = Self::perform_llm_completion(
                        client.clone(),
                        llm_api_token,
                        organization_id,
                        app_version,
                        CompletionBody {
                            thread_id,
                            prompt_id,
                            provider: cloud_llm_client::LanguageModelProvider::Google,
                            model: request.model.model_id.clone(),
                            provider_request: serde_json::to_value(&request)
                                .map_err(|e| anyhow!(e))?,
                        },
                    )
                    .await?;

                    let mut mapper = GoogleEventMapper::new();
                    Ok(map_cloud_completion_events(
                        Box::pin(response_lines(response, includes_status_messages)),
                        &provider_name,
                        move |event| mapper.map_event(event),
                    ))
                });
                async move { Ok(future.await?.boxed()) }.boxed()
            }
        }
    }
}

fn map_cloud_completion_events<T, F>(
    stream: Pin<Box<dyn Stream<Item = Result<CompletionEvent<T>>> + Send>>,
    provider: &LanguageModelProviderName,
    mut map_callback: F,
) -> BoxStream<'static, Result<LanguageModelCompletionEvent, LanguageModelCompletionError>>
where
    T: DeserializeOwned + 'static,
    F: FnMut(T) -> Vec<Result<LanguageModelCompletionEvent, LanguageModelCompletionError>>
        + Send
        + 'static,
{
    let provider = provider.clone();
    let mut stream = stream.fuse();

    let mut saw_stream_ended = false;

    let mut done = false;
    let mut pending = VecDeque::new();

    stream::poll_fn(move |cx| {
        loop {
            if let Some(item) = pending.pop_front() {
                return Poll::Ready(Some(item));
            }

            if done {
                return Poll::Ready(None);
            }

            match stream.poll_next_unpin(cx) {
                Poll::Ready(Some(event)) => {
                    let items = match event {
                        Err(error) => {
                            vec![Err(LanguageModelCompletionError::from(error))]
                        }
                        Ok(CompletionEvent::Status(CompletionRequestStatus::StreamEnded)) => {
                            saw_stream_ended = true;
                            vec![]
                        }
                        Ok(CompletionEvent::Status(status)) => {
                            LanguageModelCompletionEvent::from_completion_request_status(
                                status,
                                provider.clone(),
                            )
                            .transpose()
                            .map(|event| vec![event])
                            .unwrap_or_default()
                        }
                        Ok(CompletionEvent::Event(event)) => map_callback(event),
                    };
                    pending.extend(items);
                }
                Poll::Ready(None) => {
                    done = true;

                    if !saw_stream_ended {
                        return Poll::Ready(Some(Err(
                            LanguageModelCompletionError::StreamEndedUnexpectedly {
                                provider: provider.clone(),
                            },
                        )));
                    }
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    })
    .boxed()
}

fn provider_name(provider: &cloud_llm_client::LanguageModelProvider) -> LanguageModelProviderName {
    match provider {
        cloud_llm_client::LanguageModelProvider::Anthropic => ANTHROPIC_PROVIDER_NAME,
        cloud_llm_client::LanguageModelProvider::OpenAi => OPEN_AI_PROVIDER_NAME,
        cloud_llm_client::LanguageModelProvider::Google => GOOGLE_PROVIDER_NAME,
        cloud_llm_client::LanguageModelProvider::XAi => X_AI_PROVIDER_NAME,
    }
}

fn response_lines<T: DeserializeOwned>(
    response: Response<AsyncBody>,
    includes_status_messages: bool,
) -> impl Stream<Item = Result<CompletionEvent<T>>> {
    futures::stream::try_unfold(
        (String::new(), BufReader::new(response.into_body())),
        move |(mut line, mut body)| async move {
            match body.read_line(&mut line).await {
                Ok(0) => Ok(None),
                Ok(_) => {
                    let event = if includes_status_messages {
                        serde_json::from_str::<CompletionEvent<T>>(&line)?
                    } else {
                        CompletionEvent::Event(serde_json::from_str::<T>(&line)?)
                    };

                    line.clear();
                    Ok(Some((event, (line, body))))
                }
                Err(e) => Err(e.into()),
            }
        },
    )
}

#[derive(IntoElement, RegisterComponent)]
struct ZedAiConfiguration {
    is_connected: bool,
    plan: Option<Plan>,
    is_zed_model_provider_enabled: bool,
    eligible_for_trial: bool,
    account_too_young: bool,
    sign_in_callback: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>,
}

impl RenderOnce for ZedAiConfiguration {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let (subscription_text, has_paid_plan) = match self.plan {
            Some(Plan::ZedPro) => (
                "You have access to Zed's hosted models through your Pro subscription.",
                true,
            ),
            Some(Plan::ZedProTrial) => (
                "You have access to Zed's hosted models through your Pro trial.",
                false,
            ),
            Some(Plan::ZedStudent) => (
                "You have access to Zed's hosted models through your Student subscription.",
                true,
            ),
            Some(Plan::ZedBusiness) => (
                if self.is_zed_model_provider_enabled {
                    "You have access to Zed's hosted models through your organization."
                } else {
                    "Zed's hosted models are disabled by your organization's configuration."
                },
                true,
            ),
            Some(Plan::ZedFree) | None => (
                if self.eligible_for_trial {
                    "Subscribe for access to Zed's hosted models. Start with a 14 day free trial."
                } else {
                    "Subscribe for access to Zed's hosted models."
                },
                false,
            ),
        };

        let manage_subscription_buttons = if has_paid_plan {
            Button::new("manage_settings", "Manage Subscription")
                .full_width()
                .label_size(LabelSize::Small)
                .style(ButtonStyle::Tinted(TintColor::Accent))
                .on_click(|_, _, cx| cx.open_url(&zed_urls::account_url(cx)))
                .into_any_element()
        } else if self.plan.is_none() || self.eligible_for_trial {
            Button::new("start_trial", "Start 14-day Free Pro Trial")
                .full_width()
                .style(ui::ButtonStyle::Tinted(ui::TintColor::Accent))
                .on_click(|_, _, cx| cx.open_url(&zed_urls::start_trial_url(cx)))
                .into_any_element()
        } else {
            Button::new("upgrade", "Upgrade to Pro")
                .full_width()
                .style(ui::ButtonStyle::Tinted(ui::TintColor::Accent))
                .on_click(|_, _, cx| cx.open_url(&zed_urls::upgrade_to_zed_pro_url(cx)))
                .into_any_element()
        };

        if !self.is_connected {
            return v_flex()
                .gap_2()
                .child(Label::new("Sign in to have access to Zed's complete agentic experience with hosted models."))
                .child(
                    Button::new("sign_in", "Sign In to use PaddleBoard AI")
                        .start_icon(Icon::new(IconName::Github).size(IconSize::Small).color(Color::Muted))
                        .full_width()
                        .on_click({
                            let callback = self.sign_in_callback.clone();
                            move |_, window, cx| (callback)(window, cx)
                        }),
                );
        }

        v_flex().gap_2().w_full().map(|this| {
            if self.account_too_young {
                this.child(YoungAccountBanner).child(
                    Button::new("upgrade", "Upgrade to Pro")
                        .style(ui::ButtonStyle::Tinted(ui::TintColor::Accent))
                        .full_width()
                        .on_click(|_, _, cx| cx.open_url(&zed_urls::upgrade_to_zed_pro_url(cx))),
                )
            } else {
                this.text_sm()
                    .child(subscription_text)
                    .child(manage_subscription_buttons)
            }
        })
    }
}

struct ConfigurationView {
    state: Entity<State>,
    sign_in_callback: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>,
}

impl ConfigurationView {
    fn new(state: Entity<State>) -> Self {
        let sign_in_callback = Arc::new({
            let state = state.clone();
            move |_window: &mut Window, cx: &mut App| {
                state.update(cx, |state, cx| {
                    state.sign_in(cx).detach_and_log_err(cx);
                });
            }
        });

        Self {
            state,
            sign_in_callback,
        }
    }
}

impl Render for ConfigurationView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let user_store = state.user_store.read(cx);

        let is_zed_model_provider_enabled = user_store
            .current_organization_configuration()
            .map_or(true, |config| config.is_zed_model_provider_enabled);

        ZedAiConfiguration {
            is_connected: !state.is_signed_out(cx),
            plan: user_store.plan(),
            is_zed_model_provider_enabled,
            eligible_for_trial: user_store.trial_started_at().is_none(),
            account_too_young: user_store.account_too_young(),
            sign_in_callback: self.sign_in_callback.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use client::{Credentials, test::make_get_authenticated_user_response};
    use clock::FakeSystemClock;
    use feature_flags::FeatureFlagAppExt as _;
    use gpui::TestAppContext;
    use http_client::{FakeHttpClient, Method, Response};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    const TEST_USER_ID: u64 = 42;

    fn init_test(cx: &mut App) -> (Arc<Client>, Entity<UserStore>, CloudLanguageModelProvider) {
        let settings_store = SettingsStore::test(cx);
        cx.set_global(settings_store);
        cx.set_global(db::AppDatabase::test_new());
        let app_version = AppVersion::global(cx);
        release_channel::init_test(app_version, release_channel::ReleaseChannel::Dev, cx);
        gpui_tokio::init(cx);
        cx.update_flags(false, Vec::new());

        let client = Client::new(
            Arc::new(FakeSystemClock::new()),
            FakeHttpClient::with_404_response(),
            cx,
        );
        let user_store = cx.new(|cx| UserStore::new(client.clone(), cx));
        RefreshLlmTokenListener::register(client.clone(), user_store.clone(), cx);
        let provider = CloudLanguageModelProvider::new(user_store.clone(), client.clone(), cx);

        (client, user_store, provider)
    }

    fn override_authenticate(
        client: &Arc<Client>,
        authenticate_rx: futures::channel::oneshot::Receiver<anyhow::Result<Credentials>>,
    ) {
        let authenticate_rx = Arc::new(Mutex::new(Some(authenticate_rx)));
        client.override_authenticate(move |cx| {
            let authenticate_rx = authenticate_rx.clone();
            cx.background_spawn(async move {
                let authenticate_rx = authenticate_rx
                    .lock()
                    .expect("authenticate receiver lock poisoned")
                    .take()
                    .expect("authenticate receiver already used");
                authenticate_rx.await?
            })
        });
    }

    fn respond_to_authenticated_user_after(
        client: &Arc<Client>,
        authenticated_user_rx: futures::channel::oneshot::Receiver<()>,
    ) {
        let authenticated_user_rx = Arc::new(Mutex::new(Some(authenticated_user_rx)));
        client
            .http_client()
            .as_fake()
            .replace_handler(move |old_handler, request| {
                let authenticated_user_rx = authenticated_user_rx.clone();
                async move {
                    if request.method() == Method::GET && request.uri().path() == "/client/users/me"
                    {
                        let authenticated_user_rx = authenticated_user_rx
                            .lock()
                            .expect("authenticated user receiver lock poisoned")
                            .take();
                        if let Some(authenticated_user_rx) = authenticated_user_rx {
                            authenticated_user_rx.await.ok();
                        }

                        return Ok(Response::builder()
                            .status(200)
                            .body(
                                serde_json::to_string(&make_get_authenticated_user_response(
                                    TEST_USER_ID as i32,
                                    format!("user-{TEST_USER_ID}"),
                                ))
                                .expect("failed to serialize authenticated user response")
                                .into(),
                            )
                            .expect("failed to build authenticated user response"));
                    }

                    old_handler(request).await
                }
            });
    }

    async fn sign_in_until_authenticating(
        client: Arc<Client>,
        cx: &mut TestAppContext,
    ) -> Task<anyhow::Result<Credentials>> {
        let mut status = client.status();
        let sign_in_task = cx.update(|cx| {
            cx.spawn({
                let client = client.clone();
                async move |cx| client.sign_in(false, cx).await
            })
        });

        while !status.borrow().is_signing_in() {
            status.next().await;
        }

        sign_in_task
    }

    #[gpui::test]
    async fn provider_authenticate_does_not_start_sign_in_when_signed_out(cx: &mut TestAppContext) {
        let (client, _user_store, provider) = cx.update(init_test);
        let authenticate_calls = Arc::new(AtomicUsize::new(0));
        client.override_authenticate({
            let authenticate_calls = authenticate_calls.clone();
            move |_| {
                authenticate_calls.fetch_add(1, Ordering::SeqCst);
                Task::ready(Err(anyhow::anyhow!(
                    "provider authenticate should not start sign-in"
                )))
            }
        });

        assert!(!cx.read(|cx| provider.is_authenticated(cx)));
        assert!(matches!(
            *client.status().borrow(),
            client::Status::SignedOut
        ));

        cx.update(|cx| provider.authenticate(cx))
            .now_or_never()
            .expect("authenticate should return immediately when signed out")
            .expect("authenticate should not fail when no sign-in is in progress");
        cx.executor().run_until_parked();

        assert_eq!(authenticate_calls.load(Ordering::SeqCst), 0);
        assert!(matches!(
            *client.status().borrow(),
            client::Status::SignedOut
        ));
        assert!(!cx.read(|cx| provider.is_authenticated(cx)));
    }

    #[gpui::test]
    async fn provider_authenticate_waits_for_current_user(cx: &mut TestAppContext) {
        let (client, _user_store, provider) = cx.update(init_test);
        let (authenticate_tx, authenticate_rx) = futures::channel::oneshot::channel();
        let (authenticated_user_tx, authenticated_user_rx) = futures::channel::oneshot::channel();
        override_authenticate(&client, authenticate_rx);
        respond_to_authenticated_user_after(&client, authenticated_user_rx);

        let sign_in_task = sign_in_until_authenticating(client.clone(), cx).await;
        let authenticate_task = cx.update(|cx| provider.authenticate(cx));
        authenticate_tx
            .send(Ok(Credentials {
                user_id: TEST_USER_ID,
                access_token: "token".to_string(),
            }))
            .expect("authenticate receiver dropped");

        cx.executor().run_until_parked();
        assert!(!cx.read(|cx| provider.is_authenticated(cx)));

        authenticated_user_tx
            .send(())
            .expect("authenticated user receiver dropped");
        sign_in_task
            .await
            .expect("sign-in should complete after user response");
        authenticate_task
            .await
            .expect("provider authentication should complete after current user is populated");
        assert!(cx.read(|cx| provider.is_authenticated(cx)));

        cx.update(|cx| provider.authenticate(cx))
            .now_or_never()
            .expect("already-authenticated provider should authenticate immediately")
            .unwrap();
    }

    #[gpui::test]
    async fn provider_authenticate_returns_error_when_sign_in_fails(cx: &mut TestAppContext) {
        let (client, _user_store, provider) = cx.update(init_test);
        let (authenticate_tx, authenticate_rx) = futures::channel::oneshot::channel();
        override_authenticate(&client, authenticate_rx);

        let sign_in_task = sign_in_until_authenticating(client.clone(), cx).await;
        let authenticate_task = cx.update(|cx| provider.authenticate(cx));
        authenticate_tx
            .send(Err(anyhow::anyhow!("test authentication failed")))
            .expect("authenticate receiver dropped");

        sign_in_task
            .await
            .expect_err("sign-in should report authentication failure");
        let error = authenticate_task
            .await
            .expect_err("provider authentication should fail when sign-in fails");
        assert!(error.to_string().contains("AuthenticationError"));
    }
}

impl Component for ZedAiConfiguration {
    fn name() -> &'static str {
        "AI Configuration Content"
    }

    fn sort_name() -> &'static str {
        "AI Configuration Content"
    }

    fn scope() -> ComponentScope {
        ComponentScope::Onboarding
    }

    fn preview(_window: &mut Window, _cx: &mut App) -> Option<AnyElement> {
        struct PreviewConfiguration {
            plan: Option<Plan>,
            is_connected: bool,
            is_zed_model_provider_enabled: bool,
            eligible_for_trial: bool,
        }

        let configuration = |config: PreviewConfiguration| -> AnyElement {
            ZedAiConfiguration {
                is_connected: config.is_connected,
                plan: config.plan,
                is_zed_model_provider_enabled: config.is_zed_model_provider_enabled,
                eligible_for_trial: config.eligible_for_trial,
                account_too_young: false,
                sign_in_callback: Arc::new(|_, _| {}),
            }
            .into_any_element()
        };

        Some(
            v_flex()
                .p_4()
                .gap_4()
                .children(vec![
                    single_example(
                        "Not connected",
                        configuration(PreviewConfiguration {
                            plan: None,
                            is_connected: false,
                            is_zed_model_provider_enabled: true,
                            eligible_for_trial: false,
                        }),
                    ),
                    single_example(
                        "Accept Terms of Service",
                        configuration(PreviewConfiguration {
                            plan: None,
                            is_connected: true,
                            is_zed_model_provider_enabled: true,
                            eligible_for_trial: true,
                        }),
                    ),
                    single_example(
                        "No Plan - Not eligible for trial",
                        configuration(PreviewConfiguration {
                            plan: None,
                            is_connected: true,
                            is_zed_model_provider_enabled: true,
                            eligible_for_trial: false,
                        }),
                    ),
                    single_example(
                        "No Plan - Eligible for trial",
                        configuration(PreviewConfiguration {
                            plan: None,
                            is_connected: true,
                            is_zed_model_provider_enabled: true,
                            eligible_for_trial: true,
                        }),
                    ),
                    single_example(
                        "Free Plan",
                        configuration(PreviewConfiguration {
                            plan: Some(Plan::ZedFree),
                            is_connected: true,
                            is_zed_model_provider_enabled: true,
                            eligible_for_trial: true,
                        }),
                    ),
                    single_example(
                        "Zed Pro Trial Plan",
                        configuration(PreviewConfiguration {
                            plan: Some(Plan::ZedProTrial),
                            is_connected: true,
                            is_zed_model_provider_enabled: true,
                            eligible_for_trial: true,
                        }),
                    ),
                    single_example(
                        "Zed Pro Plan",
                        configuration(PreviewConfiguration {
                            plan: Some(Plan::ZedPro),
                            is_connected: true,
                            is_zed_model_provider_enabled: true,
                            eligible_for_trial: true,
                        }),
                    ),
                    single_example(
                        "Business Plan - Zed models enabled",
                        configuration(PreviewConfiguration {
                            plan: Some(Plan::ZedBusiness),
                            is_connected: true,
                            is_zed_model_provider_enabled: true,
                            eligible_for_trial: false,
                        }),
                    ),
                    single_example(
                        "Business Plan - Zed models disabled",
                        configuration(PreviewConfiguration {
                            plan: Some(Plan::ZedBusiness),
                            is_connected: true,
                            is_zed_model_provider_enabled: false,
                            eligible_for_trial: false,
                        }),
                    ),
                ])
                .into_any_element(),
        )
    }
}
