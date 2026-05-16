// PaddleBoard: sandboxed Jupyter kernels.
//
// The Jupyter kernel process runs inside a Podman container (with gVisor
// `runsc` when available) instead of on the host. The five ZMQ ports
// Jupyter uses (shell, iopub, stdin, control, heartbeat) are published
// 1:1 with `podman run -p N:N` so that the editor can connect to them at
// `127.0.0.1:N` exactly as if the kernel were local.
//
// PR-1 ships a single language — Python (`paddleboard/repl-python:1`). The
// image is built lazily from an inline `Containerfile` the first time the
// user launches the kernel. Subsequent launches reuse the cached image.
//
// Modeled on `wsl_kernel.rs` — the lifecycle (peek ports → write connection
// info → spawn kernel process → wait for boot → create client ZMQ sockets
// → wire up routing tasks) is identical; only the spawn step changes
// (`podman run` instead of `wsl …`).

use super::{KernelSession, RunningKernel, start_kernel_tasks};
use anyhow::{Context as _, Result};
use futures::{
    AsyncBufReadExt as _, AsyncWriteExt as _, StreamExt as _,
    channel::mpsc::{self},
    io::BufReader,
};
use gpui::{App, Entity, EntityId, Task, Window};
use jupyter_protocol::{
    ExecutionState, JupyterMessage, KernelInfoReply,
    connection_info::{ConnectionInfo, Transport},
};
use paddleboard_sandbox_prereqs::PodmanStatus;
use paddleboard_sandbox_prereqs_state::SandboxPrereqs;
use project::Fs;
use runtimelib::dirs;
use smol::net::TcpListener;
use std::{
    fmt::Debug,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use uuid::Uuid;

/// Languages PR-1 supports as sandboxed Jupyter kernels. Adding more in
/// follow-up PRs is a matter of supplying another `(image_tag, containerfile,
/// kernel_argv)` triple — see the `match` arms below.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PodmanKernelLanguage {
    Python,
}

impl PodmanKernelLanguage {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Python => "Sandboxed Python",
        }
    }

    pub fn language_id(self) -> &'static str {
        match self {
            Self::Python => "python",
        }
    }

    /// OCI image tag stored under the local registry namespace. We never
    /// push these — they're built on the user's machine the first time the
    /// kernel is launched.
    pub fn image_tag(self) -> &'static str {
        match self {
            Self::Python => "localhost/paddleboard-repl-python:1",
        }
    }

    /// Inline `Containerfile` for build-on-first-use. Kept minimal so the
    /// build is fast and reproducible — pinned base image, single `pip
    /// install`, no caching layers to worry about.
    pub fn containerfile(self) -> &'static str {
        match self {
            Self::Python => concat!(
                "FROM python:3.12-slim\n",
                "RUN pip install --no-cache-dir ipykernel==6.29.5\n",
                "WORKDIR /work\n",
                // The kernel reads ports + signing key from this file. The
                // host bind-mounts it read-only at the same path at launch.
                "CMD [\"python\", \"-m\", \"ipykernel_launcher\", \"-f\", \"/tmp/connection.json\"]\n",
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PodmanKernelSpecification {
    pub language: PodmanKernelLanguage,
}

impl PartialEq for PodmanKernelSpecification {
    fn eq(&self, other: &Self) -> bool {
        self.language == other.language
    }
}

impl Eq for PodmanKernelSpecification {}

impl PodmanKernelSpecification {
    pub fn display_name(&self) -> &'static str {
        self.language.display_name()
    }

    pub fn language_id(&self) -> &'static str {
        self.language.language_id()
    }
}

/// Every sandboxed language PR-1 knows how to launch. Used by the discovery
/// path in `repl_store.rs` to populate the kernel picker — gated on Podman
/// being reachable.
pub fn all_sandboxed_kernel_specifications() -> Vec<PodmanKernelSpecification> {
    vec![PodmanKernelSpecification {
        language: PodmanKernelLanguage::Python,
    }]
}

async fn peek_ports(ip: IpAddr) -> Result<[u16; 5]> {
    let mut addr_zeroport: SocketAddr = SocketAddr::new(ip, 0);
    addr_zeroport.set_port(0);
    let mut ports: [u16; 5] = [0; 5];
    for i in 0..5 {
        let listener = TcpListener::bind(addr_zeroport).await?;
        let addr = listener.local_addr()?;
        ports[i] = addr.port();
    }
    Ok(ports)
}

/// Build the kernel image if it isn't already present locally. Streams the
/// `Containerfile` into `podman build -f -` over stdin so we don't have to
/// manage on-disk Containerfile assets.
async fn ensure_image_built(language: PodmanKernelLanguage) -> Result<()> {
    let tag = language.image_tag();

    let exists = util::command::new_command("podman")
        .arg("image")
        .arg("exists")
        .arg(tag)
        .output()
        .await
        .context("`podman image exists` failed to execute")?;

    if exists.status.success() {
        return Ok(());
    }

    // `podman build -` reads the Containerfile from stdin with no build
    // context. Critical on macOS: passing `.` as the context tarballs the
    // entire cwd (i.e. the whole PaddleBoard repo, including
    // `target/debug/build/...`) and ships it to the Podman machine VM.
    // The cargo build tree contains symlinks pointing out to
    // `~/.cargo/git/checkouts/...` which are outside the context, so
    // podman refuses the upload with "invalid symlink". Our Containerfile
    // pulls `python:3.12-slim` and `pip install`s ipykernel — needs no
    // files from the host repo.
    let mut child = util::command::new_command("podman")
        .arg("build")
        .arg("--tag")
        .arg(tag)
        .arg("-")
        .stdin(util::command::Stdio::piped())
        .stdout(util::command::Stdio::piped())
        .stderr(util::command::Stdio::piped())
        .spawn()
        .context("failed to start `podman build` for sandboxed kernel image")?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(language.containerfile().as_bytes())
            .await
            .context("failed to write Containerfile to podman build stdin")?;
        stdin.close().await.ok();
    }

    let output = child.output().await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("podman build for {tag} failed: {stderr}");
    }
    Ok(())
}

pub struct PodmanRunningKernel {
    pub process: util::command::Child,
    container_name: String,
    connection_path: PathBuf,
    _process_status_task: Option<Task<()>>,
    pub working_directory: PathBuf,
    pub request_tx: mpsc::Sender<JupyterMessage>,
    pub stdin_tx: mpsc::Sender<JupyterMessage>,
    pub execution_state: ExecutionState,
    pub kernel_info: Option<KernelInfoReply>,
}

impl Debug for PodmanRunningKernel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PodmanRunningKernel")
            .field("container_name", &self.container_name)
            .field("process", &self.process)
            .finish()
    }
}

impl PodmanRunningKernel {
    pub fn new<S: KernelSession + 'static>(
        kernel_specification: PodmanKernelSpecification,
        entity_id: EntityId,
        working_directory: PathBuf,
        fs: Arc<dyn Fs>,
        session: Entity<S>,
        window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Box<dyn RunningKernel>>> {
        // Snapshot the prereq state on the foreground thread so the async
        // task can bail with a helpful error pointing to the sandbox-prereqs
        // install modal if Podman isn't reachable.
        //
        // gVisor preference is intentionally NOT passed via `--runtime=runsc`
        // here: Podman 5.x's remote client (which `podman` is on macOS) does
        // not expose a `--runtime` flag at all. To use gVisor the user has
        // to set it as the default runtime in the Podman machine's
        // `containers.conf` — at which point every container, including
        // ours, uses it transparently. See follow-up: surface a "Use gVisor
        // by default" toggle in the Sandbox Prerequisites modal.
        let prereq_status = SandboxPrereqs::status(cx).cloned();

        window.spawn(cx, async move |cx| {
            match prereq_status.as_ref().map(|status| &status.podman) {
                Some(PodmanStatus::Ready { .. }) => {}
                Some(PodmanStatus::Missing) => {
                    anyhow::bail!(
                        "Podman is not installed. Open the Sandbox Prerequisites \
                         status item in the status bar to install it."
                    );
                }
                Some(PodmanStatus::InstalledNotRunning { .. }) => {
                    anyhow::bail!(
                        "Podman is installed but the daemon is unreachable. On \
                         macOS, start the machine with `podman machine start`. \
                         See the Sandbox Prerequisites status item for details."
                    );
                }
                None => {
                    anyhow::bail!(
                        "Sandbox prerequisites have not been probed yet. Open \
                         the Sandbox Prerequisites status item and click Refresh."
                    );
                }
            }

            ensure_image_built(kernel_specification.language).await?;

            // The kernel binds the wildcard address inside the container; we
            // connect at `127.0.0.1:N` on the host because Podman publishes
            // each port 1:1 via `-p N:N`.
            let bind_ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
            let ports = peek_ports(bind_ip).await?;

            let connection_info = ConnectionInfo {
                transport: Transport::TCP,
                ip: "0.0.0.0".to_string(),
                stdin_port: ports[0],
                control_port: ports[1],
                hb_port: ports[2],
                shell_port: ports[3],
                iopub_port: ports[4],
                signature_scheme: "hmac-sha256".to_string(),
                key: Uuid::new_v4().to_string(),
                kernel_name: Some(format!(
                    "paddleboard-podman-{}",
                    kernel_specification.language.language_id()
                )),
            };

            let runtime_dir = dirs::runtime_dir();
            fs.create_dir(&runtime_dir).await.with_context(|| {
                format!("Failed to create jupyter runtime dir {runtime_dir:?}")
            })?;
            let connection_path =
                runtime_dir.join(format!("kernel-paddleboard-podman-{entity_id}.json"));
            let content = serde_json::to_string(&connection_info)?;
            fs.atomic_write(connection_path.clone(), content).await?;

            let container_name = format!("paddleboard-repl-{}", Uuid::new_v4());

            let mut cmd = util::command::new_command("podman");
            cmd.arg("run").arg("--rm");
            cmd.arg("--name").arg(&container_name);

            for &port in &ports {
                cmd.arg("-p").arg(format!("{port}:{port}"));
            }

            cmd.arg("-v").arg(format!(
                "{}:/tmp/connection.json:ro",
                connection_path.display()
            ));

            // The kernel needs to see the user's project to be useful (read
            // files, import local modules). Mount it read-write at `/work`
            // and `chdir` there. The container image already sets
            // `WORKDIR /work`, but we set it explicitly in case the image
            // changes underneath us.
            cmd.arg("-v")
                .arg(format!("{}:/work", working_directory.display()))
                .arg("-w")
                .arg("/work");

            cmd.arg(kernel_specification.language.image_tag());

            let mut process = cmd
                .stdout(util::command::Stdio::piped())
                .stderr(util::command::Stdio::piped())
                .stdin(util::command::Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .context("failed to start sandboxed kernel container via podman")?;

            let session_id = Uuid::new_v4().to_string();

            // The host connects to `127.0.0.1` because Podman published each
            // port 1:1 from the container's 0.0.0.0 binding.
            let mut client_connection_info = connection_info.clone();
            client_connection_info.ip = "127.0.0.1".to_string();

            // Give the kernel a moment to boot. First launches after `podman
            // build` should be quick; cold launches against a slow Podman
            // machine VM (macOS) take a couple of seconds.
            cx.background_executor().timer(Duration::from_secs(2)).await;

            match process.try_status() {
                Ok(Some(status)) => {
                    let mut stderr_content = String::new();
                    if let Some(mut stderr) = process.stderr.take() {
                        use futures::AsyncReadExt;
                        let mut buf = Vec::new();
                        if stderr.read_to_end(&mut buf).await.is_ok() {
                            stderr_content = String::from_utf8_lossy(&buf).to_string();
                        }
                    }
                    let mut stdout_content = String::new();
                    if let Some(mut stdout) = process.stdout.take() {
                        use futures::AsyncReadExt;
                        let mut buf = Vec::new();
                        if stdout.read_to_end(&mut buf).await.is_ok() {
                            stdout_content = String::from_utf8_lossy(&buf).to_string();
                        }
                    }
                    anyhow::bail!(
                        "Sandboxed kernel container exited prematurely with status: {status:?}\nstderr: {stderr_content}\nstdout: {stdout_content}"
                    );
                }
                Ok(None) => {}
                Err(_) => {}
            }

            let output_socket = runtimelib::create_client_iopub_connection(
                &client_connection_info,
                "",
                &session_id,
            )
            .await?;

            let peer_identity = runtimelib::peer_identity_for_session(&session_id)?;
            let shell_socket = runtimelib::create_client_shell_connection_with_identity(
                &client_connection_info,
                &session_id,
                peer_identity.clone(),
            )
            .await?;

            let control_socket = runtimelib::create_client_control_connection(
                &client_connection_info,
                &session_id,
            )
            .await?;

            let stdin_socket = runtimelib::create_client_stdin_connection_with_identity(
                &client_connection_info,
                &session_id,
                peer_identity,
            )
            .await?;

            let (request_tx, stdin_tx) = start_kernel_tasks(
                session.clone(),
                output_socket,
                shell_socket,
                control_socket,
                stdin_socket,
                cx,
            );

            let stderr = process.stderr.take();
            cx.spawn(async move |_cx| {
                if let Some(stderr) = stderr {
                    let reader = BufReader::new(stderr);
                    let mut lines = reader.lines();
                    while let Some(Ok(line)) = lines.next().await {
                        log::warn!("sandboxed kernel stderr: {line}");
                    }
                }
            })
            .detach();

            let stdout = process.stdout.take();
            cx.spawn(async move |_cx| {
                if let Some(stdout) = stdout {
                    let reader = BufReader::new(stdout);
                    let mut lines = reader.lines();
                    while let Some(Ok(_line)) = lines.next().await {}
                }
            })
            .detach();

            let status = process.status();
            let process_status_task = cx.spawn(async move |cx| {
                let error_message = match status.await {
                    Ok(status) => {
                        if status.success() {
                            return;
                        }
                        format!(
                            "Sandboxed kernel: container exited with status: {status:?}"
                        )
                    }
                    Err(err) => format!(
                        "Sandboxed kernel: container exited with error: {err:?}"
                    ),
                };
                session.update(cx, |session, cx| {
                    session.kernel_errored(error_message, cx);
                    cx.notify();
                });
            });

            anyhow::Ok(Box::new(Self {
                process,
                container_name,
                request_tx,
                stdin_tx,
                working_directory,
                _process_status_task: Some(process_status_task),
                connection_path,
                execution_state: ExecutionState::Idle,
                kernel_info: None,
            }) as Box<dyn RunningKernel>)
        })
    }
}

impl RunningKernel for PodmanRunningKernel {
    fn request_tx(&self) -> mpsc::Sender<JupyterMessage> {
        self.request_tx.clone()
    }

    fn stdin_tx(&self) -> mpsc::Sender<JupyterMessage> {
        self.stdin_tx.clone()
    }

    fn working_directory(&self) -> &PathBuf {
        &self.working_directory
    }

    fn execution_state(&self) -> &ExecutionState {
        &self.execution_state
    }

    fn set_execution_state(&mut self, state: ExecutionState) {
        self.execution_state = state;
    }

    fn kernel_info(&self) -> Option<&KernelInfoReply> {
        self.kernel_info.as_ref()
    }

    fn set_kernel_info(&mut self, info: KernelInfoReply) {
        self.kernel_info = Some(info);
    }

    fn force_shutdown(&mut self, _window: &mut Window, _cx: &mut App) -> Task<anyhow::Result<()>> {
        self._process_status_task.take();
        self.request_tx.close_channel();
        // `podman run --rm` removes the container on exit; killing the host
        // process is enough.
        self.process.kill().ok();
        Task::ready(Ok(()))
    }

    fn kill(&mut self) {
        self._process_status_task.take();
        self.request_tx.close_channel();
        self.process.kill().ok();
    }
}

impl Drop for PodmanRunningKernel {
    fn drop(&mut self) {
        // Belt and braces: `podman run --rm` handles cleanup on a clean exit,
        // but if the editor crashed mid-session we explicitly remove the
        // container so it doesn't linger.
        std::process::Command::new("podman")
            .args(["rm", "-f", &self.container_name])
            .output()
            .ok();
        std::fs::remove_file(&self.connection_path).ok();
        self.request_tx.close_channel();
        self.process.kill().ok();
    }
}
