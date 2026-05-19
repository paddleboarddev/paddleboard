//! Contains helper functions for constructing URLs to various Zed-related pages.
//!
//! These URLs will adapt to the configured server URL in order to construct
//! links appropriate for the environment (e.g., by linking to a local copy of
//! zed.dev in development).

use gpui::App;
use settings::Settings;

use crate::ClientSettings;

fn server_url(cx: &App) -> &str {
    &ClientSettings::get_global(cx).server_url
}

/// Returns the URL to the account page on zed.dev.
pub fn account_url(cx: &App) -> String {
    format!("{server_url}/account", server_url = server_url(cx))
}

/// PaddleBoard: this used to return `zed.dev/account/start-trial` so the
/// "Start 14-day Free Pro Trial" buttons across the app would funnel users
/// at Zed's hosted plans. PaddleBoard isn't a hosted service and has no
/// trial concept, so the URL is replaced with a no-op that opens nothing
/// useful (`about:blank`). Upstream call sites keep compiling and the
/// buttons keep rendering wherever the upstream code chose to show them
/// — we just stop sending users away. Removing the buttons themselves
/// would require touching ~5 upstream-shaped files; this single-file fix
/// keeps the merge surface flat.
pub fn start_trial_url(_cx: &App) -> String {
    "about:blank".to_string()
}

/// PaddleBoard: see `start_trial_url` — same defang for the
/// "Upgrade to Pro" call-to-action URL. Was
/// `zed.dev/account/upgrade`; now `about:blank`.
pub fn upgrade_to_zed_pro_url(_cx: &App) -> String {
    "about:blank".to_string()
}

/// Returns the URL to Zed's terms of service.
pub fn terms_of_service(cx: &App) -> String {
    format!("{server_url}/terms-of-service", server_url = server_url(cx))
}

/// Returns the URL to PaddleBoard AI's privacy and security docs.
pub fn ai_privacy_and_security(cx: &App) -> String {
    format!(
        "{server_url}/docs/ai/privacy-and-security",
        server_url = server_url(cx)
    )
}

/// Returns the URL to Zed's edit prediction documentation.
pub fn edit_prediction_docs(cx: &App) -> String {
    format!(
        "{server_url}/docs/ai/edit-prediction",
        server_url = server_url(cx)
    )
}

/// Returns the URL to Zed's ACP registry blog post.
pub fn acp_registry_blog(cx: &App) -> String {
    format!(
        "{server_url}/blog/acp-registry",
        server_url = server_url(cx)
    )
}

/// Returns the URL to Zed's Parallel Agents blog post.
pub fn parallel_agents_blog(cx: &App) -> String {
    format!("{server_url}/blog", server_url = server_url(cx))
}

pub fn shared_agent_thread_url(session_id: &str) -> String {
    format!("paddleboard://agent/shared/{}", session_id)
}
