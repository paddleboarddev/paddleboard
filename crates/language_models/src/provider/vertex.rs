// PaddleBoard: Google Vertex AI (Gemini Enterprise) provider. Reuses the
// `google_ai` request/response schema; the Vertex URL + service-account OAuth
// live in the `paddleboard_vertex` crate. This is a PaddleBoard addition over
// upstream Zed — keep it on upstream merges.

use anyhow::{Context as _, Result, bail};
use collections::BTreeMap;
use credentials_provider::CredentialsProvider;
use futures::{FutureExt, StreamExt, future::BoxFuture};
use google_ai::GenerateContentResponse;
use google_ai::completion::{GoogleEventMapper, into_google};
use gpui::{AnyView, App, AsyncApp, Context, Entity, SharedString, Task, Window};
use http_client::HttpClient;
use language_model::{
    ApiKeyState, AuthenticateError, ConfigurationViewTargetAgent, EnvVar, IconOrSvg, LanguageModel,
    LanguageModelCompletionError, LanguageModelCompletionEvent, LanguageModelId, LanguageModelName,
    LanguageModelProvider, LanguageModelProviderId, LanguageModelProviderName,
    LanguageModelProviderState, LanguageModelRequest, LanguageModelToolChoice,
    LanguageModelToolSchemaFormat, RateLimiter,
};
use paddleboard_vertex::{ServiceAccountKey, TokenProvider, VertexAuth, stream_generate_content};
use settings::{Settings, SettingsStore};
pub use settings::VertexAvailableModel as AvailableModel;
use std::sync::{Arc, LazyLock};
use strum::IntoEnumIterator;
use ui::{ConfiguredApiCard, List, ListBulletItem, prelude::*};
use ui_input::InputField;
use util::ResultExt;

const PROVIDER_ID: LanguageModelProviderId = LanguageModelProviderId::new("vertex");
const PROVIDER_NAME: LanguageModelProviderName =
    LanguageModelProviderName::new("Vertex AI (Gemini Enterprise)");

/// Marker URL the express API key is associated with in the keychain.
const EXPRESS_KEY_URL: &str = "https://aiplatform.googleapis.com";
const DEFAULT_LOCATION: &str = "us-central1";
const VERTEX_API_KEY_VAR: &str = "VERTEX_API_KEY";

static API_KEY_ENV_VAR: LazyLock<EnvVar> = LazyLock::new(|| EnvVar::new(VERTEX_API_KEY_VAR.into()));

#[derive(Default, Clone, Debug, PartialEq)]
pub struct VertexSettings {
    pub project_id: Option<String>,
    pub location: Option<String>,
    pub credentials_path: Option<String>,
    pub available_models: Vec<AvailableModel>,
}

pub struct VertexLanguageModelProvider {
    http_client: Arc<dyn HttpClient>,
    state: Entity<State>,
}

pub struct State {
    api_key_state: ApiKeyState,
    credentials_provider: Arc<dyn CredentialsProvider>,
    /// Cached service-account token minter, keyed by the credentials file path
    /// it was built from (so the OAuth token cache survives across requests).
    token_provider: Option<(String, Arc<TokenProvider>)>,
    sa_error: Option<SharedString>,
}

impl State {
    fn is_authenticated(&self) -> bool {
        self.token_provider.is_some() || self.api_key_state.has_key()
    }

    /// Resolves the auth method to use for a request: service account if a key
    /// file is loaded, otherwise the Express API key.
    fn resolve_auth(&self) -> Result<VertexAuth> {
        if let Some((_, provider)) = &self.token_provider {
            Ok(VertexAuth::ServiceAccount(provider.clone()))
        } else if let Some(key) = self.api_key_state.key(EXPRESS_KEY_URL) {
            Ok(VertexAuth::ApiKey(key.to_string()))
        } else {
            bail!(
                "Vertex AI (Gemini Enterprise) is not configured (set a service-account key file or an Express API key)"
            )
        }
    }

    fn set_api_key(&mut self, api_key: Option<String>, cx: &mut Context<Self>) -> Task<Result<()>> {
        let credentials_provider = self.credentials_provider.clone();
        self.api_key_state.store(
            EXPRESS_KEY_URL.into(),
            api_key,
            |this| &mut this.api_key_state,
            credentials_provider,
            cx,
        )
    }

    fn authenticate(&mut self, cx: &mut Context<Self>) -> Task<Result<(), AuthenticateError>> {
        let credentials_provider = self.credentials_provider.clone();
        let express = self.api_key_state.load_if_needed(
            EXPRESS_KEY_URL.into(),
            |this| &mut this.api_key_state,
            credentials_provider,
            cx,
        );
        let credentials_path = VertexLanguageModelProvider::settings(cx)
            .credentials_path
            .clone();

        cx.spawn(async move |this, cx| {
            let express_result = express.await;

            if let Some(path) = credentials_path {
                let read = cx
                    .background_spawn({
                        let path = path.clone();
                        async move { std::fs::read_to_string(&path) }
                    })
                    .await;
                let loaded = read
                    .with_context(|| format!("reading service-account key at {path}"))
                    .and_then(|json| ServiceAccountKey::from_json(&json));
                this.update(cx, |this, _| match loaded {
                    Ok(key) => {
                        this.token_provider = Some((path, Arc::new(TokenProvider::new(key))));
                        this.sa_error = None;
                    }
                    Err(error) => {
                        this.token_provider = None;
                        this.sa_error = Some(error.to_string().into());
                    }
                })
                .ok();
            }

            // Authenticated if either the service account or the Express key loaded;
            // otherwise surface the Express key's "not found" error.
            let authenticated = this
                .read_with(cx, |this, _| this.is_authenticated())
                .unwrap_or(false);
            if authenticated { Ok(()) } else { express_result }
        })
    }
}

impl VertexLanguageModelProvider {
    pub fn new(
        http_client: Arc<dyn HttpClient>,
        credentials_provider: Arc<dyn CredentialsProvider>,
        cx: &mut App,
    ) -> Self {
        let state = cx.new(|cx| {
            cx.observe_global::<SettingsStore>(|this: &mut State, cx| {
                let credentials_provider = this.credentials_provider.clone();
                this.api_key_state.handle_url_change(
                    EXPRESS_KEY_URL.into(),
                    |this| &mut this.api_key_state,
                    credentials_provider,
                    cx,
                );
                cx.notify();
            })
            .detach();
            State {
                api_key_state: ApiKeyState::new(EXPRESS_KEY_URL.into(), (*API_KEY_ENV_VAR).clone()),
                credentials_provider,
                token_provider: None,
                sa_error: None,
            }
        });

        Self { http_client, state }
    }

    fn create_language_model(&self, model: google_ai::Model) -> Arc<dyn LanguageModel> {
        Arc::new(VertexLanguageModel {
            id: LanguageModelId::from(format!("vertex/{}", model.id())),
            model,
            state: self.state.clone(),
            http_client: self.http_client.clone(),
            request_limiter: RateLimiter::new(4),
        })
    }

    fn settings(cx: &App) -> &VertexSettings {
        &crate::AllLanguageModelSettings::get_global(cx).vertex
    }
}

impl LanguageModelProviderState for VertexLanguageModelProvider {
    type ObservableEntity = State;

    fn observable_entity(&self) -> Option<Entity<Self::ObservableEntity>> {
        Some(self.state.clone())
    }
}

impl LanguageModelProvider for VertexLanguageModelProvider {
    fn id(&self) -> LanguageModelProviderId {
        PROVIDER_ID
    }

    fn name(&self) -> LanguageModelProviderName {
        PROVIDER_NAME
    }

    fn icon(&self) -> IconOrSvg {
        IconOrSvg::Icon(IconName::AiGoogle)
    }

    fn default_model(&self, _cx: &App) -> Option<Arc<dyn LanguageModel>> {
        Some(self.create_language_model(google_ai::Model::default()))
    }

    fn default_fast_model(&self, _cx: &App) -> Option<Arc<dyn LanguageModel>> {
        Some(self.create_language_model(google_ai::Model::default_fast()))
    }

    fn provided_models(&self, cx: &App) -> Vec<Arc<dyn LanguageModel>> {
        let mut models = BTreeMap::default();

        for model in google_ai::Model::iter() {
            if !matches!(model, google_ai::Model::Custom { .. }) {
                models.insert(model.id().to_string(), model);
            }
        }

        for model in &VertexLanguageModelProvider::settings(cx).available_models {
            models.insert(
                model.name.clone(),
                google_ai::Model::Custom {
                    name: model.name.clone(),
                    display_name: model.display_name.clone(),
                    max_tokens: model.max_tokens,
                    mode: model.mode.unwrap_or_default(),
                },
            );
        }

        models
            .into_values()
            .map(|model| self.create_language_model(model))
            .collect()
    }

    fn is_authenticated(&self, cx: &App) -> bool {
        self.state.read(cx).is_authenticated()
    }

    fn authenticate(&self, cx: &mut App) -> Task<Result<(), AuthenticateError>> {
        self.state.update(cx, |state, cx| state.authenticate(cx))
    }

    fn configuration_view(
        &self,
        target_agent: ConfigurationViewTargetAgent,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyView {
        cx.new(|cx| ConfigurationView::new(self.state.clone(), target_agent, window, cx))
            .into()
    }

    fn reset_credentials(&self, cx: &mut App) -> Task<Result<()>> {
        self.state.update(cx, |state, cx| state.set_api_key(None, cx))
    }
}

pub struct VertexLanguageModel {
    id: LanguageModelId,
    model: google_ai::Model,
    state: Entity<State>,
    http_client: Arc<dyn HttpClient>,
    request_limiter: RateLimiter,
}

impl VertexLanguageModel {
    fn stream_completion(
        &self,
        request: google_ai::GenerateContentRequest,
        cx: &AsyncApp,
    ) -> BoxFuture<'static, Result<futures::stream::BoxStream<'static, Result<GenerateContentResponse>>>>
    {
        let http_client = self.http_client.clone();

        let resolved = self.state.read_with(cx, |state, cx| {
            let settings = VertexLanguageModelProvider::settings(cx);
            (
                state.resolve_auth(),
                settings.project_id.clone(),
                settings
                    .location
                    .clone()
                    .unwrap_or_else(|| DEFAULT_LOCATION.to_string()),
            )
        });

        async move {
            let (auth, project_id, location) = resolved;
            let auth = auth?;
            if matches!(auth, VertexAuth::ServiceAccount(_)) && project_id.is_none() {
                bail!("Vertex service-account mode requires a `project_id` in settings");
            }
            let project = project_id.unwrap_or_default();
            stream_generate_content(http_client.as_ref(), &auth, &project, &location, request)
                .await
                .context("failed to stream Vertex completion")
        }
        .boxed()
    }
}

impl LanguageModel for VertexLanguageModel {
    fn id(&self) -> LanguageModelId {
        self.id.clone()
    }

    fn name(&self) -> LanguageModelName {
        LanguageModelName::from(self.model.display_name().to_string())
    }

    fn provider_id(&self) -> LanguageModelProviderId {
        PROVIDER_ID
    }

    fn provider_name(&self) -> LanguageModelProviderName {
        PROVIDER_NAME
    }

    fn supports_tools(&self) -> bool {
        self.model.supports_tools()
    }

    fn supports_images(&self) -> bool {
        self.model.supports_images()
    }

    fn supports_thinking(&self) -> bool {
        self.model.supports_thinking()
    }

    fn supports_tool_choice(&self, choice: LanguageModelToolChoice) -> bool {
        match choice {
            LanguageModelToolChoice::Auto
            | LanguageModelToolChoice::Any
            | LanguageModelToolChoice::None => true,
        }
    }

    fn tool_input_format(&self) -> LanguageModelToolSchemaFormat {
        LanguageModelToolSchemaFormat::JsonSchemaSubset
    }

    fn telemetry_id(&self) -> String {
        format!("vertex/{}", self.model.request_id())
    }

    fn max_token_count(&self) -> u64 {
        self.model.max_token_count()
    }

    fn max_output_tokens(&self) -> Option<u64> {
        self.model.max_output_tokens()
    }

    fn stream_completion(
        &self,
        request: LanguageModelRequest,
        cx: &AsyncApp,
    ) -> BoxFuture<
        'static,
        Result<
            futures::stream::BoxStream<
                'static,
                Result<LanguageModelCompletionEvent, LanguageModelCompletionError>,
            >,
            LanguageModelCompletionError,
        >,
    > {
        let request = into_google(
            request,
            self.model.request_id().to_string(),
            self.model.mode(),
        );
        let request = self.stream_completion(request, cx);
        let future = self.request_limiter.stream(async move {
            let response = request.await.map_err(LanguageModelCompletionError::from)?;
            Ok(GoogleEventMapper::new().map_stream(response))
        });
        async move { Ok(future.await?.boxed()) }.boxed()
    }
}

struct ConfigurationView {
    api_key_editor: Entity<InputField>,
    state: Entity<State>,
    target_agent: ConfigurationViewTargetAgent,
    load_credentials_task: Option<Task<()>>,
}

impl ConfigurationView {
    fn new(
        state: Entity<State>,
        target_agent: ConfigurationViewTargetAgent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&state, |_, _, cx| cx.notify()).detach();

        let load_credentials_task = Some(cx.spawn_in(window, {
            let state = state.clone();
            async move |this, cx| {
                if let Some(task) = Some(state.update(cx, |state, cx| state.authenticate(cx))) {
                    let _ = task.await;
                }
                this.update(cx, |this, cx| {
                    this.load_credentials_task = None;
                    cx.notify();
                })
                .log_err();
            }
        }));

        Self {
            api_key_editor: cx.new(|cx| InputField::new(window, cx, "Vertex Express API key")),
            target_agent,
            state,
            load_credentials_task,
        }
    }

    fn save_api_key(&mut self, _: &menu::Confirm, window: &mut Window, cx: &mut Context<Self>) {
        let api_key = self.api_key_editor.read(cx).text(cx).trim().to_string();
        if api_key.is_empty() {
            return;
        }
        self.api_key_editor
            .update(cx, |editor, cx| editor.set_text("", window, cx));
        let state = self.state.clone();
        cx.spawn_in(window, async move |_, cx| {
            state
                .update(cx, |state, cx| state.set_api_key(Some(api_key), cx))
                .await
        })
        .detach_and_log_err(cx);
    }

    fn reset_api_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.api_key_editor
            .update(cx, |editor, cx| editor.set_text("", window, cx));
        let state = self.state.clone();
        cx.spawn_in(window, async move |_, cx| {
            state
                .update(cx, |state, cx| state.set_api_key(None, cx))
                .await
        })
        .detach_and_log_err(cx);
    }
}

impl Render for ConfigurationView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let has_service_account = state.token_provider.is_some();
        let sa_error = state.sa_error.clone();

        if self.load_credentials_task.is_some() {
            return div()
                .child(Label::new("Loading credentials…"))
                .into_any_element();
        }

        if has_service_account {
            return ConfiguredApiCard::new("Service account configured")
                .into_any_element();
        }

        let agent_label = match &self.target_agent {
            ConfigurationViewTargetAgent::ZedAgent => {
                "PaddleBoard's agent with Vertex AI (Gemini Enterprise)".into()
            }
            ConfigurationViewTargetAgent::Other(agent) => agent.clone(),
        };

        if self.state.read(cx).api_key_state.has_key() {
            ConfiguredApiCard::new("Express API key configured")
                .on_click(cx.listener(|this, _, window, cx| this.reset_api_key(window, cx)))
                .into_any_element()
        } else {
            v_flex()
                .size_full()
                .on_action(cx.listener(Self::save_api_key))
                .child(Label::new(format!(
                    "To use {agent_label}, configure Vertex AI (Gemini Enterprise):"
                )))
                .child(
                    List::new()
                        .child(ListBulletItem::new(
                            "Full Vertex: set `language_models.vertex.project_id`, \
                             `location`, and `credentials_path` (a service-account JSON key) \
                             in settings.",
                        ))
                        .child(ListBulletItem::new(
                            "Quick start (Express): paste a Vertex API key below and press enter.",
                        )),
                )
                .child(self.api_key_editor.clone())
                .when_some(sa_error, |this, error| {
                    this.child(
                        Label::new(format!("Service-account error: {error}"))
                            .size(LabelSize::Small)
                            .color(Color::Error),
                    )
                })
                .child(
                    Label::new(format!(
                        "You can also set the {VERTEX_API_KEY_VAR} environment variable and restart."
                    ))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
                .into_any_element()
        }
    }
}
