use agent_client_protocol::schema as acp;
use agent_settings::AgentSettings;
use anyhow::Result;
use futures::FutureExt as _;
use gpui::{App, Entity, SharedString, Task};
use paddleboard_sandbox_prereqs_state::SandboxPrereqs;
use paddleboard_sandbox_settings::{SandboxGateDecision, SandboxSettings, decide_gate};
use project::Project;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings::Settings;
use std::{
    path::PathBuf,
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
/// that could potentially harm the host system. The command runs inside an ephemeral Ubuntu
/// container; the project worktree identified by `cd` is mounted at `/workspace` inside the
/// container and the command starts with that as its working directory, so file changes made
/// there are visible on the host.
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

            let (working_dir, authorize, gate) = cx.update(|cx| {
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

                let gate = decide_gate(
                    SandboxPrereqs::status(cx),
                    SandboxSettings::get_global(cx),
                );

                Ok((working_dir, authorize, gate))
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
                            "PaddleBoard sandbox: {reason}. Running sandboxed anyway; \
                             open Sandbox Prerequisites to install."
                        );
                    }
                    false
                }
                SandboxGateDecision::FallBackToHost { reason } => {
                    log::warn!(
                        "PaddleBoard sandbox: {reason}. Falling back to host execution \
                         per `paddleboard_sandbox.on_missing_runtime`."
                    );
                    true
                }
                SandboxGateDecision::Allow => false,
            };

            let image = input
                .image
                .clone()
                .unwrap_or_else(|| "ubuntu:latest".to_string());

            // Mount the host worktree at a fixed in-container path so the container filesystem
            // layout is stable and host paths do not leak through the mount point. The user's
            // shell command and any host path we interpolate are wrapped with POSIX single-quote
            // escaping (close quote, backslash-escape, reopen quote) so they survive both the
            // outer shell that spawns podman and the `bash -c` inside the container.
            const CONTAINER_WORKDIR: &str = "/workspace";
            let host_wd = shell_single_quote(&working_dir.to_string_lossy());
            let container_wd = shell_single_quote(CONTAINER_WORKDIR);
            let image_arg = shell_single_quote(&image);
            let user_command = shell_single_quote(&input.command);
            let command = if run_on_host {
                // No podman wrapper: the command runs in the host shell. The
                // worktree is the working directory we pass to `create_terminal`,
                // so the user's command sees the same `cd` it would have inside
                // the container — just without isolation.
                format!("bash -c {user_command}")
            } else {
                format!(
                    "podman run --rm --runtime=runsc -v {host_wd}:{container_wd} -w {container_wd} {image_arg} bash -c {user_command}",
                )
            };

            let terminal = self
                .environment
                .create_terminal(
                    command.clone(),
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
    resolve_worktree_dir(&input.cd, project, cx)
}

pub(super) fn resolve_worktree_dir(
    cd: &str,
    project: &Entity<Project>,
    cx: &App,
) -> Result<PathBuf> {
    // We compare via canonical paths when possible so that trailing slashes, `.`/`..` components,
    // or symlink differences between what the model emits and what the worktree stores do not
    // cause spurious "invalid working directory" errors. Canonicalization can fail (e.g. permission
    // errors), so we fall back to the raw PathBuf in that case, which is strictly more permissive
    // than the previous `to_string_lossy()` equality check.
    let input_path = PathBuf::from(cd);
    let canonical_input = std::fs::canonicalize(&input_path).unwrap_or(input_path);
    for worktree in project.read(cx).worktrees(cx) {
        let worktree = worktree.read(cx);
        let worktree_abs = worktree.abs_path();
        let canonical_worktree =
            std::fs::canonicalize(&worktree_abs).unwrap_or_else(|_| worktree_abs.to_path_buf());
        if canonical_input == canonical_worktree {
            return Ok(worktree_abs.to_path_buf());
        }
    }
    anyhow::bail!("invalid working directory. must be one of the project root directories")
}

/// POSIX shell single-quote escaping: wrap `s` in single quotes, and replace every `'` in `s`
/// with `'\''` (close quote, escaped literal quote, reopen quote). The result is safe to pass
/// through `bash -c` and through any POSIX shell interpolation, regardless of the original
/// contents of `s`.
pub(super) fn shell_single_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::shell_single_quote;

    #[test]
    fn quotes_plain_strings() {
        assert_eq!(shell_single_quote("hello"), "'hello'");
        assert_eq!(shell_single_quote(""), "''");
    }

    #[test]
    fn escapes_embedded_single_quotes() {
        assert_eq!(shell_single_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_single_quote("'"), "''\\'''");
    }

    #[test]
    fn passes_through_shell_metacharacters_verbatim() {
        assert_eq!(
            shell_single_quote("rm -rf $HOME && echo pwned"),
            "'rm -rf $HOME && echo pwned'"
        );
    }
}
