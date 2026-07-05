use agent_client_protocol::schema::v1 as acp;
use agent_settings::AgentSettings;
use anyhow::Result;
use browser::{ForwardedPort, ForwardedPorts};
use gpui::{App, AppContext as _, Entity, SharedString, Task};
use paddleboard_sandbox_prereqs_state::SandboxPrereqs;
use paddleboard_sandbox_settings::{
    BuiltInCapability, SandboxGateDecision, SandboxSettings, decide_gate,
};
use project::Project;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings::Settings;
use std::{sync::Arc, time::Duration};
use util::ResultExt as _;
use util::command::new_command;

use crate::{
    AgentTool, ToolCallEventStream, ToolInput, ToolPermissionDecision,
    decide_permission_from_settings,
    tools::sandbox_tool::{resolve_worktree_dir, shell_single_quote},
};

const READY_POLL_INTERVAL: Duration = Duration::from_millis(500);
const PORT_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Starts a long-running service inside an isolated Podman (gVisor runsc) container and
/// publishes one TCP port from the container to a host port chosen by Podman. The host port
/// is registered in the workspace's Forwarded Ports panel and surfaced as a one-click link
/// in the embedded browser.
///
/// Use this when the agent needs to run a development server, demo app, agent UI (such as
/// `adk web`), or any other long-lived process the user is meant to interact with through
/// the browser. For one-shot commands (builds, tests, scripts) use `sandbox_tool` instead.
///
/// The container runs detached and survives across tool calls. The user can stop it from
/// the browser panel's Forwarded Ports row.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SandboxServiceToolInput {
    /// The command to run inside the container. Should start a long-lived process that
    /// listens on `port` (e.g. `python -m http.server 8000`, `adk web --port 8000`).
    pub command: String,
    /// Working directory for the command. Must be one of the project's root directories.
    pub cd: String,
    /// The TCP port the service will listen on inside the container.
    pub port: u16,
    /// Optional container image. Defaults to `ubuntu:latest`.
    pub image: Option<String>,
    /// Optional short label shown in the browser's Forwarded Ports row. Defaults to the
    /// first whitespace-delimited word of `command`.
    pub label: Option<String>,
    /// Optional substring to wait for in the container's stdout/stderr before returning.
    /// If absent, the tool returns as soon as Podman reports a host port mapping.
    pub ready_log_substring: Option<String>,
    /// Optional maximum time to wait for the service to become ready (in milliseconds).
    /// Defaults to 30000.
    pub startup_timeout_ms: Option<u64>,
    /// Optional list of host environment variable *names* to forward into the container,
    /// e.g. `["GOOGLE_API_KEY"]` for an `adk web` service. Values are read from PaddleBoard's
    /// own process environment at run time and passed via `podman run -e` — they are never
    /// exposed to the model. Names missing from the host env are skipped with a log warning.
    pub forward_env: Option<Vec<String>>,
}

pub struct SandboxServiceTool {
    pub project: Entity<Project>,
}

impl SandboxServiceTool {
    pub fn new(project: Entity<Project>) -> Self {
        Self { project }
    }
}

impl AgentTool for SandboxServiceTool {
    type Input = SandboxServiceToolInput;
    type Output = String;

    const NAME: &'static str = "sandbox_service_tool";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Execute
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        match input {
            Ok(input) => format!("Sandbox service: {}", input.command).into(),
            Err(_) => "Sandbox Service".into(),
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        cx.spawn(async move |cx| {
            let input = input
                .recv()
                .await
                .map_err(|e| format!("Failed to receive tool input: {e}"))?;

            let (working_dir, authorize, gate) = cx.update(|cx| {
                let working_dir = resolve_worktree_dir(&input.cd, &self.project, cx)
                    .map_err(|err| err.to_string())?;

                let decision = decide_permission_from_settings(
                    Self::NAME,
                    std::slice::from_ref(&input.command),
                    AgentSettings::get_global(cx),
                );

                let authorize = match decision {
                    ToolPermissionDecision::Allow => None,
                    ToolPermissionDecision::Deny(reason) => return Err(reason),
                    ToolPermissionDecision::Confirm => {
                        let context = crate::ToolPermissionContext::new(
                            Self::NAME,
                            vec![input.command.clone()],
                        );
                        Some(event_stream.authorize(
                            self.initial_title(Ok(input.clone()), cx),
                            context,
                            cx,
                        ))
                    }
                };

                let gate = decide_gate(
                    SandboxPrereqs::status(cx),
                    // Phase 1: long-lived services stay Podman-only (the
                    // built-in microVM tier does not do port publishing yet).
                    BuiltInCapability::Unsupported,
                    SandboxSettings::get_global(cx),
                );

                Ok::<_, String>((working_dir, authorize, gate))
            })?;

            if let Some(authorize) = authorize {
                authorize.await.map_err(|e| e.to_string())?;
            }

            let run_on_host = match &gate {
                SandboxGateDecision::Block { reason } => {
                    return Err(format!(
                        "Sandbox prerequisites missing: {reason}. \
                         Open Sandbox Prerequisites from the status bar to install Podman / gVisor, \
                         or set `paddleboard_sandbox.on_missing_runtime` to \"fall_back_to_host\" \
                         to run on the host without a container."
                    ));
                }
                SandboxGateDecision::WarnOnce { reason } => {
                    if paddleboard_sandbox_settings::claim_warn_once_slot() {
                        log::warn!(
                            "PaddleBoard sandbox: {reason}. Starting service sandboxed; \
                             open Sandbox Prerequisites to install."
                        );
                    }
                    false
                }
                SandboxGateDecision::FallBackToHost { reason } => {
                    log::warn!(
                        "PaddleBoard sandbox: {reason}. Starting service on the host \
                         (no container) per `paddleboard_sandbox.on_missing_runtime`."
                    );
                    true
                }
                // Unreachable with BuiltInCapability::Unsupported above; if it
                // ever leaks through, refuse rather than run a service in a
                // tier that cannot publish its port.
                SandboxGateDecision::UseBuiltIn { reason, .. } => {
                    return Err(format!(
                        "Sandbox prerequisites missing: {reason}. The service tool cannot \
                         run on the built-in microVM tier yet; install Podman + gVisor."
                    ));
                }
                SandboxGateDecision::Allow => false,
            };

            let image = input
                .image
                .clone()
                .unwrap_or_else(|| "ubuntu:latest".to_string());
            let label = input
                .label
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| {
                    input
                        .command
                        .split_whitespace()
                        .next()
                        .unwrap_or("service")
                        .to_string()
                });
            let container_port = input.port;
            let port_spec = format!("127.0.0.1::{container_port}");

            const CONTAINER_WORKDIR: &str = "/workspace";
            let host_wd = working_dir.to_string_lossy().to_string();

            let env_args = resolve_forward_env(input.forward_env.as_deref());

            // Host fallback bypasses podman entirely: we spawn the service in
            // the user's host shell. Port mapping is lost — the service binds
            // directly to its declared port on localhost, so `host_port ==
            // container_port`. We still register a Forwarded Ports entry so
            // the user gets a clickable link.
            if run_on_host {
                let host_command_id = format!("host-{}", std::process::id());
                let host_wd_for_spawn = working_dir.clone();
                let user_command = input.command.clone();
                let env_args_host = env_args.clone();
                cx.background_spawn(async move {
                    let mut cmd = new_command("bash");
                    cmd.current_dir(&host_wd_for_spawn);
                    for env_arg in &env_args_host {
                        if let Some((name, value)) = env_arg.split_once('=') {
                            cmd.env(name, value);
                        }
                    }
                    cmd.args(["-c", &user_command]);
                    if let Err(error) = cmd.spawn() {
                        log::warn!(
                            "sandbox_service_tool: host fallback spawn failed: {error}"
                        );
                    }
                })
                .detach();

                let port = ForwardedPort {
                    label: SharedString::from(label.clone()),
                    host_port: container_port,
                    container_id: Some(Arc::from(host_command_id.as_str())),
                    path: None,
                };
                cx.update(|cx| ForwardedPorts::register(cx, port));

                let quoted = shell_single_quote(&input.command);
                return Ok(format!(
                    "Started service `{quoted}` on the host (sandbox prerequisites missing; \
                     running without a container per `paddleboard_sandbox.on_missing_runtime`).\n\
                     Listening on port {container_port}.\n\
                     Available at http://localhost:{container_port} (registered in the browser panel as `{label}`)."
                ));
            }

            let container_id = cx
                .background_spawn({
                    let image = image.clone();
                    let host_wd = host_wd.clone();
                    let port_spec = port_spec.clone();
                    let user_command = input.command.clone();
                    let env_args = env_args.clone();
                    async move {
                        let mut cmd = new_command("podman");
                        cmd.args(["run", "-d", "--rm", "--runtime=runsc"]);
                        for env_arg in &env_args {
                            cmd.args(["-e", env_arg]);
                        }
                        cmd.args([
                            "-p",
                            &port_spec,
                            "-v",
                            &format!("{host_wd}:{CONTAINER_WORKDIR}"),
                            "-w",
                            CONTAINER_WORKDIR,
                            &image,
                            "bash",
                            "-c",
                            &user_command,
                        ]);
                        let output = cmd.output().await?;
                        if !output.status.success() {
                            anyhow::bail!(
                                "podman run failed: {}",
                                String::from_utf8_lossy(&output.stderr)
                            );
                        }
                        anyhow::Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
                    }
                })
                .await
                .map_err(|e| e.to_string())?;

            let timeout = Duration::from_millis(input.startup_timeout_ms.unwrap_or(30_000));
            let host_port = match poll_for_host_port(cx, &container_id, container_port, timeout)
                .await
            {
                Some(p) => p,
                None => {
                    stop_container(cx, &container_id);
                    return Err(format!(
                        "Service did not bind a host port for container port {container_port} within {timeout:?}. Container stopped."
                    ));
                }
            };

            if let Some(ready) = input.ready_log_substring.as_deref()
                && !wait_for_log_substring(cx, &container_id, ready, timeout).await
            {
                stop_container(cx, &container_id);
                return Err(format!(
                    "Service did not log {:?} within {:?}. Container stopped.",
                    ready, timeout
                ));
            }

            let port = ForwardedPort {
                label: SharedString::from(label.clone()),
                host_port,
                container_id: Some(Arc::from(container_id.as_str())),
                path: None,
            };

            cx.update(|cx| ForwardedPorts::register(cx, port));

            // The single-quoted echo of the original command makes it harder for an agent to
            // accidentally inject shell metacharacters into its own status message — the value
            // is what was actually passed through `bash -c`.
            let quoted = shell_single_quote(&input.command);
            Ok(format!(
                "Started service `{quoted}` in container `{container_id}` (image `{image}`).\n\
                 Container port {container_port} → host port {host_port}.\n\
                 Available at http://localhost:{host_port} (registered in the browser panel as `{label}`)."
            ))
        })
    }
}

async fn poll_for_host_port(
    cx: &mut gpui::AsyncApp,
    container_id: &str,
    container_port: u16,
    timeout: Duration,
) -> Option<u16> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let id = container_id.to_string();
        let result = cx
            .background_spawn(async move {
                let mut cmd = new_command("podman");
                cmd.args(["port", &id, &format!("{container_port}/tcp")]);
                let output = cmd.output().await?;
                anyhow::Ok(String::from_utf8_lossy(&output.stdout).to_string())
            })
            .await
            .log_err();

        if let Some(stdout) = result
            && let Some(port) = parse_host_port(&stdout)
        {
            return Some(port);
        }

        cx.background_executor().timer(PORT_POLL_INTERVAL).await;
    }
    None
}

async fn wait_for_log_substring(
    cx: &mut gpui::AsyncApp,
    container_id: &str,
    needle: &str,
    timeout: Duration,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let id = container_id.to_string();
        let logs = cx
            .background_spawn(async move {
                let mut cmd = new_command("podman");
                cmd.args(["logs", &id]);
                let output = cmd.output().await?;
                let mut combined = String::from_utf8_lossy(&output.stdout).to_string();
                combined.push_str(&String::from_utf8_lossy(&output.stderr));
                anyhow::Ok(combined)
            })
            .await
            .log_err();

        if let Some(logs) = logs
            && logs.contains(needle)
        {
            return true;
        }

        cx.background_executor().timer(READY_POLL_INTERVAL).await;
    }
    false
}

fn stop_container(cx: &mut gpui::AsyncApp, container_id: &str) {
    let id = container_id.to_string();
    cx.background_spawn(async move {
        let mut cmd = new_command("podman");
        cmd.args(["stop", &id]);
        if let Err(error) = cmd.output().await {
            log::warn!("podman stop {id} failed: {error}");
        }
    })
    .detach();
}

/// Resolve `forward_env` names against the host process environment, producing one
/// `NAME=value` string per successfully-resolved variable. Missing names are skipped with a
/// log warning — we don't fail the run, because the service may still work without an
/// optional credential. We also reject names with `=` in them so a malicious agent can't
/// smuggle extra env vars through a single entry.
fn resolve_forward_env(names: Option<&[String]>) -> Vec<String> {
    let Some(names) = names else { return Vec::new() };
    let mut args = Vec::with_capacity(names.len());
    for name in names {
        if name.is_empty() || name.contains('=') {
            log::warn!("sandbox_service_tool: ignoring invalid forward_env name {name:?}");
            continue;
        }
        match std::env::var(name) {
            Ok(value) => args.push(format!("{name}={value}")),
            Err(_) => {
                log::warn!(
                    "sandbox_service_tool: forward_env name {name:?} not set in host \
                     environment; skipping"
                );
            }
        }
    }
    args
}

/// Parse `podman port` output. Each line looks like `0.0.0.0:54321` or `127.0.0.1:54321`,
/// possibly prefixed by `proto/host_port -> ` depending on the podman version. We take the
/// last `:`-delimited token and parse it as a u16.
fn parse_host_port(output: &str) -> Option<u16> {
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let token = line.rsplit(':').next()?.trim();
        if let Ok(port) = token.parse::<u16>() {
            return Some(port);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{parse_host_port, resolve_forward_env};

    #[test]
    fn resolve_forward_env_returns_empty_when_none() {
        assert!(resolve_forward_env(None).is_empty());
        assert!(resolve_forward_env(Some(&[])).is_empty());
    }

    #[test]
    fn resolve_forward_env_skips_missing_and_invalid_names() {
        // Use a name that's overwhelmingly unlikely to be set.
        let names = vec![
            "PB_SANDBOX_TEST_DEFINITELY_UNSET_VAR_42".to_string(),
            "".to_string(),
            "INVALID=NAME".to_string(),
        ];
        assert!(resolve_forward_env(Some(&names)).is_empty());
    }

    #[test]
    fn resolve_forward_env_picks_up_set_vars() {
        // Set in this process — won't bleed across test binaries.
        // SAFETY: setting an env var local to this test; no other thread reads it.
        unsafe {
            std::env::set_var("PB_SANDBOX_TEST_FORWARD_ENV", "hello world");
        }
        let names = vec!["PB_SANDBOX_TEST_FORWARD_ENV".to_string()];
        let resolved = resolve_forward_env(Some(&names));
        assert_eq!(resolved, vec!["PB_SANDBOX_TEST_FORWARD_ENV=hello world"]);
        // SAFETY: cleaning up the test-local env var.
        unsafe {
            std::env::remove_var("PB_SANDBOX_TEST_FORWARD_ENV");
        }
    }

    #[test]
    fn parses_ipv4_host_port() {
        assert_eq!(parse_host_port("0.0.0.0:54321\n"), Some(54321));
        assert_eq!(parse_host_port("127.0.0.1:8080\n"), Some(8080));
    }

    #[test]
    fn parses_when_arrow_prefix_present() {
        // Some podman versions emit `8000/tcp -> 0.0.0.0:54321`
        assert_eq!(parse_host_port("8000/tcp -> 0.0.0.0:54321"), Some(54321));
    }

    #[test]
    fn returns_first_parseable_line() {
        let s = "junk\n0.0.0.0:7000\n0.0.0.0:9000\n";
        assert_eq!(parse_host_port(s), Some(7000));
    }

    #[test]
    fn returns_none_on_empty_or_garbage() {
        assert_eq!(parse_host_port(""), None);
        assert_eq!(parse_host_port("not a port"), None);
    }
}
