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
use paddleboard_vertex::{
    GcloudTokenProvider, ServiceAccountKey, TokenProvider, VertexAuth, stream_generate_content,
};
use fs::Fs;
use settings::{Settings, SettingsStore, update_settings_file};
pub use settings::VertexAvailableModel as AvailableModel;
use std::sync::{Arc, LazyLock};
use ui::prelude::*;
use ui_input::InputField;
use util::ResultExt;

const PROVIDER_ID: LanguageModelProviderId = LanguageModelProviderId::new("vertex");
const PROVIDER_NAME: LanguageModelProviderName =
    LanguageModelProviderName::new("Gemini Enterprise (Vertex AI)");

/// Marker URL the express API key is associated with in the keychain.
const EXPRESS_KEY_URL: &str = "https://aiplatform.googleapis.com";
/// Default to the `global` location: the newest models (Gemini 3, the `-latest`
/// aliases) are only published there, and the 2.5 family is available there too.
const DEFAULT_LOCATION: &str = "global";
const VERTEX_API_KEY_VAR: &str = "VERTEX_API_KEY";

/// Curated default model list for Vertex — these are confirmed available on the
/// `global` location. (Vertex publishes specific model versions per project/region,
/// so the consumer-Gemini ids aren't reused; users add region-specific ids via the
/// `available_models` setting.) Gemini context windows are ~1M tokens.
const VERTEX_MODELS: &[(&str, &str)] = &[
    ("gemini-2.5-pro", "Gemini 2.5 Pro"),
    ("gemini-3-flash-preview", "Gemini 3 Flash (Preview)"),
    ("gemini-2.5-flash", "Gemini 2.5 Flash"),
    ("gemini-flash-latest", "Gemini Flash (Latest)"),
    ("gemini-flash-lite-latest", "Gemini Flash Lite (Latest)"),
];
const VERTEX_MODEL_MAX_TOKENS: u64 = 1_048_576;

fn vertex_model(name: &str, display_name: &str) -> google_ai::Model {
    google_ai::Model::Custom {
        name: name.to_string(),
        display_name: Some(display_name.to_string()),
        max_tokens: VERTEX_MODEL_MAX_TOKENS,
        mode: Default::default(),
    }
}

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
    /// Borrows short-lived tokens from the `gcloud` CLI — the no-stored-key path.
    gcloud_provider: Arc<GcloudTokenProvider>,
    sa_error: Option<SharedString>,
}

impl State {
    /// Whether a stored credential (service-account key or Express API key) is
    /// configured. gcloud auth is handled separately since it stores nothing.
    fn has_stored_credential(&self) -> bool {
        self.token_provider.is_some() || self.api_key_state.has_key()
    }

    /// Resolves the auth method for a request. Precedence: a loaded service-account
    /// key, then an Express API key, then gcloud (Application Default Credentials) —
    /// the default no-stored-key path, which needs only `project_id`/`location` plus
    /// a `gcloud auth login`.
    fn resolve_auth(&self) -> Result<VertexAuth> {
        if let Some((_, provider)) = &self.token_provider {
            Ok(VertexAuth::ServiceAccount(provider.clone()))
        } else if let Some(key) = self.api_key_state.key(EXPRESS_KEY_URL) {
            Ok(VertexAuth::ApiKey(key.to_string()))
        } else {
            Ok(VertexAuth::Gcloud(self.gcloud_provider.clone()))
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
                .read_with(cx, |this, _| this.has_stored_credential())
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
                gcloud_provider: Arc::new(GcloudTokenProvider::new()),
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
        Some(self.create_language_model(vertex_model("gemini-2.5-pro", "Gemini 2.5 Pro")))
    }

    fn default_fast_model(&self, _cx: &App) -> Option<Arc<dyn LanguageModel>> {
        Some(self.create_language_model(vertex_model(
            "gemini-3-flash-preview",
            "Gemini 3 Flash (Preview)",
        )))
    }

    fn provided_models(&self, cx: &App) -> Vec<Arc<dyn LanguageModel>> {
        let mut models = BTreeMap::default();

        for (name, display_name) in VERTEX_MODELS {
            models.insert(name.to_string(), vertex_model(name, display_name));
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
        // A stored credential (service-account key or Express API key), or the
        // gcloud path — which needs no stored key, just a `project_id` to target.
        self.state.read(cx).has_stored_credential() || Self::settings(cx).project_id.is_some()
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
            if matches!(
                auth,
                VertexAuth::ServiceAccount(_) | VertexAuth::Gcloud(_)
            ) && project_id.is_none()
            {
                bail!(
                    "Vertex needs a `project_id` (and `location`) in settings for service-account or gcloud auth"
                );
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
    project_id_editor: Entity<InputField>,
    location_editor: Entity<InputField>,
    credentials_editor: Entity<InputField>,
    api_key_editor: Entity<InputField>,
    state: Entity<State>,
    load_credentials_task: Option<Task<()>>,
}

impl ConfigurationView {
    fn new(
        state: Entity<State>,
        _target_agent: ConfigurationViewTargetAgent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&state, |_, _, cx| cx.notify()).detach();

        let settings = VertexLanguageModelProvider::settings(cx).clone();

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

        let project_id_editor =
            cx.new(|cx| InputField::new(window, cx, "your-gcp-project").label("GCP Project ID"));
        let location_editor = cx.new(|cx| {
            InputField::new(window, cx, "global").label("Location (default: global)")
        });
        let credentials_editor = cx.new(|cx| {
            InputField::new(window, cx, "/path/to/service-account.json")
                .label("Service-account key file (optional)")
        });
        let api_key_editor = cx
            .new(|cx| InputField::new(window, cx, "Gemini Enterprise Express API key").label("Express API key (optional)"));

        if let Some(project_id) = &settings.project_id {
            project_id_editor.update(cx, |field, cx| field.set_text(project_id, window, cx));
        }
        if let Some(location) = &settings.location {
            location_editor.update(cx, |field, cx| field.set_text(location, window, cx));
        }
        if let Some(path) = &settings.credentials_path {
            credentials_editor.update(cx, |field, cx| field.set_text(path, window, cx));
        }

        Self {
            project_id_editor,
            location_editor,
            credentials_editor,
            api_key_editor,
            state,
            load_credentials_task,
        }
    }

    fn save(&mut self, _: &menu::Confirm, window: &mut Window, cx: &mut Context<Self>) {
        let project_id = self.project_id_editor.read(cx).text(cx).trim().to_string();
        let location = self.location_editor.read(cx).text(cx).trim().to_string();
        let credentials = self.credentials_editor.read(cx).text(cx).trim().to_string();
        let api_key = self.api_key_editor.read(cx).text(cx).trim().to_string();

        let fs = <dyn Fs>::global(cx);
        update_settings_file(fs, cx, move |settings, _| {
            let vertex = settings
                .language_models
                .get_or_insert_default()
                .vertex
                .get_or_insert_default();
            vertex.project_id = (!project_id.is_empty()).then_some(project_id);
            vertex.location = (!location.is_empty()).then_some(location);
            vertex.credentials_path = (!credentials.is_empty()).then_some(credentials);
        });

        // The Express API key is a secret — store it in the keychain, not settings.
        if !api_key.is_empty() {
            self.api_key_editor
                .update(cx, |field, cx| field.set_text("", window, cx));
            let state = self.state.clone();
            cx.spawn_in(window, async move |_, cx| {
                state
                    .update(cx, |state, cx| state.set_api_key(Some(api_key), cx))
                    .await
            })
            .detach_and_log_err(cx);
        }
    }
}

impl Render for ConfigurationView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.load_credentials_task.is_some() {
            return div()
                .child(Label::new("Loading credentials…"))
                .into_any_element();
        }

        let state = self.state.read(cx);
        let has_express_key = state.api_key_state.has_key();
        let has_sa_key = state.token_provider.is_some();
        let sa_error = state.sa_error.clone();
        let project_id_set = VertexLanguageModelProvider::settings(cx).project_id.is_some();

        // Mirror resolve_auth's precedence so the user sees the mode that will be used.
        let status: SharedString = if has_sa_key {
            "Authenticating via service-account key.".into()
        } else if has_express_key {
            "Authenticating via Express API key.".into()
        } else if project_id_set {
            "Authenticating via gcloud — run `gcloud auth login` if you haven't.".into()
        } else {
            "Not configured — set a Project ID (gcloud), a key file, or an Express key below.".into()
        };

        v_flex()
            .size_full()
            .gap_1()
            .on_action(cx.listener(Self::save))
            .child(
                Label::new("Run Gemini through your GCP project. The recommended setup stores no key: \
                            run `gcloud auth login`, set a Project ID, and save.")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(self.project_id_editor.clone())
            .child(self.location_editor.clone())
            .child(self.credentials_editor.clone())
            .child(self.api_key_editor.clone())
            .child(
                Button::new("save-vertex", "Save")
                    .style(ButtonStyle::Filled)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.save(&menu::Confirm, window, cx)
                    })),
            )
            .child(
                Label::new(status)
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .when_some(sa_error, |this, error| {
                this.child(
                    Label::new(format!("Service-account error: {error}"))
                        .size(LabelSize::Small)
                        .color(Color::Error),
                )
            })
            .into_any_element()
    }
}
