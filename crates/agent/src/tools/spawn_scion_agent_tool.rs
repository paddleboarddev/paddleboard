use std::sync::Arc;
use std::time::Duration;

use agent_client_protocol::schema::v1 as acp;
use anyhow::Result;
use gpui::{App, Entity, SharedString, Task};
use gpui_tokio::Tokio;
use language_model::LanguageModelToolResultContent;
use paddleboard_scion::{AgentPhase, ScionCli, StartAgentOptions};
use project::Project;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings::Settings as _;

use crate::{AgentTool, ToolCallEventStream, ToolInput};

// PaddleBoard: delegate a subtask to a Scion-managed agent that runs in its own
// container + git worktree, rather than to an in-process subagent that shares this
// workspace. Requires the `scion` CLI on PATH (the tool is only registered when it is).

const POLL_INTERVAL: Duration = Duration::from_secs(5);
const DEFAULT_TIMEOUT_SECS: u64 = 1800;
const LOG_TAIL_LINES: usize = 200;

/// Delegate a well-scoped task to an **isolated** Scion agent.
///
/// Unlike `spawn_agent` (which runs an in-process sub-agent that shares this
/// workspace), this launches a Scion agent that runs in its own container and its
/// own git worktree. Use it when delegated work writes to the filesystem and must
/// not collide with your workspace or with other agents — e.g. parallel
/// implementation tasks that each modify code.
///
/// ### Designing the task
/// - The Scion agent runs in a fresh, isolated checkout and does NOT see this
///   conversation. Put all required context (goal, constraints, file paths) in `task`.
/// - Give it a short, unique, filesystem-safe `name` (e.g. "auth-refactor").
/// - Optionally give it a `persona` from the "Available Personas" list — its
///   identity overlay is prepended to the task, since the agent can't see this
///   session's persona.
/// - Prefer this over `spawn_agent` specifically when isolation matters; for quick
///   in-process lookups, use `spawn_agent` instead.
///
/// ### Output
/// - By default this waits for the agent to finish and returns its final phase plus
///   the tail of its logs.
/// - Set `sync_on_complete` to pull the agent's changes back into this workspace on
///   success; otherwise review them later via the orchestration panel or `scion`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SpawnScionAgentToolInput {
    /// Short, unique, filesystem-safe name for the agent (e.g. "auth-refactor").
    pub name: String,
    /// The full task prompt. The agent has no access to this conversation, so include
    /// all context, requirements, and file paths it needs.
    pub task: String,
    /// Optional Scion template (agent harness/image), e.g. "claude-code". Omit for the default.
    #[serde(default)]
    pub template: Option<String>,
    /// Optional git branch name for the agent's isolated worktree.
    #[serde(default)]
    pub branch: Option<String>,
    /// Optional persona for the agent, by name from the "Available Personas"
    /// list (e.g. "qa-engineer"). The Scion agent runs outside this session, so
    /// the persona's identity overlay is prepended to the task prompt.
    #[serde(default)]
    pub persona: Option<String>,
    /// Wait for the agent to reach a terminal phase before returning (default true).
    /// When false, returns as soon as the agent has started.
    #[serde(default = "default_true")]
    pub wait_for_completion: bool,
    /// On successful completion, run `scion sync from <name>` to pull the agent's
    /// changes into this workspace (default false).
    #[serde(default)]
    pub sync_on_complete: bool,
    /// Maximum seconds to wait for completion before returning with the agent still
    /// running (default 1800). Ignored when `wait_for_completion` is false.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged, rename_all = "snake_case")]
pub enum SpawnScionAgentToolOutput {
    Success {
        name: String,
        /// Terminal (or last observed) Scion phase, e.g. "stopped", "error", "running".
        phase: String,
        /// Whether the agent reached a terminal phase before the tool returned.
        completed: bool,
        /// Whether the agent's changes were synced back into the workspace.
        synced: bool,
        /// Human-readable summary plus the tail of the agent's logs.
        output: String,
    },
    Error {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        error: String,
    },
}

impl From<SpawnScionAgentToolOutput> for LanguageModelToolResultContent {
    fn from(output: SpawnScionAgentToolOutput) -> Self {
        serde_json::to_string(&output)
            .unwrap_or_else(|e| format!("Failed to serialize spawn_scion_agent output: {e}"))
            .into()
    }
}

/// Tool that delegates a task to an isolated Scion agent.
pub struct SpawnScionAgentTool {
    project: Entity<Project>,
}

impl SpawnScionAgentTool {
    pub fn new(project: Entity<Project>) -> Self {
        Self { project }
    }
}

fn error_output(name: Option<String>, error: impl Into<String>) -> SpawnScionAgentToolOutput {
    SpawnScionAgentToolOutput::Error {
        name,
        error: error.into(),
    }
}

// PaddleBoard: the `name`/`branch`/`template`/`task` fields are chosen freely by the
// model and reach the `scion` CLI as argv (positional operands and flag values). With
// no shell involved this is not command injection, but a value beginning with `-` would
// be parsed by scion as an option (argv injection), and `name` is used as a worktree
// slug, so `..`/`/` could escape the intended directory. Validate before spawning.
fn is_safe_slug(value: &str, allow_slash: bool) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && !value.contains("..")
        && value
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && value.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') || (allow_slash && c == '/')
        })
}

fn validate_scion_input(input: &SpawnScionAgentToolInput) -> Result<(), String> {
    if !is_safe_slug(&input.name, false) {
        return Err(format!(
            "Invalid agent name {:?}: start with a letter/digit and use only [A-Za-z0-9._-].",
            input.name
        ));
    }
    if let Some(template) = &input.template
        && !is_safe_slug(template, false)
    {
        return Err(format!(
            "Invalid template {template:?}: start with a letter/digit and use only [A-Za-z0-9._-]."
        ));
    }
    if let Some(branch) = &input.branch
        && !is_safe_slug(branch, true)
    {
        return Err(format!(
            "Invalid branch {branch:?}: start with a letter/digit and use only [A-Za-z0-9._/-]."
        ));
    }
    if input.task.trim_start().starts_with('-') {
        return Err("Invalid task: must not begin with '-'.".to_string());
    }
    Ok(())
}

impl AgentTool for SpawnScionAgentTool {
    type Input = SpawnScionAgentToolInput;
    type Output = SpawnScionAgentToolOutput;

    const NAME: &'static str = "spawn_scion_agent";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Other
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        match input {
            Ok(input) => format!("Delegating to Scion: {}", input.name).into(),
            Err(value) => value
                .get("name")
                .and_then(|name| name.as_str())
                .map(|name| SharedString::from(format!("Delegating to Scion: {name}")))
                .unwrap_or_else(|| "Spawning Scion agent".into()),
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        // Resolve the project root for the scion working directory before going async.
        let project_dir = self
            .project
            .read(cx)
            .worktrees(cx)
            .next()
            .map(|worktree| worktree.read(cx).abs_path().to_path_buf());
        let personas_enabled =
            paddleboard_personas_settings::PersonasSettings::get_global(cx).enabled;

        cx.spawn(async move |cx| {
            let input = input
                .recv()
                .await
                .map_err(|e| error_output(None, e.to_string()))?;

            let name = input.name.clone();

            if let Err(message) = validate_scion_input(&input) {
                return Err(error_output(Some(name), message));
            }

            // Scion agents run outside this session, so a persona can't ride the
            // system prompt — resolve it here and prepend its overlay to the task.
            let task = if let Some(requested) = input
                .persona
                .clone()
                .filter(|requested| !requested.trim().is_empty())
            {
                if !personas_enabled {
                    return Err(error_output(
                        Some(name),
                        "The persona system is disabled in settings, so the `persona` \
                         parameter is unavailable.",
                    ));
                }
                let Some(root) = project_dir.clone() else {
                    return Err(error_output(
                        Some(name),
                        "The `persona` parameter needs an open project to discover personas.",
                    ));
                };
                let personas = cx
                    .background_executor()
                    .spawn(async move { paddleboard_personas::discover(Some(root.as_path())) })
                    .await;
                let Some(persona) = personas
                    .iter()
                    .find(|persona| persona.name == requested)
                    .or_else(|| {
                        personas
                            .iter()
                            .find(|persona| persona.name.eq_ignore_ascii_case(&requested))
                    })
                else {
                    let available = personas
                        .iter()
                        .map(|persona| persona.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(error_output(
                        Some(name),
                        format!(
                            "No persona named '{requested}'. Available: {}",
                            if available.is_empty() { "(none)" } else { &available }
                        ),
                    ));
                };
                let overlay = paddleboard_personas::build_overlay(persona, &personas);
                paddleboard_personas::prepend_overlay_to_task(&overlay, &input.task)
            } else {
                input.task.clone()
            };

            let cli = {
                let mut cli = ScionCli::new();
                if let Some(dir) = project_dir {
                    cli = cli.with_project_dir(dir);
                }
                Arc::new(cli)
            };

            if !cli.is_available() {
                return Err(error_output(
                    Some(name),
                    "The `scion` CLI is not installed or not on PATH. Install it with: \
                     go install github.com/GoogleCloudPlatform/scion/cmd/scion@latest",
                ));
            }

            let options = StartAgentOptions {
                template: input.template.clone(),
                branch: input.branch.clone(),
                detached: true,
                ..Default::default()
            };

            let start_result = Tokio::spawn_result(cx, {
                let cli = cli.clone();
                let name = name.clone();
                let task = task.clone();
                async move { cli.start_agent(&name, Some(&task), &options).await }
            })
            .await;

            if let Err(error) = start_result {
                return Err(error_output(
                    Some(name),
                    format!("failed to start scion agent: {error}"),
                ));
            }

            event_stream.update_fields(
                acp::ToolCallUpdateFields::new()
                    .content(vec![format!("Started Scion agent '{name}'.").into()]),
            );

            let mut last_phase = AgentPhase::Created;
            let mut completed = false;

            if input.wait_for_completion {
                let timeout_secs = input.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS);
                let max_polls = (timeout_secs / POLL_INTERVAL.as_secs()).max(1);

                for _ in 0..max_polls {
                    cx.background_executor().timer(POLL_INTERVAL).await;

                    let agents = Tokio::spawn_result(cx, {
                        let cli = cli.clone();
                        async move { cli.list_agents(true, false).await }
                    })
                    .await;

                    let agents = match agents {
                        Ok(agents) => agents,
                        Err(error) => {
                            return Err(error_output(
                                Some(name),
                                format!("failed to poll scion agents: {error}"),
                            ));
                        }
                    };

                    let Some(agent) = agents.into_iter().find(|agent| agent.name == name) else {
                        // The agent is no longer listed; treat it as gone.
                        break;
                    };

                    last_phase = agent.phase.unwrap_or(AgentPhase::Unknown);
                    if last_phase.is_terminal() {
                        completed = true;
                        break;
                    }
                }
            } else {
                // Best-effort single read so we can report the current phase.
                if let Ok(agents) = Tokio::spawn_result(cx, {
                    let cli = cli.clone();
                    async move { cli.list_agents(true, false).await }
                })
                .await
                {
                    if let Some(agent) = agents.into_iter().find(|agent| agent.name == name) {
                        last_phase = agent.phase.unwrap_or(AgentPhase::Unknown);
                    }
                }
            }

            let logs = match Tokio::spawn_result(cx, {
                let cli = cli.clone();
                let name = name.clone();
                async move { cli.agent_logs(&name, Some(LOG_TAIL_LINES)).await }
            })
            .await
            {
                Ok(logs) => logs,
                Err(error) => format!("(could not fetch logs: {error})"),
            };

            let mut synced = false;
            if input.sync_on_complete && completed && last_phase == AgentPhase::Stopped {
                match Tokio::spawn_result(cx, {
                    let cli = cli.clone();
                    let name = name.clone();
                    async move { cli.sync_from(&name).await }
                })
                .await
                {
                    Ok(_) => synced = true,
                    Err(error) => {
                        log::warn!("scion sync from {name} failed: {error}");
                    }
                }
            }

            let phase = last_phase.to_string();
            let summary = if completed {
                let sync_note = if synced {
                    " Changes were synced into your workspace."
                } else if input.sync_on_complete {
                    " Changes were NOT synced (agent did not finish successfully)."
                } else {
                    ""
                };
                format!("Scion agent '{name}' finished in phase '{phase}'.{sync_note}")
            } else if input.wait_for_completion {
                format!(
                    "Scion agent '{name}' is still running (phase '{phase}') after the timeout. \
                     Track it in the Orchestration panel or with `scion`."
                )
            } else {
                format!("Scion agent '{name}' started (phase '{phase}').")
            };

            let output = format!("{summary}\n\nRecent logs:\n{logs}");

            event_stream.update_fields(
                acp::ToolCallUpdateFields::new().content(vec![output.clone().into()]),
            );

            Ok(SpawnScionAgentToolOutput::Success {
                name,
                phase,
                completed,
                synced,
                output,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_uses_sensible_defaults() {
        let input: SpawnScionAgentToolInput =
            serde_json::from_str(r#"{"name":"auth-refactor","task":"do the thing"}"#)
                .expect("minimal input should parse");
        assert!(input.wait_for_completion);
        assert!(!input.sync_on_complete);
        assert!(input.template.is_none());
        assert!(input.branch.is_none());
        assert!(input.persona.is_none());
        assert!(input.timeout_secs.is_none());
    }

    #[test]
    fn input_accepts_a_persona() {
        let input: SpawnScionAgentToolInput = serde_json::from_str(
            r#"{"name":"auth-refactor","task":"do the thing","persona":"qa-engineer"}"#,
        )
        .expect("input with persona should parse");
        assert_eq!(input.persona.as_deref(), Some("qa-engineer"));
    }

    #[test]
    fn success_output_serializes_expected_fields() {
        let output = SpawnScionAgentToolOutput::Success {
            name: "auth-refactor".into(),
            phase: "stopped".into(),
            completed: true,
            synced: false,
            output: "done".into(),
        };
        let json = serde_json::to_string(&output).expect("success should serialize");
        assert!(json.contains(r#""phase":"stopped""#));
        assert!(json.contains(r#""completed":true"#));
        assert!(json.contains(r#""synced":false"#));
    }

    #[test]
    fn error_output_round_trips_through_untagged_enum() {
        let json = serde_json::to_string(&error_output(Some("auth-refactor".into()), "boom"))
            .expect("error should serialize");
        let parsed: SpawnScionAgentToolOutput =
            serde_json::from_str(&json).expect("error should deserialize");
        match parsed {
            SpawnScionAgentToolOutput::Error { name, error } => {
                assert_eq!(name.as_deref(), Some("auth-refactor"));
                assert_eq!(error, "boom");
            }
            SpawnScionAgentToolOutput::Success { .. } => panic!("expected the error variant"),
        }
    }
}
