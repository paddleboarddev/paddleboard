pub mod client;
pub mod listener;
pub mod oauth;
pub mod protocol;
#[cfg(any(test, feature = "test-support"))]
pub mod test;
pub mod transport;
pub mod types;

use collections::HashMap;
use http_client::HttpClient;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::{fmt::Display, path::PathBuf};

use anyhow::Result;
use client::Client;
use gpui::AsyncApp;
use parking_lot::RwLock;
pub use settings::ContextServerCommand;
use url::Url;

use crate::transport::{HttpTransport, SandboxConfig};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContextServerId(pub Arc<str>);

impl Display for ContextServerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

enum ContextServerTransport {
    Stdio(ContextServerCommand, Option<PathBuf>),
    SandboxedStdio(ContextServerCommand, SandboxConfig),
    Custom(Arc<dyn crate::transport::Transport>),
}

// PaddleBoard: `stderr_log` stores recent stderr lines for the MCP inline-logs UI.
pub type StderrLog = Arc<std::sync::Mutex<std::collections::VecDeque<String>>>;

// PaddleBoard: bundles the stderr buffer with a broadcast channel for live streaming.
#[derive(Clone)]
pub struct StderrLogHandle {
    pub buffer: StderrLog,
    pub sender: async_channel::Sender<String>,
}

impl StderrLogHandle {
    pub fn new() -> (Self, async_channel::Receiver<String>) {
        let (sender, receiver) = async_channel::unbounded();
        let handle = Self {
            buffer: Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new())),
            sender,
        };
        (handle, receiver)
    }
}

pub struct ContextServer {
    id: ContextServerId,
    client: RwLock<Option<Arc<crate::protocol::InitializedContextServerProtocol>>>,
    configuration: ContextServerTransport,
    request_timeout: Option<Duration>,
    stderr_log: Option<StderrLog>,
    stderr_sender: Option<async_channel::Sender<String>>,
}

impl ContextServer {
    pub fn stdio(
        id: ContextServerId,
        command: ContextServerCommand,
        working_directory: Option<Arc<Path>>,
    ) -> Self {
        Self {
            id,
            client: RwLock::new(None),
            configuration: ContextServerTransport::Stdio(
                command,
                working_directory.map(|directory| directory.to_path_buf()),
            ),
            request_timeout: None,
            stderr_log: None,
            stderr_sender: None,
        }
    }

    pub fn sandboxed_stdio(
        id: ContextServerId,
        command: ContextServerCommand,
        image: String,
        forward_env: Vec<String>,
        mount_worktree: bool,
        working_directory: Option<Arc<Path>>,
    ) -> Self {
        Self {
            id,
            client: RwLock::new(None),
            configuration: ContextServerTransport::SandboxedStdio(
                command,
                SandboxConfig {
                    image,
                    forward_env,
                    mount_worktree,
                    host_worktree: working_directory.map(|directory| directory.to_path_buf()),
                },
            ),
            request_timeout: None,
            stderr_log: None,
            stderr_sender: None,
        }
    }

    pub fn http(
        id: ContextServerId,
        endpoint: &Url,
        headers: HashMap<String, String>,
        http_client: Arc<dyn HttpClient>,
        executor: gpui::BackgroundExecutor,
        request_timeout: Option<Duration>,
    ) -> Result<Self> {
        let transport = match endpoint.scheme() {
            "http" | "https" => {
                log::info!("Using HTTP transport for {}", endpoint);
                let transport =
                    HttpTransport::new(http_client, endpoint.to_string(), headers, executor);
                Arc::new(transport) as _
            }
            _ => anyhow::bail!("unsupported MCP url scheme {}", endpoint.scheme()),
        };
        Ok(Self::new_with_timeout(id, transport, request_timeout))
    }

    pub fn new(id: ContextServerId, transport: Arc<dyn crate::transport::Transport>) -> Self {
        Self::new_with_timeout(id, transport, None)
    }

    pub fn new_with_timeout(
        id: ContextServerId,
        transport: Arc<dyn crate::transport::Transport>,
        request_timeout: Option<Duration>,
    ) -> Self {
        Self {
            id,
            client: RwLock::new(None),
            configuration: ContextServerTransport::Custom(transport),
            request_timeout,
            stderr_log: None,
            stderr_sender: None,
        }
    }

    pub fn id(&self) -> ContextServerId {
        self.id.clone()
    }

    pub fn client(&self) -> Option<Arc<crate::protocol::InitializedContextServerProtocol>> {
        self.client.read().clone()
    }

    // PaddleBoard: start with an optional stderr log handle for the MCP inline-logs UI.
    pub async fn start_with_log(
        &self,
        log_handle: Option<StderrLogHandle>,
        cx: &AsyncApp,
    ) -> Result<()> {
        let (buf, sender) = match log_handle {
            Some(h) => (Some(h.buffer), Some(h.sender)),
            None => (None, None),
        };
        self.initialize(self.new_client(buf, sender, cx)?).await
    }

    pub async fn start(&self, cx: &AsyncApp) -> Result<()> {
        self.initialize(self.new_client(self.stderr_log.clone(), self.stderr_sender.clone(), cx)?).await
    }

    fn new_client(
        &self,
        log: Option<StderrLog>,
        sender: Option<async_channel::Sender<String>>,
        cx: &AsyncApp,
    ) -> Result<Client> {
        Ok(match &self.configuration {
            ContextServerTransport::Stdio(command, working_directory) => Client::stdio(
                client::ContextServerId(self.id.0.clone()),
                client::ModelContextServerBinary {
                    executable: Path::new(&command.path).to_path_buf(),
                    args: command.args.clone(),
                    env: command.env.clone(),
                    timeout: command.timeout,
                },
                working_directory,
                log,
                sender,
                cx.clone(),
            )?,
            ContextServerTransport::SandboxedStdio(command, sandbox) => Client::sandboxed_stdio(
                client::ContextServerId(self.id.0.clone()),
                client::ModelContextServerBinary {
                    executable: Path::new(&command.path).to_path_buf(),
                    args: command.args.clone(),
                    env: command.env.clone(),
                    timeout: command.timeout,
                },
                sandbox,
                log,
                sender,
                cx.clone(),
            )?,
            ContextServerTransport::Custom(transport) => Client::new(
                client::ContextServerId(self.id.0.clone()),
                self.id().0,
                transport.clone(),
                self.request_timeout,
                log,
                sender,
                cx.clone(),
            )?,
        })
    }

    async fn initialize(&self, client: Client) -> Result<()> {
        log::debug!("starting context server {}", self.id);
        let protocol = crate::protocol::ModelContextProtocol::new(client);
        let client_info = types::Implementation {
            name: "PaddleBoard".to_string(),
            title: None,
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: None,
        };
        let initialized_protocol = protocol.initialize(client_info).await?;

        log::debug!(
            "context server {} initialized: {:?}",
            self.id,
            initialized_protocol.initialize,
        );

        *self.client.write() = Some(Arc::new(initialized_protocol));
        Ok(())
    }

    pub fn stop(&self) -> Result<()> {
        let mut client = self.client.write();
        if let Some(protocol) = client.take() {
            drop(protocol);
        }
        Ok(())
    }
}
