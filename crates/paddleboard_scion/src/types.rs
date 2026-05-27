use std::collections::HashMap;
use std::fmt;

use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScionVersion {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub commit: String,
    #[serde(default)]
    pub short: String,
    #[serde(default)]
    pub build_time: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentPhase {
    Created,
    Provisioning,
    Cloning,
    Starting,
    Running,
    Suspended,
    Stopping,
    Stopped,
    Error,
    #[serde(other)]
    Unknown,
}

impl AgentPhase {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Stopped | Self::Error)
    }

    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Provisioning | Self::Cloning | Self::Starting | Self::Running
        )
    }
}

impl fmt::Display for AgentPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Created => "created",
            Self::Provisioning => "provisioning",
            Self::Cloning => "cloning",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Suspended => "suspended",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
            Self::Error => "error",
            Self::Unknown => "unknown",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentActivity {
    Working,
    Thinking,
    Executing,
    WaitingForInput,
    Blocked,
    Completed,
    LimitsExceeded,
    Stalled,
    Offline,
    Crashed,
    #[serde(other)]
    Unknown,
}

impl AgentActivity {
    pub fn needs_attention(self) -> bool {
        matches!(
            self,
            Self::WaitingForInput | Self::Blocked | Self::LimitsExceeded | Self::Crashed
        )
    }
}

impl fmt::Display for AgentActivity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Working => "working",
            Self::Thinking => "thinking",
            Self::Executing => "executing",
            Self::WaitingForInput => "waiting_for_input",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::LimitsExceeded => "limits_exceeded",
            Self::Stalled => "stalled",
            Self::Offline => "offline",
            Self::Crashed => "crashed",
            Self::Unknown => "unknown",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDetail {
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub task_summary: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub name: String,

    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub container_id: String,
    #[serde(default)]
    pub template: String,
    #[serde(default)]
    pub harness_config: String,
    #[serde(default)]
    pub harness_auth: String,

    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub project_path: String,

    #[serde(default)]
    pub labels: HashMap<String, String>,
    #[serde(default)]
    pub annotations: HashMap<String, String>,

    #[serde(default)]
    pub container_status: String,
    #[serde(default)]
    pub phase: Option<AgentPhase>,
    #[serde(default)]
    pub activity: Option<AgentActivity>,
    #[serde(default)]
    pub detail: Option<AgentDetail>,

    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub detached: bool,
    #[serde(default)]
    pub runtime: String,
    #[serde(default)]
    pub profile: String,
    #[serde(default)]
    pub warnings: Vec<String>,

    #[serde(default)]
    pub created: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_seen: Option<DateTime<Utc>>,

    #[serde(default)]
    pub created_by: String,
    #[serde(default)]
    pub visibility: String,

    #[serde(default)]
    pub runtime_broker_id: String,
    #[serde(default)]
    pub runtime_broker_name: String,
    #[serde(default)]
    pub task_summary: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub harness: String,
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TemplateListResponse {
    #[serde(default)]
    pub local: HashMap<String, Vec<TemplateInfo>>,
    #[serde(default)]
    pub hub: HashMap<String, Vec<TemplateInfo>>,
}

impl TemplateListResponse {
    pub fn into_flat_list(self) -> Vec<TemplateInfo> {
        let mut templates = Vec::new();
        for (_scope, list) in self.local {
            templates.extend(list);
        }
        for (_scope, list) in self.hub {
            templates.extend(list);
        }
        templates
    }
}

pub struct StartAgentOptions {
    pub template: Option<String>,
    pub image: Option<String>,
    pub branch: Option<String>,
    pub detached: bool,
    pub no_auth: bool,
    pub broker: Option<String>,
    pub harness_config: Option<String>,
}

impl Default for StartAgentOptions {
    fn default() -> Self {
        Self {
            template: None,
            image: None,
            branch: None,
            detached: true,
            no_auth: false,
            broker: None,
            harness_config: None,
        }
    }
}

impl StartAgentOptions {
    pub(crate) fn to_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(ref template) = self.template {
            args.extend_from_slice(&["-t".into(), template.clone()]);
        }
        if let Some(ref image) = self.image {
            args.extend_from_slice(&["-i".into(), image.clone()]);
        }
        if let Some(ref branch) = self.branch {
            args.extend_from_slice(&["-b".into(), branch.clone()]);
        }
        if self.detached {
            args.push("-d".into());
        }
        if self.no_auth {
            args.push("--no-auth".into());
        }
        if let Some(ref broker) = self.broker {
            args.extend_from_slice(&["--broker".into(), broker.clone()]);
        }
        if let Some(ref config) = self.harness_config {
            args.extend_from_slice(&["--harness-config".into(), config.clone()]);
        }
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_agent_list() {
        let json = r#"[
            {
                "name": "feature-auth",
                "template": "claude-code",
                "harnessConfig": "claude-code-default",
                "project": "my-app",
                "projectPath": "/Users/dev/my-app",
                "phase": "running",
                "activity": "waiting_for_input",
                "detail": {
                    "toolName": "bash",
                    "message": "Waiting for user input"
                },
                "image": "ghcr.io/org/scion-agent:latest",
                "runtime": "container",
                "created": "2026-05-26T10:00:00Z",
                "lastSeen": "2026-05-26T10:30:00Z"
            }
        ]"#;
        let agents: Vec<AgentInfo> =
            serde_json::from_str(json).expect("sample agent list should parse");
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "feature-auth");
        assert_eq!(agents[0].phase, Some(AgentPhase::Running));
        assert_eq!(agents[0].activity, Some(AgentActivity::WaitingForInput));
        assert_eq!(
            agents[0].detail.as_ref().map(|d| d.tool_name.as_str()),
            Some("bash")
        );
        assert_eq!(agents[0].template, "claude-code");
    }

    #[test]
    fn parse_version() {
        let json = r#"{"buildTime":"2026-05-01","commit":"abc123","short":"v0.1.0","version":"0.1.0"}"#;
        let version: ScionVersion =
            serde_json::from_str(json).expect("version should parse");
        assert_eq!(version.version, "0.1.0");
        assert_eq!(version.commit, "abc123");
        assert_eq!(version.short, "v0.1.0");
    }

    #[test]
    fn parse_template_list() {
        let json = r#"{
            "local": {
                "global": [
                    {
                        "name": "claude-code",
                        "source": "builtin",
                        "harness": "claude-code",
                        "image": "ghcr.io/org/agent:latest",
                        "description": "Claude Code agent"
                    }
                ],
                "project": [
                    {
                        "name": "gemini",
                        "source": "builtin",
                        "harness": "gemini",
                        "image": "",
                        "description": "Gemini CLI agent"
                    }
                ]
            }
        }"#;
        let response: TemplateListResponse =
            serde_json::from_str(json).expect("template response should parse");
        let templates = response.into_flat_list();
        assert_eq!(templates.len(), 2);
        assert!(templates.iter().any(|t| t.name == "claude-code"));
        assert!(templates.iter().any(|t| t.harness == "gemini"));
    }

    #[test]
    fn unknown_phase_deserializes() {
        let json = r#"{"name":"test","phase":"teleporting"}"#;
        let info: AgentInfo =
            serde_json::from_str(json).expect("unknown phase should parse");
        assert_eq!(info.phase, Some(AgentPhase::Unknown));
    }

    #[test]
    fn unknown_activity_deserializes() {
        let json = r#"{"name":"test","activity":"napping"}"#;
        let info: AgentInfo =
            serde_json::from_str(json).expect("unknown activity should parse");
        assert_eq!(info.activity, Some(AgentActivity::Unknown));
    }

    #[test]
    fn minimal_agent_info_parses() {
        let json = r#"{"name":"minimal"}"#;
        let info: AgentInfo =
            serde_json::from_str(json).expect("minimal agent should parse");
        assert_eq!(info.name, "minimal");
        assert!(info.phase.is_none());
        assert!(info.activity.is_none());
        assert!(info.created.is_none());
        assert!(info.labels.is_empty());
        assert!(info.warnings.is_empty());
    }

    #[test]
    fn phase_helpers() {
        assert!(AgentPhase::Running.is_active());
        assert!(!AgentPhase::Running.is_terminal());
        assert!(AgentPhase::Stopped.is_terminal());
        assert!(!AgentPhase::Stopped.is_active());
        assert!(AgentPhase::Error.is_terminal());
    }

    #[test]
    fn activity_needs_attention() {
        assert!(AgentActivity::WaitingForInput.needs_attention());
        assert!(AgentActivity::Blocked.needs_attention());
        assert!(AgentActivity::LimitsExceeded.needs_attention());
        assert!(AgentActivity::Crashed.needs_attention());
        assert!(!AgentActivity::Working.needs_attention());
        assert!(!AgentActivity::Completed.needs_attention());
    }

    #[test]
    fn start_options_to_args() {
        let options = StartAgentOptions {
            template: Some("claude-code".into()),
            branch: Some("feature/auth".into()),
            detached: true,
            ..Default::default()
        };
        let args = options.to_args();
        assert!(args.contains(&"-t".into()));
        assert!(args.contains(&"claude-code".into()));
        assert!(args.contains(&"-b".into()));
        assert!(args.contains(&"feature/auth".into()));
        assert!(args.contains(&"-d".into()));
        assert!(!args.contains(&"--no-auth".into()));
    }

    #[test]
    fn empty_version_parses() {
        let json = r#"{"version":"","commit":"","short":"unknown","buildTime":""}"#;
        let version: ScionVersion =
            serde_json::from_str(json).expect("empty version should parse");
        assert_eq!(version.version, "");
        assert_eq!(version.short, "unknown");
    }

    #[test]
    fn agent_with_all_phases() {
        for phase_str in [
            "created",
            "provisioning",
            "cloning",
            "starting",
            "running",
            "suspended",
            "stopping",
            "stopped",
            "error",
        ] {
            let json = format!(r#"{{"name":"test","phase":"{phase_str}"}}"#);
            let info: AgentInfo = serde_json::from_str(&json)
                .unwrap_or_else(|_| panic!("phase '{phase_str}' should parse"));
            assert_ne!(
                info.phase,
                Some(AgentPhase::Unknown),
                "phase '{phase_str}' should not be Unknown"
            );
        }
    }

    #[test]
    fn phase_display_matches_serde() {
        let phases = [
            (AgentPhase::Created, "created"),
            (AgentPhase::Provisioning, "provisioning"),
            (AgentPhase::Running, "running"),
            (AgentPhase::Stopped, "stopped"),
            (AgentPhase::Error, "error"),
            (AgentPhase::Unknown, "unknown"),
        ];
        for (phase, expected) in phases {
            assert_eq!(phase.to_string(), expected);
        }
    }

    #[test]
    fn activity_display_matches_serde() {
        let activities = [
            (AgentActivity::Working, "working"),
            (AgentActivity::WaitingForInput, "waiting_for_input"),
            (AgentActivity::LimitsExceeded, "limits_exceeded"),
            (AgentActivity::Crashed, "crashed"),
            (AgentActivity::Unknown, "unknown"),
        ];
        for (activity, expected) in activities {
            assert_eq!(activity.to_string(), expected);
        }
    }

    #[test]
    fn agent_with_all_activities() {
        for activity_str in [
            "working",
            "thinking",
            "executing",
            "waiting_for_input",
            "blocked",
            "completed",
            "limits_exceeded",
            "stalled",
            "offline",
            "crashed",
        ] {
            let json = format!(r#"{{"name":"test","activity":"{activity_str}"}}"#);
            let info: AgentInfo = serde_json::from_str(&json)
                .unwrap_or_else(|_| panic!("activity '{activity_str}' should parse"));
            assert_ne!(
                info.activity,
                Some(AgentActivity::Unknown),
                "activity '{activity_str}' should not be Unknown"
            );
        }
    }
}
