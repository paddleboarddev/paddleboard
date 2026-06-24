// PaddleBoard: host-side tool that installs an agent-generated MCP server.
//
// The "Build an MCP" flow runs the agent inside the gVisor sandbox, which can't
// reach host paths — so the agent can't persist the server to the data dir or
// register it in settings itself. This tool runs in the PaddleBoard process
// (`&mut App`), writes the server files to `paths::data_dir()/mcp_servers/<id>/`,
// and registers a plain `Stdio` context server (the same shape catalog installs
// use) so the AI Dock launches it. Plain stdio runs on the host, so it can
// actually execute the data-dir server; SandboxedStdio only mounts the worktree.
use std::{collections::HashMap, sync::Arc};

use agent_client_protocol::schema as acp;
use agent_settings::AgentSettings;
use fs::Fs;
use gpui::{App, SharedString, Task};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings::{
    ContextServerCommand, ContextServerSettingsContent, Settings as _, update_settings_file,
};

use crate::{
    AgentTool, ToolCallEventStream, ToolInput, ToolPermissionDecision,
    decide_permission_from_settings,
};

/// Installs a generated MCP server so PaddleBoard launches it and the agent can use it.
///
/// Use this as the FINAL step of building an MCP server (after the code is written
/// and tested). It writes the given files into PaddleBoard's data directory and
/// registers a context server that runs them on the host via `uv run`. The server
/// then appears in the AI Dock (MCP tab) and starts automatically.
///
/// The server reads any API key from the environment PaddleBoard was launched with
/// (e.g. `os.environ["SUBSTACK_API_KEY"]`). NEVER put secret values in `files` or args.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct InstallMcpServerToolInput {
    /// Slug id for the server: lowercase letters, digits, hyphens (e.g. "substack").
    /// Becomes the AI Dock entry key.
    pub id: String,
    /// Files to write into the server directory, keyed by relative filename. Must
    /// include the Python entry script and usually a `requirements.txt`.
    pub files: HashMap<String, String>,
    /// The entry script filename within `files`. Defaults to "server.py".
    pub entry: Option<String>,
    /// The requirements filename within `files`. Defaults to "requirements.txt" when present.
    pub requirements: Option<String>,
}

pub struct InstallMcpServerTool;

impl AgentTool for InstallMcpServerTool {
    type Input = InstallMcpServerToolInput;
    type Output = String;

    const NAME: &'static str = "install_mcp_server";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Edit
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        if let Ok(input) = input {
            format!("Install MCP server {}", input.id).into()
        } else {
            "Install MCP server".into()
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        cx.spawn(async move |cx| {
            let input = input.recv().await.map_err(|e| e.to_string())?;

            let id = input.id.trim().to_string();
            if id.is_empty()
                || !id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Err(format!(
                    "Invalid server id {id:?}: use lowercase letters, digits, hyphens or underscores."
                ));
            }
            if input.files.is_empty() {
                return Err("No files were provided to install.".to_string());
            }
            let entry = input
                .entry
                .clone()
                .unwrap_or_else(|| "server.py".to_string());
            if !input.files.contains_key(&entry) {
                return Err(format!(
                    "Entry file {entry:?} is not among the provided files: {:?}",
                    input.files.keys().collect::<Vec<_>>()
                ));
            }
            for name in input.files.keys() {
                if name.is_empty()
                    || name.contains("..")
                    || name.starts_with('/')
                    || name.contains('\\')
                {
                    return Err(format!("Invalid filename {name:?} in files."));
                }
            }

            // Permission: installing an auto-launching server warrants a checkpoint.
            let authorize = cx.update(|cx| {
                let decision = decide_permission_from_settings(
                    Self::NAME,
                    std::slice::from_ref(&id),
                    AgentSettings::get_global(cx),
                );
                match decision {
                    ToolPermissionDecision::Allow => Ok(None),
                    ToolPermissionDecision::Deny(reason) => Err(reason),
                    ToolPermissionDecision::Confirm => {
                        let context =
                            crate::ToolPermissionContext::new(Self::NAME, vec![id.clone()]);
                        Ok(Some(event_stream.authorize(
                            format!("Install MCP server {id}"),
                            context,
                            cx,
                        )))
                    }
                }
            })?;
            if let Some(authorize) = authorize {
                authorize.await.map_err(|e| e.to_string())?;
            }

            let fs = cx.update(|cx| <dyn Fs>::global(cx));
            let dir = paths::data_dir().join("mcp_servers").join(&id);
            fs.create_dir(&dir)
                .await
                .map_err(|e| format!("Creating {}: {e}", dir.display()))?;
            for (name, content) in &input.files {
                let path = dir.join(name);
                fs.atomic_write(path.clone(), content.clone())
                    .await
                    .map_err(|e| format!("Writing {}: {e}", path.display()))?;
            }

            let requirements = input.requirements.clone().or_else(|| {
                input
                    .files
                    .contains_key("requirements.txt")
                    .then(|| "requirements.txt".to_string())
            });
            let mut args = vec!["run".to_string()];
            if let Some(req) = requirements
                .as_ref()
                .filter(|req| input.files.contains_key(*req))
            {
                args.push("--with-requirements".to_string());
                args.push(dir.join(req).to_string_lossy().into_owned());
            }
            args.push(dir.join(&entry).to_string_lossy().into_owned());

            let id_key: Arc<str> = id.as_str().into();
            cx.update(|cx| {
                update_settings_file(fs.clone(), cx, move |settings, _| {
                    settings.project.context_servers.insert(
                        id_key,
                        ContextServerSettingsContent::Stdio {
                            enabled: true,
                            remote: false,
                            command: ContextServerCommand {
                                // `uv` resolves from PATH at launch (like the catalog's
                                // `npx`/`uvx`); `uv run` provisions deps on first launch.
                                path: "uv".into(),
                                args,
                                env: None,
                                timeout: None,
                            },
                        },
                    );
                });
            });

            Ok(format!(
                "Installed MCP server '{id}' to {}. It now appears in the AI Dock (MCP tab) and \
                 will launch automatically. Make sure any required API key is exported in the \
                 environment PaddleBoard was launched from.",
                dir.display()
            ))
        })
    }
}
