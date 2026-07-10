//! GitHub / GitLab OAuth Device Flow (RFC 8628) — "OAuth lite".
//!
//! The flow a CLI like `gh` uses: request a short user code, send the user to
//! the browser to approve it, poll for the access token. Needs only a public
//! OAuth client id (no shipped secret), which is why it's the one OAuth
//! variant an open-source desktop app can ship safely.
//!
//! Providers are described by [`OAuthProvider`] ([`GITHUB`], [`GITLAB`]); the
//! transport is provider-agnostic. This module is transport + parsing only; the
//! UI (`git_ui`'s login modal) drives the poll loop on its own executor and
//! stores the resulting token via [`crate::save`]. Only gitlab.com and
//! github.com ship a client id — self-managed hosts and Bitbucket stay PAT-only.

use anyhow::{Context as _, Result, anyhow, bail};
use futures::AsyncReadExt as _;
use http_client::{AsyncBody, HttpClient, Request};
use serde::Deserialize;
use std::sync::Arc;

pub const GITHUB_DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
pub const GITHUB_ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
/// Scope granting git read/write on the user's repositories.
pub const GITHUB_OAUTH_SCOPES: &str = "repo";

pub const GITLAB_DEVICE_CODE_URL: &str = "https://gitlab.com/oauth/authorize_device";
pub const GITLAB_ACCESS_TOKEN_URL: &str = "https://gitlab.com/oauth/token";
/// Scopes granting git read/write over HTTPS on the user's GitLab repositories.
pub const GITLAB_OAUTH_SCOPES: &str = "read_repository write_repository";

/// PaddleBoard's registered GitHub OAuth App client id. Device flow uses no
/// client secret, so this public id is safe to ship in source — it's what lets
/// the OAuth sign-in light up for everyone building from a release, not just
/// developers who export the env var below.
const DEFAULT_GITHUB_OAUTH_CLIENT_ID: &str = "Ov23lirhSNtJAomztkd7";

/// The OAuth client id for PaddleBoard's GitHub OAuth app. Device flow uses no
/// client secret, so a public id is safe to bake into builds. Resolution:
/// runtime env var first (so it's testable without a rebuild), then a value
/// baked at compile time by the packaging scripts, then the shipped default
/// above. Returns `None` only if every source resolves empty (e.g. the default
/// is blanked out), which hides the OAuth sign-in UI — PATs always work regardless.
pub fn github_oauth_client_id() -> Option<String> {
    std::env::var("PADDLEBOARD_GITHUB_OAUTH_CLIENT_ID")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            option_env!("PADDLEBOARD_GITHUB_OAUTH_CLIENT_ID")
                .map(str::to_string)
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            Some(DEFAULT_GITHUB_OAUTH_CLIENT_ID)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
}

/// PaddleBoard's registered GitLab (gitlab.com) OAuth application client id.
/// Empty until the app is registered — register a **user-owned, non-confidential**
/// application under gitlab.com → User Settings → Applications with the
/// `read_repository`/`write_repository` scopes and the Device Authorization
/// grant, then paste its Application ID here. Blank hides the GitLab OAuth
/// sign-in (PATs still work). Device flow ships no secret, so a public id is
/// safe to bake in. Only covers gitlab.com; self-managed instances are PAT-based.
const DEFAULT_GITLAB_OAUTH_CLIENT_ID: &str =
    "a4ebbbff80444c8e001e214fed65cea933a859898499f487e875a9942f111d03";

/// The OAuth client id for PaddleBoard's GitLab app. Same resolution order as
/// [`github_oauth_client_id`]: runtime env var, then a compile-time baked value,
/// then the shipped default. `None` (the default until registration) hides the
/// GitLab OAuth sign-in.
pub fn gitlab_oauth_client_id() -> Option<String> {
    std::env::var("PADDLEBOARD_GITLAB_OAUTH_CLIENT_ID")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            option_env!("PADDLEBOARD_GITLAB_OAUTH_CLIENT_ID")
                .map(str::to_string)
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            Some(DEFAULT_GITLAB_OAUTH_CLIENT_ID)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
}

/// A device-flow OAuth provider PaddleBoard can sign into. Only the two SaaS
/// hosts with a shipped client id; self-managed hosts stay PAT-based.
#[derive(Clone, Copy)]
pub struct OAuthProvider {
    /// Human name for status messages and the button, e.g. "GitLab".
    pub display_name: &'static str,
    pub device_code_url: &'static str,
    pub access_token_url: &'static str,
    pub scopes: &'static str,
    /// Credential key the token is saved under (mirrors `KnownProvider::url`).
    pub save_host: &'static str,
    /// Username git sends with the token (mirrors `KnownProvider::token_username`).
    pub save_username: &'static str,
    /// Resolves the public client id, or `None` when unset (button hidden).
    pub client_id: fn() -> Option<String>,
}

pub const GITHUB: OAuthProvider = OAuthProvider {
    display_name: "GitHub",
    device_code_url: GITHUB_DEVICE_CODE_URL,
    access_token_url: GITHUB_ACCESS_TOKEN_URL,
    scopes: GITHUB_OAUTH_SCOPES,
    save_host: "https://github.com",
    save_username: "x-access-token",
    client_id: github_oauth_client_id,
};

pub const GITLAB: OAuthProvider = OAuthProvider {
    display_name: "GitLab",
    device_code_url: GITLAB_DEVICE_CODE_URL,
    access_token_url: GITLAB_ACCESS_TOKEN_URL,
    scopes: GITLAB_OAUTH_SCOPES,
    save_host: "https://gitlab.com",
    save_username: "oauth2",
    client_id: gitlab_oauth_client_id,
};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DeviceAuthorization {
    pub device_code: String,
    /// The short code the user types/confirms in the browser, e.g. `WDJB-MJHT`.
    pub user_code: String,
    /// Where the user goes to enter it, e.g. `https://github.com/login/device`.
    pub verification_uri: String,
    /// The same page with the user code already filled in (RFC 8628 optional).
    /// GitLab returns it; GitHub does not. Preferred for opening the browser so
    /// the user doesn't have to type the code.
    #[serde(default)]
    pub verification_uri_complete: Option<String>,
    /// Seconds between polls; GitHub sends 5.
    #[serde(default = "default_poll_interval")]
    pub interval: u64,
    /// Seconds until the device code expires.
    pub expires_in: u64,
}

fn default_poll_interval() -> u64 {
    5
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollOutcome {
    AccessToken(String),
    /// User hasn't approved yet — keep polling.
    Pending,
    /// Polling too fast — add 5 seconds to the interval.
    SlowDown,
    /// User rejected the request.
    Denied,
    /// The device code expired before approval.
    Expired,
}

fn parse_device_authorization(body: &str) -> Result<DeviceAuthorization> {
    serde_json::from_str(body)
        .with_context(|| format!("unexpected device authorization response: {body}"))
}

#[derive(Deserialize)]
struct PollResponse {
    access_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

fn parse_poll_response(body: &str) -> Result<PollOutcome> {
    let response: PollResponse = serde_json::from_str(body)
        .with_context(|| format!("unexpected access token response: {body}"))?;
    if let Some(token) = response.access_token {
        if token.is_empty() {
            return Err(anyhow!("the OAuth server returned an empty access token"));
        }
        return Ok(PollOutcome::AccessToken(token));
    }
    match response.error.as_deref() {
        Some("authorization_pending") => Ok(PollOutcome::Pending),
        Some("slow_down") => Ok(PollOutcome::SlowDown),
        Some("access_denied") => Ok(PollOutcome::Denied),
        Some("expired_token") => Ok(PollOutcome::Expired),
        other => Err(anyhow!(
            "device flow error: {} ({})",
            other.unwrap_or("unknown"),
            response.error_description.unwrap_or_default()
        )),
    }
}

async fn post_form(url: &str, form_body: String, http: &Arc<dyn HttpClient>) -> Result<String> {
    let request = Request::post(url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(AsyncBody::from(form_body))?;
    let mut response = http
        .send(request)
        .await
        .with_context(|| format!("error reaching {url}"))?;
    let mut body = Vec::new();
    response.body_mut().read_to_end(&mut body).await?;
    let body = String::from_utf8(body).context("non-UTF-8 response body")?;
    if !response.status().is_success() {
        bail!("{url} returned {}: {body}", response.status().as_u16());
    }
    Ok(body)
}

/// Step 1: ask the provider for a device + user code pair.
pub async fn request_device_authorization(
    provider: &OAuthProvider,
    client_id: &str,
    http: &Arc<dyn HttpClient>,
) -> Result<DeviceAuthorization> {
    let body = format!(
        "client_id={}&scope={}",
        url_encode(client_id),
        url_encode(provider.scopes)
    );
    let response = post_form(provider.device_code_url, body, http).await?;
    parse_device_authorization(&response)
}

/// Step 2 (repeated): ask whether the user has approved yet. The caller owns
/// the sleep between calls, honoring [`DeviceAuthorization::interval`] and
/// [`PollOutcome::SlowDown`].
pub async fn poll_device_authorization_once(
    provider: &OAuthProvider,
    client_id: &str,
    device_code: &str,
    http: &Arc<dyn HttpClient>,
) -> Result<PollOutcome> {
    let body = format!(
        "client_id={}&device_code={}&grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code",
        url_encode(client_id),
        url_encode(device_code)
    );
    let response = post_form(provider.access_token_url, body, http).await?;
    parse_poll_response(&response)
}

/// Percent-encode a form value. Client ids and device codes are alphanumeric
/// in practice; this keeps us correct if that ever changes.
fn url_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_device_authorization_response() {
        let body = r#"{
            "device_code": "3584d83530557fdd1f46af8289938c8ef79f9dc5",
            "user_code": "WDJB-MJHT",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900,
            "interval": 5
        }"#;
        let auth = parse_device_authorization(body).unwrap();
        assert_eq!(auth.user_code, "WDJB-MJHT");
        assert_eq!(auth.verification_uri, "https://github.com/login/device");
        assert_eq!(auth.interval, 5);
        assert_eq!(auth.expires_in, 900);
        // GitHub omits the pre-filled URL.
        assert_eq!(auth.verification_uri_complete, None);
    }

    #[test]
    fn parses_gitlab_style_verification_uri_complete() {
        let body = r#"{
            "device_code": "e6ce253f021c0b89833866f9ac9aba721bd3dd1e",
            "user_code": "XDU8VCHN",
            "verification_uri": "https://gitlab.com/oauth/device",
            "verification_uri_complete": "https://gitlab.com/oauth/device?user_code=XDU8VCHN",
            "expires_in": 300,
            "interval": 5
        }"#;
        let auth = parse_device_authorization(body).unwrap();
        assert_eq!(
            auth.verification_uri_complete.as_deref(),
            Some("https://gitlab.com/oauth/device?user_code=XDU8VCHN")
        );
    }

    #[test]
    fn parses_poll_outcomes() {
        assert_eq!(
            parse_poll_response(r#"{"error":"authorization_pending"}"#).unwrap(),
            PollOutcome::Pending
        );
        assert_eq!(
            parse_poll_response(r#"{"error":"slow_down","interval":10}"#).unwrap(),
            PollOutcome::SlowDown
        );
        assert_eq!(
            parse_poll_response(r#"{"error":"access_denied"}"#).unwrap(),
            PollOutcome::Denied
        );
        assert_eq!(
            parse_poll_response(r#"{"error":"expired_token"}"#).unwrap(),
            PollOutcome::Expired
        );
        assert_eq!(
            parse_poll_response(
                r#"{"access_token":"gho_16C7e42F292c6912E7710c838347Ae178B4a","token_type":"bearer","scope":"repo"}"#
            )
            .unwrap(),
            PollOutcome::AccessToken("gho_16C7e42F292c6912E7710c838347Ae178B4a".to_string())
        );
        assert!(parse_poll_response(r#"{"error":"incorrect_client_credentials"}"#).is_err());
    }

    #[test]
    fn url_encoding_passes_safe_chars_and_escapes_others() {
        assert_eq!(url_encode("Iv1.abc-DEF_123"), "Iv1.abc-DEF_123");
        assert_eq!(url_encode("a b&c"), "a%20b%26c");
    }

    #[test]
    fn providers_target_their_hosts() {
        assert_eq!(GITHUB.device_code_url, GITHUB_DEVICE_CODE_URL);
        assert_eq!(GITHUB.save_username, "x-access-token");
        assert_eq!(GITLAB.device_code_url, "https://gitlab.com/oauth/authorize_device");
        assert_eq!(GITLAB.access_token_url, "https://gitlab.com/oauth/token");
        assert_eq!(GITLAB.save_host, "https://gitlab.com");
        assert_eq!(GITLAB.save_username, "oauth2");
    }
}
