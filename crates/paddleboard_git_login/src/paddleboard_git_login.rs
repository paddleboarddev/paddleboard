//! PaddleBoard: "Git Login" — save a Personal Access Token per git hosting
//! provider (GitHub, GitLab, BitBucket, or a custom host) so git HTTPS
//! operations authenticate without prompting every time.
//!
//! Tokens are stored in the OS keychain via the shared
//! [`credentials_provider::CredentialsProvider`] abstraction — never in
//! settings or plaintext. This crate is the storage/model layer; the
//! management UI and the askpass injection live in `git_ui` and the app.

use anyhow::{Context as _, Result, anyhow};
use credentials_provider::CredentialsProvider;
use gpui::AsyncApp;

/// A git hosting provider PaddleBoard knows how to help you log in to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownProvider {
    /// Display name, e.g. "GitHub".
    pub name: &'static str,
    /// Bare host, e.g. "github.com".
    pub host: &'static str,
    /// Credential key / base URL, e.g. "https://github.com".
    pub url: &'static str,
    /// The username git should send when authenticating with a token over
    /// HTTPS, when the user hasn't specified one (provider convention).
    pub token_username: &'static str,
    /// Where the user creates a token/app-password.
    pub token_url: &'static str,
    /// Scopes to recommend when creating the token.
    pub scopes_hint: &'static str,
    /// Environment variable checked as a fallback token source.
    pub env_var: &'static str,
}

/// The providers surfaced in the Git Login UI. Custom hosts are still
/// supported (any host can be saved), these just get prefilled help.
pub const KNOWN_PROVIDERS: &[KnownProvider] = &[
    KnownProvider {
        name: "GitHub",
        host: "github.com",
        url: "https://github.com",
        // GitHub accepts any username with a valid PAT as the password;
        // `x-access-token` is the conventional value for token auth.
        token_username: "x-access-token",
        token_url: "https://github.com/settings/tokens/new",
        scopes_hint: "repo",
        env_var: "GITHUB_TOKEN",
    },
    KnownProvider {
        name: "GitLab",
        host: "gitlab.com",
        url: "https://gitlab.com",
        // GitLab: username `oauth2` with a PAT as the password.
        token_username: "oauth2",
        token_url: "https://gitlab.com/-/user_settings/personal_access_tokens",
        scopes_hint: "read_repository, write_repository",
        env_var: "GITLAB_TOKEN",
    },
    KnownProvider {
        name: "Bitbucket",
        host: "bitbucket.org",
        url: "https://bitbucket.org",
        // Bitbucket: username `x-token-auth` with a repository access token.
        token_username: "x-token-auth",
        token_url: "https://bitbucket.org/account/settings/app-passwords/",
        scopes_hint: "repository, repository:write",
        env_var: "BITBUCKET_TOKEN",
    },
];

/// A resolved git login: the username + token to feed to git over HTTPS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitLogin {
    pub username: String,
    pub token: String,
    /// True when the token came from an environment variable rather than the keychain.
    pub from_env: bool,
}

/// Normalize a host or git URL to the credential key `https://<host>`.
///
/// Handles `https://user@github.com/owner/repo`, bare `github.com`, and
/// `gitlab.example.com:8080/owner` (port is dropped for keying).
pub fn credential_key(host_or_url: &str) -> String {
    let trimmed = host_or_url.trim();
    let without_scheme = trimmed
        .split_once("://")
        .map_or(trimmed, |(_scheme, rest)| rest);
    let without_user = without_scheme
        .rsplit_once('@')
        .map_or(without_scheme, |(_user, rest)| rest);
    let host = without_user
        .split(['/', ':'])
        .next()
        .unwrap_or(without_user);
    format!("https://{host}")
}

/// Find the known provider matching a host or URL, if any.
pub fn known_provider(host_or_url: &str) -> Option<&'static KnownProvider> {
    let key = credential_key(host_or_url);
    KNOWN_PROVIDERS.iter().find(|provider| provider.url == key)
}

/// Resolve a token from the environment for a known provider, using the given
/// environment lookup. Factored out so it can be tested without real env vars.
fn resolve_env_token(
    host_or_url: &str,
    get_env: impl Fn(&str) -> Option<String>,
) -> Option<(&'static str, String)> {
    let provider = known_provider(host_or_url)?;
    let token = get_env(provider.env_var)?;
    if token.is_empty() {
        return None;
    }
    Some((provider.token_username, token))
}

/// Load a saved login for the given host or git URL.
///
/// A non-empty environment variable (e.g. `GITHUB_TOKEN`) takes precedence over
/// the keychain, matching the behavior of LLM API keys. Returns `None` when no
/// token is found.
pub async fn load(
    host_or_url: &str,
    provider: &dyn CredentialsProvider,
    cx: &AsyncApp,
) -> Result<Option<GitLogin>> {
    if let Some((username, token)) = resolve_env_token(host_or_url, |name| std::env::var(name).ok())
    {
        return Ok(Some(GitLogin {
            username: username.to_string(),
            token,
            from_env: true,
        }));
    }

    let key = credential_key(host_or_url);
    match provider.read_credentials(&key, cx).await? {
        Some((username, token_bytes)) => {
            let token = String::from_utf8(token_bytes)
                .with_context(|| format!("stored git token for {key} is not valid UTF-8"))?;
            Ok(Some(GitLogin {
                username,
                token,
                from_env: false,
            }))
        }
        None => Ok(None),
    }
}

/// Save a login (username + token) for the given host or git URL. The token is
/// written to the OS keychain.
pub async fn save(
    host_or_url: &str,
    username: &str,
    token: &str,
    provider: &dyn CredentialsProvider,
    cx: &AsyncApp,
) -> Result<()> {
    if token.is_empty() {
        return Err(anyhow!("cannot save an empty token"));
    }
    let key = credential_key(host_or_url);
    provider
        .write_credentials(&key, username, token.as_bytes(), cx)
        .await
}

/// What git is asking for in an askpass prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    Username,
    Password,
}

/// Parse a git HTTPS askpass prompt into the target URL and what git wants.
///
/// git emits prompts like `Username for 'https://github.com': ` and
/// `Password for 'https://x-access-token@github.com': `. Returns `None` for
/// anything we should NOT auto-answer (SSH key passphrases, host-key
/// confirmations, non-HTTP URLs) so those still fall through to the modal.
pub fn parse_git_prompt(prompt: &str) -> Option<(String, PromptKind)> {
    let kind = if prompt.starts_with("Username for ") {
        PromptKind::Username
    } else if prompt.starts_with("Password for ") {
        PromptKind::Password
    } else {
        return None;
    };
    let start = prompt.find('\'')? + 1;
    let rest = prompt.get(start..)?;
    let end = rest.find('\'')?;
    let url = &rest[..end];
    // PaddleBoard security: only auto-answer HTTPS prompts. Answering an `http://`
    // prompt would hand the saved token to git for transmission in cleartext (and a
    // malicious/redirected remote could coax it onto http). Let cleartext and non-HTTP
    // prompts fall through to the interactive modal instead.
    if !url.starts_with("https://") {
        return None;
    }
    Some((url.to_string(), kind))
}

/// Delete the saved login for the given host or git URL.
pub async fn delete(
    host_or_url: &str,
    provider: &dyn CredentialsProvider,
    cx: &AsyncApp,
) -> Result<()> {
    let key = credential_key(host_or_url);
    provider.delete_credentials(&key, cx).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Mutex;

    #[test]
    fn credential_key_normalizes_hosts_and_urls() {
        assert_eq!(credential_key("github.com"), "https://github.com");
        assert_eq!(credential_key("https://github.com"), "https://github.com");
        assert_eq!(
            credential_key("https://x-access-token@github.com/owner/repo.git"),
            "https://github.com"
        );
        assert_eq!(
            credential_key("https://gitlab.example.com:8080/group/proj"),
            "https://gitlab.example.com"
        );
        assert_eq!(credential_key("  bitbucket.org  "), "https://bitbucket.org");
    }

    #[test]
    fn known_provider_lookup() {
        assert_eq!(known_provider("github.com").map(|p| p.name), Some("GitHub"));
        assert_eq!(
            known_provider("https://oauth2@gitlab.com/x/y").map(|p| p.name),
            Some("GitLab")
        );
        assert_eq!(known_provider("example.invalid"), None);
    }

    #[test]
    fn env_token_resolution_uses_provider_convention() {
        let env = |name: &str| match name {
            "GITHUB_TOKEN" => Some("ghp_abc".to_string()),
            "GITLAB_TOKEN" => Some(String::new()), // empty is ignored
            _ => None,
        };
        assert_eq!(
            resolve_env_token("github.com", env),
            Some(("x-access-token", "ghp_abc".to_string()))
        );
        // Empty env var is treated as unset.
        assert_eq!(resolve_env_token("gitlab.com", env), None);
        // Unknown host has no env fallback.
        assert_eq!(resolve_env_token("example.invalid", env), None);
    }

    /// In-memory credentials provider for tests (never touches the real keychain
    /// or the dev credentials file).
    struct FakeProvider(Mutex<HashMap<String, (String, Vec<u8>)>>);

    impl FakeProvider {
        fn new() -> Self {
            Self(Mutex::new(HashMap::new()))
        }
    }

    impl CredentialsProvider for FakeProvider {
        fn read_credentials<'a>(
            &'a self,
            url: &'a str,
            _cx: &'a AsyncApp,
        ) -> Pin<Box<dyn Future<Output = Result<Option<(String, Vec<u8>)>>> + 'a>> {
            Box::pin(async move { Ok(self.0.lock().unwrap().get(url).cloned()) })
        }

        fn write_credentials<'a>(
            &'a self,
            url: &'a str,
            username: &'a str,
            password: &'a [u8],
            _cx: &'a AsyncApp,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
            Box::pin(async move {
                self.0
                    .lock()
                    .unwrap()
                    .insert(url.to_string(), (username.to_string(), password.to_vec()));
                Ok(())
            })
        }

        fn delete_credentials<'a>(
            &'a self,
            url: &'a str,
            _cx: &'a AsyncApp,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
            Box::pin(async move {
                self.0.lock().unwrap().remove(url);
                Ok(())
            })
        }
    }

    #[test]
    fn parse_git_prompt_matches_https_only() {
        assert_eq!(
            parse_git_prompt("Username for 'https://github.com': "),
            Some(("https://github.com".to_string(), PromptKind::Username))
        );
        assert_eq!(
            parse_git_prompt("Password for 'https://x-access-token@github.com': "),
            Some((
                "https://x-access-token@github.com".to_string(),
                PromptKind::Password
            ))
        );
        // SSH key passphrase — must NOT be auto-answered.
        assert_eq!(
            parse_git_prompt("Enter passphrase for key '/home/u/.ssh/id_ed25519': "),
            None
        );
        // Non-HTTP URL — leave to the modal.
        assert_eq!(parse_git_prompt("Password for 'ssh://git@host': "), None);
        // Cleartext http:// — must NOT be auto-answered (would leak the token in cleartext).
        assert_eq!(parse_git_prompt("Password for 'http://github.com': "), None);
        assert_eq!(parse_git_prompt("Username for 'http://github.com': "), None);
        // Host-key confirmation — leave to the modal.
        assert_eq!(
            parse_git_prompt("The authenticity of host 'github.com' can't be established."),
            None
        );
    }

    #[gpui::test]
    async fn save_load_delete_roundtrip(cx: &mut gpui::TestAppContext) {
        let provider = FakeProvider::new();
        let async_cx = cx.to_async();

        // Nothing stored yet (and no env var for a custom host).
        assert_eq!(
            load("https://example.invalid", &provider, &async_cx)
                .await
                .unwrap(),
            None
        );

        save(
            "https://github.com/owner/repo.git",
            "octocat",
            "ghp_secret",
            &provider,
            &async_cx,
        )
        .await
        .unwrap();

        // Stored under the normalized host key, retrievable by any URL on that host.
        let login = load("github.com", &provider, &async_cx)
            .await
            .unwrap()
            .expect("login should be present");
        assert_eq!(login.username, "octocat");
        assert_eq!(login.token, "ghp_secret");
        assert!(!login.from_env);

        delete("https://github.com", &provider, &async_cx)
            .await
            .unwrap();
        assert_eq!(load("github.com", &provider, &async_cx).await.unwrap(), None);
    }

    #[gpui::test]
    async fn empty_token_is_rejected(cx: &mut gpui::TestAppContext) {
        let provider = FakeProvider::new();
        let async_cx = cx.to_async();
        assert!(save("github.com", "octocat", "", &provider, &async_cx)
            .await
            .is_err());
    }
}
