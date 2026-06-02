//! Google Vertex AI transport + service-account OAuth for PaddleBoard.
//!
//! Vertex's Gemini API uses the same request/response schema as the consumer
//! Gemini API, so this crate reuses `google_ai`'s body types and only provides
//! the Vertex-specific URL/auth and a service-account token minter.

use std::mem;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, anyhow, bail};
use futures::{AsyncBufReadExt, AsyncReadExt, StreamExt, io::BufReader, stream::BoxStream};
use google_ai::{
    GenerateContentRequest, GenerateContentResponse, validate_generate_content_request,
};
use http_client::{AsyncBody, HttpClient, Method, Request as HttpRequest};
use serde::{Deserialize, Serialize};

const OAUTH_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const VERTEX_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
/// Refresh a cached token this many seconds before its real expiry.
const TOKEN_EXPIRY_SKEW_SECS: u64 = 60;

/// A parsed GCP service-account key (the JSON Google issues for a service
/// account). Only the fields needed to mint an access token are kept.
#[derive(Clone, Deserialize)]
pub struct ServiceAccountKey {
    pub client_email: String,
    pub private_key: String,
    #[serde(default)]
    pub private_key_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default = "default_token_uri")]
    pub token_uri: String,
}

fn default_token_uri() -> String {
    OAUTH_TOKEN_URL.to_string()
}

impl ServiceAccountKey {
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).context("parsing service-account JSON")
    }
}

/// How a Vertex request is authenticated.
#[derive(Clone)]
pub enum VertexAuth {
    /// Full Vertex via a service-account key — mints an OAuth2 bearer token.
    ServiceAccount(std::sync::Arc<TokenProvider>),
    /// Full Vertex via short-lived tokens borrowed from the `gcloud` CLI
    /// (Application Default Credentials). Nothing is stored on disk.
    Gcloud(std::sync::Arc<GcloudTokenProvider>),
    /// Vertex Express mode via an API key (`?key=` on the global endpoint).
    ApiKey(String),
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Serialize)]
struct JwtClaims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: u64,
    exp: u64,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

struct CachedToken {
    token: String,
    expires_at: u64,
}

/// Mints and caches a short-lived OAuth2 access token from a service-account key
/// using the JWT-bearer grant.
pub struct TokenProvider {
    key: ServiceAccountKey,
    cached: Mutex<Option<CachedToken>>,
}

impl TokenProvider {
    pub fn new(key: ServiceAccountKey) -> Self {
        Self {
            key,
            cached: Mutex::new(None),
        }
    }

    /// Returns a valid access token, reusing the cached one when it is still
    /// fresh and otherwise minting + exchanging a new JWT.
    pub async fn token(&self, client: &dyn HttpClient) -> Result<String> {
        let now = now_unix();
        if let Some(token) = self
            .cached
            .lock()
            .ok()
            .and_then(|guard| {
                guard
                    .as_ref()
                    .filter(|cached| token_is_fresh(cached.expires_at, now))
                    .map(|cached| cached.token.clone())
            })
        {
            return Ok(token);
        }

        let assertion = self.sign_jwt(now)?;
        let body = format!(
            "grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer&assertion={assertion}"
        );
        let request = HttpRequest::builder()
            .method(Method::POST)
            .uri(&self.key.token_uri)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(AsyncBody::from(body))?;

        let mut response = client.send(request).await?;
        let mut text = String::new();
        response.body_mut().read_to_string(&mut text).await?;
        if !response.status().is_success() {
            bail!(
                "Vertex token exchange failed (status {:?}): {text}",
                response.status()
            );
        }

        let parsed: TokenResponse =
            serde_json::from_str(&text).context("parsing OAuth token response")?;
        let expires_at = now_unix() + parsed.expires_in;
        if let Ok(mut guard) = self.cached.lock() {
            *guard = Some(CachedToken {
                token: parsed.access_token.clone(),
                expires_at,
            });
        }
        Ok(parsed.access_token)
    }

    fn sign_jwt(&self, now: u64) -> Result<String> {
        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
        header.kid = self.key.private_key_id.clone();
        let claims = JwtClaims {
            iss: &self.key.client_email,
            scope: VERTEX_SCOPE,
            aud: &self.key.token_uri,
            iat: now,
            exp: now + 3600,
        };
        let encoding_key = jsonwebtoken::EncodingKey::from_rsa_pem(self.key.private_key.as_bytes())
            .context("loading service-account private key (expected RSA PEM)")?;
        jsonwebtoken::encode(&header, &claims, &encoding_key).context("signing service-account JWT")
    }
}

fn token_is_fresh(expires_at: u64, now: u64) -> bool {
    expires_at > now.saturating_add(TOKEN_EXPIRY_SKEW_SECS)
}

/// `gcloud auth print-access-token` does not report the token lifetime; Google
/// access tokens last ~1h, so cache conservatively.
const GCLOUD_TOKEN_TTL_SECS: u64 = 3000;

/// Borrows short-lived access tokens from the `gcloud` CLI (Application Default
/// Credentials). No credential is stored on disk by PaddleBoard — `gcloud`
/// manages the user's login and refresh.
#[derive(Default)]
pub struct GcloudTokenProvider {
    cached: Mutex<Option<CachedToken>>,
}

impl GcloudTokenProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn token(&self) -> Result<String> {
        let now = now_unix();
        if let Some(token) = self.cached.lock().ok().and_then(|guard| {
            guard
                .as_ref()
                .filter(|cached| token_is_fresh(cached.expires_at, now))
                .map(|cached| cached.token.clone())
        }) {
            return Ok(token);
        }

        let output = smol::process::Command::new("gcloud")
            .args(["auth", "print-access-token"])
            .output()
            .await
            .context(
                "running `gcloud auth print-access-token` — is the gcloud CLI installed and on PATH?",
            )?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "`gcloud auth print-access-token` failed — run `gcloud auth login`. Details: {}",
                stderr.trim()
            );
        }
        let token = String::from_utf8(output.stdout)
            .context("gcloud token was not valid UTF-8")?
            .trim()
            .to_string();
        if token.is_empty() {
            bail!("gcloud returned an empty access token; run `gcloud auth login`");
        }

        if let Ok(mut guard) = self.cached.lock() {
            *guard = Some(CachedToken {
                token: token.clone(),
                expires_at: now_unix() + GCLOUD_TOKEN_TTL_SECS,
            });
        }
        Ok(token)
    }
}

/// PaddleBoard security: `project`/`location` come from user settings and are
/// interpolated into the request host/path that the Google bearer token is sent to.
/// Restrict them to the GCP segment charset (`[a-z0-9-]`) so a tampered setting like
/// `evil.example.com.` cannot reshape the authority and exfiltrate the token (SSRF).
fn is_valid_gcp_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Builds the Vertex `streamGenerateContent` URL for the given auth mode.
/// Service-account and gcloud use the regional endpoint with the project/location
/// path (Bearer auth); Express mode uses the global endpoint with a `?key=`.
pub fn vertex_stream_url(
    auth: &VertexAuth,
    project: &str,
    location: &str,
    model_id: &str,
) -> String {
    match auth {
        VertexAuth::ServiceAccount(_) | VertexAuth::Gcloud(_) => {
            // The `global` location uses the un-prefixed host; regions are prefixed.
            // (Newer models — Gemini 3, the `-latest` aliases — are global-only.)
            let host = if location == "global" {
                "aiplatform.googleapis.com".to_string()
            } else {
                format!("{location}-aiplatform.googleapis.com")
            };
            format!(
                "https://{host}/v1/projects/{project}/locations/{location}/publishers/google/models/{model_id}:streamGenerateContent?alt=sse"
            )
        }
        VertexAuth::ApiKey(key) => format!(
            "https://aiplatform.googleapis.com/v1/publishers/google/models/{model_id}:streamGenerateContent?alt=sse&key={key}"
        ),
    }
}

/// Streams a Gemini completion from Vertex AI, reusing `google_ai`'s request and
/// response types. The model id is moved into the URL path (Vertex carries the
/// model there, not in the body).
pub async fn stream_generate_content(
    client: &dyn HttpClient,
    auth: &VertexAuth,
    project: &str,
    location: &str,
    mut request: GenerateContentRequest,
) -> Result<BoxStream<'static, Result<GenerateContentResponse>>> {
    validate_generate_content_request(&request)?;
    // Reject project/location values that could reshape the request authority before
    // the bearer token is attached (see is_valid_gcp_segment). Express (ApiKey) mode
    // uses the fixed global host, so only the Bearer modes need the location check.
    anyhow::ensure!(
        is_valid_gcp_segment(project),
        "invalid Vertex project id: {project:?}"
    );
    if !matches!(auth, VertexAuth::ApiKey(_)) {
        anyhow::ensure!(
            is_valid_gcp_segment(location),
            "invalid Vertex location: {location:?}"
        );
    }
    let model_id = mem::take(&mut request.model.model_id);
    let uri = vertex_stream_url(auth, project, location, &model_id);

    let mut builder = HttpRequest::builder()
        .method(Method::POST)
        .uri(uri)
        .header("Content-Type", "application/json");
    let bearer = match auth {
        VertexAuth::ServiceAccount(provider) => Some(provider.token(client).await?),
        VertexAuth::Gcloud(provider) => Some(provider.token().await?),
        VertexAuth::ApiKey(_) => None,
    };
    if let Some(token) = bearer {
        builder = builder.header("Authorization", format!("Bearer {token}"));
    }

    let request = builder.body(AsyncBody::from(serde_json::to_string(&request)?))?;
    let mut response = client.send(request).await?;
    if response.status().is_success() {
        let reader = BufReader::new(response.into_body());
        Ok(reader
            .lines()
            .filter_map(|line| async move {
                match line {
                    Ok(line) => line.strip_prefix("data: ").map(|line| {
                        serde_json::from_str(line)
                            .map_err(|error| anyhow!("Error parsing JSON: {error:?}\n{line:?}"))
                    }),
                    Err(error) => Some(Err(anyhow!(error))),
                }
            })
            .boxed())
    } else {
        let status = response.status();
        let mut text = String::new();
        response.body_mut().read_to_string(&mut text).await?;
        // A 404 on Vertex almost always means the model id isn't published in this
        // project/location (availability varies), not a real "not found" — point the
        // user at the fix rather than dumping the raw body.
        if status.as_u16() == 404 {
            bail!(
                "Vertex model `{model_id}` isn't available in location `{location}` for project `{project}`. \
                 Try `location: \"global\"` (newer models live there), or add an available model under \
                 `language_models.vertex.available_models`. (Vertex response: {text})"
            );
        }
        bail!("Vertex streamGenerateContent failed (status {status:?}): {text}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_service_account_json() {
        let json = r#"{
            "type": "service_account",
            "project_id": "my-proj",
            "private_key_id": "abc123",
            "private_key": "-----BEGIN PRIVATE KEY-----\nMII...\n-----END PRIVATE KEY-----\n",
            "client_email": "svc@my-proj.iam.gserviceaccount.com",
            "token_uri": "https://oauth2.googleapis.com/token"
        }"#;
        let key = ServiceAccountKey::from_json(json).expect("parses");
        assert_eq!(key.client_email, "svc@my-proj.iam.gserviceaccount.com");
        assert_eq!(key.project_id.as_deref(), Some("my-proj"));
        assert_eq!(key.private_key_id.as_deref(), Some("abc123"));
        assert_eq!(key.token_uri, "https://oauth2.googleapis.com/token");
    }

    #[test]
    fn token_uri_defaults_when_absent() {
        let json = r#"{
            "private_key": "-----BEGIN PRIVATE KEY-----\nx\n-----END PRIVATE KEY-----\n",
            "client_email": "svc@x.iam.gserviceaccount.com"
        }"#;
        let key = ServiceAccountKey::from_json(json).expect("parses");
        assert_eq!(key.token_uri, OAUTH_TOKEN_URL);
    }

    #[test]
    fn service_account_url_is_regional_with_project() {
        let auth = VertexAuth::ServiceAccount(std::sync::Arc::new(TokenProvider::new(
            ServiceAccountKey {
                client_email: "x".into(),
                private_key: "x".into(),
                private_key_id: None,
                project_id: None,
                token_uri: OAUTH_TOKEN_URL.into(),
            },
        )));
        let url = vertex_stream_url(&auth, "proj", "us-central1", "gemini-2.0-flash");
        assert_eq!(
            url,
            "https://us-central1-aiplatform.googleapis.com/v1/projects/proj/locations/us-central1/publishers/google/models/gemini-2.0-flash:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn gcloud_url_is_regional_with_project() {
        let auth = VertexAuth::Gcloud(std::sync::Arc::new(GcloudTokenProvider::new()));
        let url = vertex_stream_url(&auth, "proj", "us-east1", "gemini-2.0-flash");
        assert_eq!(
            url,
            "https://us-east1-aiplatform.googleapis.com/v1/projects/proj/locations/us-east1/publishers/google/models/gemini-2.0-flash:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn global_location_uses_unprefixed_host() {
        let auth = VertexAuth::Gcloud(std::sync::Arc::new(GcloudTokenProvider::new()));
        let url = vertex_stream_url(&auth, "proj", "global", "gemini-3-flash-preview");
        assert_eq!(
            url,
            "https://aiplatform.googleapis.com/v1/projects/proj/locations/global/publishers/google/models/gemini-3-flash-preview:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn express_url_is_global_with_key() {
        let auth = VertexAuth::ApiKey("SECRET".into());
        let url = vertex_stream_url(&auth, "ignored", "ignored", "gemini-2.0-flash");
        assert_eq!(
            url,
            "https://aiplatform.googleapis.com/v1/publishers/google/models/gemini-2.0-flash:streamGenerateContent?alt=sse&key=SECRET"
        );
    }

    #[test]
    fn token_freshness_respects_skew() {
        // expires far in the future -> fresh
        assert!(token_is_fresh(1_000, 100));
        // expires within the skew window -> stale
        assert!(!token_is_fresh(120, 100));
        // already expired -> stale
        assert!(!token_is_fresh(50, 100));
    }
}
