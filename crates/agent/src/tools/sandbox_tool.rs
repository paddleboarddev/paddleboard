use agent_client_protocol::schema::v1 as acp;
use agent_settings::AgentSettings;
use anyhow::Result;
use futures::FutureExt as _;
use gpui::{App, Entity, SharedString, Task};
use paddleboard_container_engine::{EngineKind, ExecRequest};
use paddleboard_sandbox_prereqs_state::SandboxPrereqs;
use paddleboard_sandbox_settings::{
    BuiltInCapability, NativeBackend, SandboxGateDecision, SandboxSettings, decide_gate,
};
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

/// Executes a shell command securely within an isolated container — a Podman (gVisor runsc)
/// container when Podman is installed, or PaddleBoard's built-in microVM sandbox otherwise.
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

            let (working_dir, authorize, gate, builtin_gap) = cx.update(|cx| {
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
                    BuiltInCapability::Supported,
                    SandboxSettings::get_global(cx),
                );
                let builtin_gap = SandboxPrereqs::status(cx)
                    .and_then(|status| status.builtin.unavailable_reason());

                Ok((working_dir, authorize, gate, builtin_gap))
            })?;
            if let Some(authorize) = authorize {
                authorize.await.map_err(|e| e.to_string())?;
            }

            let engine_kind = match &gate {
                SandboxGateDecision::Block { reason } => {
                    let builtin_gap = builtin_gap.map_or(String::new(), |gap| {
                        format!(" The built-in microVM sandbox is also unavailable: {gap}.")
                    });
                    return Err(format!(
                        "Sandbox prerequisites missing: {reason}.{builtin_gap} \
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
                    Some(EngineKind::PodmanGvisor)
                }
                SandboxGateDecision::FallBackToHost { reason } => {
                    log::warn!(
                        "PaddleBoard sandbox: {reason}. Falling back to host execution \
                         per `paddleboard_sandbox.on_missing_runtime`."
                    );
                    None
                }
                SandboxGateDecision::UseBuiltIn { reason, backend } => {
                    let (kind, label) = match backend {
                        NativeBackend::AppleContainer => {
                            (EngineKind::AppleContainer, "Apple container")
                        }
                        NativeBackend::BuiltInKrun => (EngineKind::BuiltInKrun, "built-in microVM"),
                    };
                    log::info!("PaddleBoard sandbox: {reason}; running in the {label} sandbox.");
                    Some(kind)
                }
                SandboxGateDecision::Allow => Some(EngineKind::PodmanGvisor),
            };

            let image = input
                .image
                .clone()
                .unwrap_or_else(|| paddleboard_container_engine::DEFAULT_SANDBOX_IMAGE.to_string());

            // The engine mounts the host worktree at a fixed in-container path
            // (`/workspace`) so the container filesystem layout is stable and host
            // paths do not leak through the mount point. All interpolated strings are
            // single-quote escaped by the engine, so they survive both the outer shell
            // and the `bash -c` inside the container.
            let command = match engine_kind {
                None => {
                    // No container wrapper: the command runs in the host shell. The
                    // worktree is the working directory we pass to `create_terminal`,
                    // so the user's command sees the same `cd` it would have inside
                    // the container — just without isolation.
                    format!("bash -c {}", shell_single_quote(&input.command))
                }
                Some(kind) => {
                    let engine = paddleboard_container_engine::engine(kind);
                    // First use of the built-in tier downloads the image; tell the
                    // user what the wait is.
                    if !engine.is_image_ready(&image) {
                        event_stream.update_fields(
                            acp::ToolCallUpdateFields::new()
                                .title(format!("Sandbox: pulling {image}…")),
                        );
                    }
                    let prepared = engine
                        .prepare_exec(ExecRequest {
                            image: image.clone(),
                            host_workdir: working_dir.clone(),
                            command: input.command.clone(),
                        })
                        .await
                        .map_err(|e| format!("failed to prepare sandbox: {e:#}"))?;
                    event_stream.update_fields(
                        acp::ToolCallUpdateFields::new()
                            .title(format!("Sandbox: {}", input.command)),
                    );
                    prepared.shell_command
                }
            };

            let terminal = self
                .environment
                .create_terminal(
                    command.clone(),
                    // PaddleBoard: this tool builds its own `podman run` invocation
                    // (with `-e` forwarding) inside `command`, so it passes no extra
                    // env and no upstream sandbox_wrap.
                    Vec::new(),
                    Some(working_dir),
                    Some(COMMAND_OUTPUT_LIMIT),
                    None,
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

// Shared with sandbox_service_tool; the implementation (and its tests) moved
// to the engine crate alongside the command builders that depend on it.
pub(super) use paddleboard_container_engine::shell_single_quote;
