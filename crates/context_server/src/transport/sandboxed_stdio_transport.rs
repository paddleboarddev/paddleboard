use std::path::PathBuf;
use std::pin::Pin;

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use futures::io::{BufReader, BufWriter};
use futures::{
    AsyncBufReadExt as _, AsyncRead, AsyncWrite, AsyncWriteExt as _, Stream, StreamExt as _,
};
use gpui::AsyncApp;
use smol::channel;
use smol::process::Child;
use util::TryFutureExt as _;

use crate::client::ModelContextServerBinary;
use crate::transport::Transport;

/// Configuration for running an MCP server inside a Podman + gVisor sandbox.
///
/// Mirrors the `sandbox_service_tool` execution model: the binary the user thinks of
/// as their MCP server is launched inside `podman run -i --rm --runtime=runsc ...`
/// rather than directly on the host. Stdin/stdout/stderr are proxied through the
/// `podman` client process, so the MCP JSON-RPC framing continues to work
/// transparently from the agent's perspective.
pub struct SandboxConfig {
    pub image: String,
    pub forward_env: Vec<String>,
    pub mount_worktree: bool,
    pub host_worktree: Option<PathBuf>,
}

pub struct SandboxedStdioTransport {
    stdout_sender: channel::Sender<String>,
    stdin_receiver: channel::Receiver<String>,
    stderr_receiver: channel::Receiver<String>,
    server: Child,
}

impl SandboxedStdioTransport {
    pub fn new(
        binary: ModelContextServerBinary,
        sandbox: &SandboxConfig,
        cx: &AsyncApp,
    ) -> Result<Self> {
        let argv = build_podman_argv(&binary, sandbox);

        let mut command = smol::process::Command::new("podman");
        command
            .args(&argv)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let mut server = command.spawn().with_context(|| {
            format!("failed to spawn podman with args {argv:?} for sandboxed MCP server")
        })?;

        let stdin = server.stdin.take().unwrap();
        let stdout = server.stdout.take().unwrap();
        let stderr = server.stderr.take().unwrap();

        let (stdin_sender, stdin_receiver) = channel::unbounded::<String>();
        let (stdout_sender, stdout_receiver) = channel::unbounded::<String>();
        let (stderr_sender, stderr_receiver) = channel::unbounded::<String>();

        cx.spawn(async move |_| Self::handle_output(stdin, stdout_receiver).log_err().await)
            .detach();

        cx.spawn(async move |_| Self::handle_input(stdout, stdin_sender).await)
            .detach();

        cx.spawn(async move |_| Self::handle_err(stderr, stderr_sender).await)
            .detach();

        Ok(Self {
            stdout_sender,
            stdin_receiver,
            stderr_receiver,
            server,
        })
    }

    async fn handle_input<Stdout>(stdin: Stdout, inbound_rx: channel::Sender<String>)
    where
        Stdout: AsyncRead + Unpin + Send + 'static,
    {
        let mut stdin = BufReader::new(stdin);
        let mut line = String::new();
        while let Ok(n) = stdin.read_line(&mut line).await {
            if n == 0 {
                break;
            }
            if inbound_rx.send(line.clone()).await.is_err() {
                break;
            }
            line.clear();
        }
    }

    async fn handle_output<Stdin>(
        stdin: Stdin,
        outbound_rx: channel::Receiver<String>,
    ) -> Result<()>
    where
        Stdin: AsyncWrite + Unpin + Send + 'static,
    {
        let mut stdin = BufWriter::new(stdin);
        let mut pinned_rx = Box::pin(outbound_rx);
        while let Some(message) = pinned_rx.next().await {
            log::trace!("outgoing message: {}", message);
            stdin.write_all(message.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await?;
        }
        Ok(())
    }

    async fn handle_err<Stderr>(stderr: Stderr, stderr_tx: channel::Sender<String>)
    where
        Stderr: AsyncRead + Unpin + Send + 'static,
    {
        let mut stderr = BufReader::new(stderr);
        let mut line = String::new();
        while let Ok(n) = stderr.read_line(&mut line).await {
            if n == 0 {
                break;
            }
            if stderr_tx.send(line.clone()).await.is_err() {
                break;
            }
            line.clear();
        }
    }
}

#[async_trait]
impl Transport for SandboxedStdioTransport {
    async fn send(&self, message: String) -> Result<()> {
        Ok(self.stdout_sender.send(message).await?)
    }

    fn receive(&self) -> Pin<Box<dyn Stream<Item = String> + Send>> {
        Box::pin(self.stdin_receiver.clone())
    }

    fn receive_err(&self) -> Pin<Box<dyn Stream<Item = String> + Send>> {
        Box::pin(self.stderr_receiver.clone())
    }
}

impl Drop for SandboxedStdioTransport {
    fn drop(&mut self) {
        let _ = self.server.kill();
    }
}

/// Build the argv passed to `podman run` for a sandboxed MCP server. Pure so the
/// command-line construction can be tested without a podman binary on the host.
fn build_podman_argv(binary: &ModelContextServerBinary, sandbox: &SandboxConfig) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "run".into(),
        "-i".into(),
        "--rm".into(),
        "--runtime=runsc".into(),
        // PaddleBoard: gVisor's runsc does not support SELinux labels.
        // Without this flag, Fedora-based Podman machine VMs reject the
        // container with "SELinux is not supported".
        "--security-opt".into(),
        "label=disable".into(),
    ];

    if let Some(env) = &binary.env {
        // Iterate in a stable order so tests and `podman inspect` output are predictable.
        let mut pairs: Vec<_> = env.iter().collect();
        pairs.sort_by(|a, b| a.0.cmp(b.0));
        for (k, v) in pairs {
            args.push("-e".into());
            args.push(format!("{k}={v}"));
        }
    }

    for name in &sandbox.forward_env {
        if name.is_empty() || name.contains('=') {
            log::warn!("sandboxed_stdio: ignoring invalid forward_env name {name:?}");
            continue;
        }
        match std::env::var(name) {
            Ok(value) => {
                args.push("-e".into());
                args.push(format!("{name}={value}"));
            }
            Err(_) => {
                log::warn!(
                    "sandboxed_stdio: forward_env name {name:?} not set on host; skipping"
                );
            }
        }
    }

    if sandbox.mount_worktree
        && let Some(wd) = &sandbox.host_worktree
    {
        args.push("-v".into());
        args.push(format!("{}:/workspace", wd.display()));
        args.push("-w".into());
        args.push("/workspace".into());
    }

    args.push(sandbox.image.clone());

    args.push(binary.executable.display().to_string());
    args.extend(binary.args.iter().cloned());

    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use collections::HashMap;
    use std::path::PathBuf;

    fn make_binary(exe: &str, args: &[&str]) -> ModelContextServerBinary {
        ModelContextServerBinary {
            executable: PathBuf::from(exe),
            args: args.iter().map(|s| s.to_string()).collect(),
            env: None,
            timeout: None,
        }
    }

    fn make_sandbox(image: &str) -> SandboxConfig {
        SandboxConfig {
            image: image.to_string(),
            forward_env: Vec::new(),
            mount_worktree: false,
            host_worktree: None,
        }
    }

    #[test]
    fn minimal_invocation_has_runtime_flags_and_image_and_command() {
        let binary = make_binary("mcp-server", &["--port", "stdio"]);
        let sandbox = make_sandbox("python:3.12-slim");
        let argv = build_podman_argv(&binary, &sandbox);
        assert_eq!(
            argv,
            vec![
                "run",
                "-i",
                "--rm",
                "--runtime=runsc",
                "--security-opt",
                "label=disable",
                "python:3.12-slim",
                "mcp-server",
                "--port",
                "stdio",
            ]
        );
    }

    #[test]
    fn worktree_mount_is_emitted_when_enabled() {
        let binary = make_binary("/usr/local/bin/server", &[]);
        let sandbox = SandboxConfig {
            image: "alpine".into(),
            forward_env: Vec::new(),
            mount_worktree: true,
            host_worktree: Some(PathBuf::from("/home/user/project")),
        };
        let argv = build_podman_argv(&binary, &sandbox);
        let v_idx = argv
            .iter()
            .position(|a| a == "-v")
            .expect("expected -v flag when mount_worktree is true");
        assert_eq!(argv[v_idx + 1], "/home/user/project:/workspace");
        assert!(argv.windows(2).any(|w| w == ["-w", "/workspace"]));
    }

    #[test]
    fn worktree_mount_omitted_when_disabled_or_missing_path() {
        // Disabled even with a path
        let binary = make_binary("server", &[]);
        let sandbox = SandboxConfig {
            image: "alpine".into(),
            forward_env: Vec::new(),
            mount_worktree: false,
            host_worktree: Some(PathBuf::from("/proj")),
        };
        let argv = build_podman_argv(&binary, &sandbox);
        assert!(!argv.iter().any(|a| a == "-v"));

        // Enabled but no path
        let sandbox = SandboxConfig {
            image: "alpine".into(),
            forward_env: Vec::new(),
            mount_worktree: true,
            host_worktree: None,
        };
        let argv = build_podman_argv(&binary, &sandbox);
        assert!(!argv.iter().any(|a| a == "-v"));
    }

    #[test]
    fn explicit_command_env_is_forwarded_in_sorted_order() {
        let mut env = HashMap::default();
        env.insert("ZULU".to_string(), "z".to_string());
        env.insert("ALPHA".to_string(), "a".to_string());
        let binary = ModelContextServerBinary {
            executable: PathBuf::from("server"),
            args: vec![],
            env: Some(env),
            timeout: None,
        };
        let sandbox = make_sandbox("alpine");
        let argv = build_podman_argv(&binary, &sandbox);
        // The two -e flags should appear in alphabetical order by name.
        let positions: Vec<usize> = argv.iter().enumerate().filter_map(|(i, a)| {
            if a == "-e" { Some(i) } else { None }
        }).collect();
        assert_eq!(positions.len(), 2);
        assert_eq!(argv[positions[0] + 1], "ALPHA=a");
        assert_eq!(argv[positions[1] + 1], "ZULU=z");
    }

    #[test]
    fn forward_env_resolves_against_host_process_env() {
        // SAFETY: setting a test-local env var; no other thread reads it.
        unsafe {
            std::env::set_var("PB_SANDBOXED_MCP_TEST_FORWARD_VAR", "secret-value");
        }
        let binary = make_binary("server", &[]);
        let sandbox = SandboxConfig {
            image: "alpine".into(),
            forward_env: vec![
                "PB_SANDBOXED_MCP_TEST_FORWARD_VAR".into(),
                "PB_SANDBOXED_MCP_TEST_UNSET_VAR".into(), // skipped
                "".into(),                                  // skipped
                "INVALID=NAME".into(),                      // skipped
            ],
            mount_worktree: false,
            host_worktree: None,
        };
        let argv = build_podman_argv(&binary, &sandbox);
        let e_count = argv.iter().filter(|a| *a == "-e").count();
        assert_eq!(e_count, 1, "only the set, valid name should produce -e");
        assert!(
            argv.iter()
                .any(|a| a == "PB_SANDBOXED_MCP_TEST_FORWARD_VAR=secret-value")
        );
        // SAFETY: cleanup of test-local env var.
        unsafe {
            std::env::remove_var("PB_SANDBOXED_MCP_TEST_FORWARD_VAR");
        }
    }

    #[test]
    fn image_and_binary_appear_after_all_flags() {
        let binary = make_binary("/opt/mcp/server", &["--verbose"]);
        let sandbox = SandboxConfig {
            image: "ghcr.io/example/mcp:1.2.3".into(),
            forward_env: Vec::new(),
            mount_worktree: true,
            host_worktree: Some(PathBuf::from("/tmp/proj")),
        };
        let argv = build_podman_argv(&binary, &sandbox);
        let image_idx = argv
            .iter()
            .position(|a| a == "ghcr.io/example/mcp:1.2.3")
            .unwrap();
        // The last `-w` value comes before the image; the binary path comes after.
        assert!(image_idx > argv.iter().position(|a| a == "-w").unwrap());
        assert_eq!(argv[image_idx + 1], "/opt/mcp/server");
        assert_eq!(argv[image_idx + 2], "--verbose");
    }
}
