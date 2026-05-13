use agent_client_protocol as acp;
use agent_settings::AgentSettings;
use anyhow::Result;
use browser::{ForwardedPort, ForwardedPorts};
use gpui::{App, AppContext as _, Entity, SharedString, Task};
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

            let (working_dir, authorize) = cx.update(|cx| {
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
                Ok::<_, String>((working_dir, authorize))
            })?;

            if let Some(authorize) = authorize {
                authorize.await.map_err(|e| e.to_string())?;
            }

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

            let container_id = cx
                .background_spawn({
                    let image = image.clone();
                    let host_wd = host_wd.clone();
                    let port_spec = port_spec.clone();
                    let user_command = input.command.clone();
                    async move {
                        let mut cmd = new_command("podman");
                        cmd.args([
                            "run",
                            "-d",
                            "--rm",
                            "--runtime=runsc",
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
                container_id: Arc::from(container_id.as_str()),
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
    use super::parse_host_port;

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
