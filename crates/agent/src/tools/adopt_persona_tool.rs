use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use anyhow::Result;
use gpui::{App, AppContext as _, Entity, SharedString, Task, WeakEntity};
use language_model::LanguageModelToolResultContent;
use project::Project;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{AgentTool, Thread, ThreadPersona, ToolCallEventStream, ToolInput};

// PaddleBoard: persona adoption. Lets the model honor "be my QA tester" by
// switching this thread's persona itself. The system prompt is rebuilt per
// request, so the change applies from the very next completion. Registered
// only while the persona system is enabled.

/// Adopt one of the user's personas for this thread, or drop the active one.
///
/// Personas are identities the user has defined — voice, values, and
/// behavioral rules you hold for the whole thread. The available personas are
/// listed in your system prompt under "Available Personas".
///
/// - Call with `name` set to a listed persona's name to adopt it.
/// - Call with no `name` to drop the active persona.
/// - Only change personas when the user asks you to.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AdoptPersonaToolInput {
    /// Name of the persona to adopt, exactly as listed under "Available
    /// Personas". Omit to drop the currently active persona.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdoptPersonaToolOutput {
    /// The persona now active for this thread, or null when cleared.
    pub active_persona: Option<String>,
    pub message: String,
}

impl From<AdoptPersonaToolOutput> for LanguageModelToolResultContent {
    fn from(output: AdoptPersonaToolOutput) -> Self {
        serde_json::to_string(&output)
            .unwrap_or_else(|e| format!("Failed to serialize adopt_persona output: {e}"))
            .into()
    }
}

pub struct AdoptPersonaTool {
    thread: WeakEntity<Thread>,
    project: Entity<Project>,
}

impl AdoptPersonaTool {
    pub fn new(thread: WeakEntity<Thread>, project: Entity<Project>) -> Self {
        Self { thread, project }
    }
}

fn error_output(message: impl Into<String>) -> AdoptPersonaToolOutput {
    AdoptPersonaToolOutput {
        active_persona: None,
        message: message.into(),
    }
}

impl AgentTool for AdoptPersonaTool {
    type Input = AdoptPersonaToolInput;
    type Output = AdoptPersonaToolOutput;

    const NAME: &'static str = "adopt_persona";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Other
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        match input {
            Ok(AdoptPersonaToolInput { name: Some(name) }) => {
                format!("Adopting persona: {name}").into()
            }
            _ => "Changing persona".into(),
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        _event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        let project_root = self
            .project
            .read(cx)
            .visible_worktrees(cx)
            .next()
            .map(|worktree| worktree.read(cx).abs_path().to_path_buf());
        let thread = self.thread.clone();

        cx.spawn(async move |cx| {
            let input = input.recv().await.map_err(|e| error_output(e.to_string()))?;

            let Some(name) = input.name.filter(|name| !name.trim().is_empty()) else {
                thread
                    .update(cx, |thread, cx| thread.set_persona(None, cx))
                    .map_err(|e| error_output(e.to_string()))?;
                return Ok(AdoptPersonaToolOutput {
                    active_persona: None,
                    message: "Dropped the active persona. Respond in your default voice from \
                              now on."
                        .to_string(),
                });
            };

            let personas = cx
                .background_spawn(async move {
                    paddleboard_personas::discover(project_root.as_deref())
                })
                .await;

            let Some(persona) = personas
                .iter()
                .find(|persona| persona.name == name)
                .or_else(|| {
                    personas
                        .iter()
                        .find(|persona| persona.name.eq_ignore_ascii_case(&name))
                })
            else {
                let available = personas
                    .iter()
                    .map(|persona| persona.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(error_output(format!(
                    "No persona named '{name}'. Available: {}",
                    if available.is_empty() {
                        "(none)"
                    } else {
                        &available
                    }
                )));
            };

            let adopted_name = persona.name.clone();
            let overlay = paddleboard_personas::build_overlay(persona);
            thread
                .update(cx, |thread, cx| {
                    thread.set_persona(
                        Some(ThreadPersona {
                            name: adopted_name.clone().into(),
                            overlay,
                        }),
                        cx,
                    )
                })
                .map_err(|e| error_output(e.to_string()))?;

            Ok(AdoptPersonaToolOutput {
                active_persona: Some(adopted_name.clone()),
                message: format!(
                    "Adopted persona '{adopted_name}'. Its identity, values, and behavioral \
                     rules now apply — embody them starting with your next response and hold \
                     them for the rest of the thread."
                ),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_name_is_optional() {
        let input: AdoptPersonaToolInput =
            serde_json::from_str("{}").expect("empty input should parse");
        assert!(input.name.is_none());

        let input: AdoptPersonaToolInput = serde_json::from_str(r#"{"name":"qa-engineer"}"#)
            .expect("named input should parse");
        assert_eq!(input.name.as_deref(), Some("qa-engineer"));
    }

    #[test]
    fn output_serializes_active_persona() {
        let output = AdoptPersonaToolOutput {
            active_persona: Some("qa-engineer".into()),
            message: "adopted".into(),
        };
        let json = serde_json::to_string(&output).expect("output should serialize");
        assert!(json.contains(r#""active_persona":"qa-engineer""#));
    }
}
