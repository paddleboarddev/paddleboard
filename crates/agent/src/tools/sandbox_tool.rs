use agent_client_protocol as acp;
use agent_settings::AgentSettings;
use anyhow::Result;
use futures::FutureExt as _;
use gpui::{App, Entity, SharedString, Task};
use project::Project;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings::Settings;
use std::{
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use crate::{
    AgentTool, ThreadEnvironment, ToolCallEventStream, ToolInput, ToolPermissionDecision,
    decide_permission_from_settings,
};

const COMMAND_OUTPUT_LIMIT: u64 = 16 * 1024;

/// Executes a shell command securely within an isolated Podman (gVisor runsc) container.
///
/// Use this tool whenever you generate unverified code, run tests, or execute build scripts
/// that could potentially harm the host system. The command runs inside an ephemeral Ubuntu container
/// but maps the project directory so file changes are preserved.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SandboxToolInput {
    /// The command to execute securely.
    pub command: String,
    /// Working directory for the command. This must be one of the root directories of the project.
    pub cd: String,
    /// Optional container image (defaults to 'ubuntu:latest').
    pub image: Option<String>,
    /// Optional maximum runtime (in milliseconds). If exceeded, the running terminal task is killed.
    pub timeout_ms: Option<u64>,
}

pub struct SandboxTool {
    pub project: Entity<Project>,
    pub environment: Rc<dyn ThreadEnvironment>,
}

impl SandboxTool {
    pub fn new(project: Entity<Project>, environment: Rc<dyn ThreadEnvironment>) -> Self {
        Self {
            project,
            environment,
        }
    }
}

impl AgentTool for SandboxTool {
    type Input = SandboxToolInput;
    type Output = String;

    const NAME: &'static str = "sandbox_tool";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Execute
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        if let Ok(input) = input {
            format!("Sandbox: {}", input.command).into()
        } else {
            "Sandbox Execution".into()
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
                let working_dir =
                    working_dir(&input, &self.project, cx).map_err(|err| err.to_string())?;

                let decision = decide_permission_from_settings(
                    Self::NAME,
                    std::slice::from_ref(&input.command),
                    AgentSettings::get_global(cx),
                );

                let authorize = match decision {
                    ToolPermissionDecision::Allow => None,
                    ToolPermissionDecision::Deny(reason) => {
                        return Err(reason);
                    }
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
                Ok((working_dir, authorize))
            })?;
            if let Some(authorize) = authorize {
                authorize.await.map_err(|e| e.to_string())?;
            }

            let image = input
                .image
                .clone()
                .unwrap_or_else(|| "ubuntu:latest".to_string());
            let wd_str = working_dir.to_string_lossy().to_string();

            // Construct the secure podman gvisor command
            let podman_command = format!(
                "podman run --rm --runtime=runsc -v '{}:{}' -w '{}' {} bash -c '{}'",
                wd_str,
                wd_str,
                wd_str,
                image,
                input.command.replace("'", "'\\''")
            );

            let terminal = self
                .environment
                .create_terminal(
                    podman_command.clone(),
                    Some(working_dir),
                    Some(COMMAND_OUTPUT_LIMIT),
                    cx,
                )
                .await
                .map_err(|e| e.to_string())?;

            let terminal_id = terminal.id(cx).map_err(|e| e.to_string())?;
            event_stream.update_fields(acp::ToolCallUpdateFields::new().content(vec![
                acp::ToolCallContent::Terminal(acp::Terminal::new(terminal_id)),
            ]));

            let timeout = input.timeout_ms.map(Duration::from_millis);
            let mut timed_out = false;
            let mut user_stopped_via_signal = false;
            let wait_for_exit = terminal.wait_for_exit(cx).map_err(|e| e.to_string())?;

            match timeout {
                Some(timeout) => {
                    let timeout_task = cx.background_executor().timer(timeout);

                    futures::select! {
                        _ = wait_for_exit.clone().fuse() => {},
                        _ = timeout_task.fuse() => {
                            timed_out = true;
                            terminal.kill(cx).map_err(|e| e.to_string())?;
                            wait_for_exit.await;
                        }
                        _ = event_stream.cancelled_by_user().fuse() => {
                            user_stopped_via_signal = true;
                            terminal.kill(cx).map_err(|e| e.to_string())?;
                            wait_for_exit.await;
                        }
                    }
                }
                None => {
                    futures::select! {
                        _ = wait_for_exit.clone().fuse() => {},
                        _ = event_stream.cancelled_by_user().fuse() => {
                            user_stopped_via_signal = true;
                            terminal.kill(cx).map_err(|e| e.to_string())?;
                            wait_for_exit.await;
                        }
                    }
                }
            };

            let user_stopped_via_signal =
                user_stopped_via_signal || event_stream.was_cancelled_by_user();
            let user_stopped_via_terminal = terminal.was_stopped_by_user(cx).unwrap_or(false);
            let user_stopped = user_stopped_via_signal || user_stopped_via_terminal;

            let output = terminal.current_output(cx).map_err(|e| e.to_string())?;

            Ok(process_content(
                output,
                &input.command,
                timed_out,
                user_stopped,
            ))
        })
    }
}

fn process_content(
    output: acp::TerminalOutputResponse,
    command: &str,
    timed_out: bool,
    user_stopped: bool,
) -> String {
    let content = output.output.trim();
    let is_empty = content.is_empty();

    let content = format!("```\n{content}\n```");
    let content = if output.truncated {
        format!(
            "Command output too long. The first {} bytes:\n\n{content}",
            content.len(),
        )
    } else {
        content
    };

    let content = if user_stopped {
        if is_empty {
            "The user stopped this command. No output was captured before stopping.\n\n\
            Since the user intentionally interrupted this command, ask them what they would like to do next \
            rather than automatically retrying or assuming something went wrong.".to_string()
        } else {
            format!("The user stopped this command, here is what it output before stopping:\n\n{content}")
        }
    } else if timed_out {
        if is_empty {
            "The command timed out without producing any output.".to_string()
        } else {
            format!("The command timed out. Here is what it output before timing out:\n\n{content}")
        }
    } else {
        let exit_code = output.exit_status.as_ref().and_then(|s| s.exit_code);
        match exit_code {
            Some(0) => {
                if is_empty {
                    "The command succeeded and produced no output.".to_string()
                } else {
                    format!("The command succeeded and produced the following output:\n\n{content}")
                }
            }
            Some(exit_code) => {
                if is_empty {
                    format!("The command failed with exit code {exit_code} and produced no output.")
                } else {
                    format!("The command failed with exit code {exit_code} and produced the following output:\n\n{content}")
                }
            }
            None => {
                if is_empty {
                    "The command did not produce any output and returned no exit code.".to_string()
                } else {
                    format!("The command returned no exit code, but produced the following output:\n\n{content}")
                }
            }
        }
    };

    format!("Ran command `{command}`\n\n{content}")
}
fn working_dir(input: &SandboxToolInput, project: &Entity<Project>, cx: &App) -> Result<PathBuf> {
    let mut working_dir = None;
    for worktree in project.read(cx).worktrees(cx) {
        let worktree = worktree.read(cx);
        if input.cd == worktree.abs_path().to_string_lossy() {
            working_dir = Some(worktree.abs_path().to_path_buf());
            break;
        }
    }
    working_dir.ok_or_else(|| {
        anyhow::anyhow!("invalid working directory. must be one of the project root directories")
    })
}
