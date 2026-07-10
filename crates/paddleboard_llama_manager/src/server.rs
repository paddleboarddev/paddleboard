use anyhow::{Context as _, Result};
use std::ffi::OsString;
use std::net::{Ipv4Addr, TcpListener};
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};
use util::process::Child;

/// How long to wait for a freshly spawned server to answer `/health` before
/// giving up. A cold model load can take a while on a constrained machine.
pub const READINESS_TIMEOUT: Duration = Duration::from_secs(180);

/// How often to re-check readiness while waiting.
pub const READINESS_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// A running managed `llama-server` process bound to a loopback port. Killing it
/// (explicitly or on drop) tears down the whole process group.
///
/// On Unix the handle also holds the write end of the spawned watchdog's stdin
/// pipe (inside `child`), which is what ties the server's lifetime to the app:
/// see [`WATCHDOG_SCRIPT`].
pub struct ServerHandle {
    child: Child,
}

/// Ties a spawned `llama-server` to the lifetime of the PaddleBoard process at
/// the OS level. `util::process::Child` detaches Unix children into their own
/// session (`setsid`), so without this a server outlives the app whenever Rust
/// destructors don't run — crash, force-quit, SIGKILL, or an `exit()` that
/// skips dropping globals — which is exactly how orphaned servers were observed
/// parented to launchd.
///
/// The mechanism is a pipe, not polling: the wrapper starts the real server in
/// the background, then blocks `read`ing its stdin, which is a pipe whose only
/// write end lives in this app (held open by `ServerHandle`). The app never
/// writes to it, so `read` returns only at EOF — and the kernel delivers that
/// EOF the moment the app exits *for any reason*, at which point the wrapper
/// kills the server and exits. `wait` ensures the wrapper only exits after the
/// server is truly gone. Explicit kills are unaffected: the wrapper is the
/// session/group leader, so `killpg` in `ServerHandle::kill` takes down both.
#[cfg(unix)]
const WATCHDOG_SCRIPT: &str = r#"
"$0" "$@" &
server=$!
read -r _
kill -9 "$server" 2>/dev/null
wait "$server" 2>/dev/null
"#;

#[cfg(unix)]
fn server_command(binary: &Path, args: Vec<OsString>) -> std::process::Command {
    let mut command = std::process::Command::new("/bin/sh");
    command.arg("-c").arg(WATCHDOG_SCRIPT).arg(binary);
    command.args(args);
    command
}

#[cfg(not(unix))]
fn server_command(binary: &Path, args: Vec<OsString>) -> std::process::Command {
    // Windows needs no wrapper: `util::process::Child` assigns the child to a
    // job object that the OS terminates when the app's handles close, so the
    // server can't outlive the app even on hard kills.
    let mut command = std::process::Command::new(binary);
    command.args(args);
    command
}

impl ServerHandle {
    pub fn kill(&mut self) {
        if let Err(error) = self.child.kill() {
            log::warn!("failed to kill managed llama-server: {error:?}");
        }
    }
}

#[cfg(test)]
impl ServerHandle {
    /// Wrap an arbitrary spawned command so tests can exercise the
    /// clear-and-kill-on-failure behavior without a real `llama-server`.
    pub(crate) fn spawn_for_test(command: std::process::Command) -> Result<Self> {
        let child = Child::spawn(command, Stdio::null(), Stdio::null(), Stdio::null())
            .context("spawning test process")?;
        Ok(Self { child })
    }

    /// The OS pid of the wrapped process, for asserting it was killed.
    pub(crate) fn pid(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        // Unix `smol::process::Child` does not kill on drop, so do it explicitly;
        // otherwise a model-switch or app quit would orphan the server.
        self.kill();
    }
}

/// Ask the OS for an unused loopback TCP port by binding port 0 and reading back
/// the assigned port. The listener is dropped immediately so `llama-server` can
/// claim it. There is a small TOCTOU window before the server binds; if another
/// process steals the port, the server fails to start and the error surfaces
/// through the readiness poll.
pub fn find_free_port() -> Result<u16> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .context("binding to an ephemeral loopback port")?;
    let port = listener
        .local_addr()
        .context("reading the ephemeral port")?
        .port();
    Ok(port)
}

/// The argv for a managed `llama-server`, shared by the chat and embedding
/// spawns so their common security posture (loopback-only, never `--rpc`) can't
/// drift between the two paths.
fn server_args(model: &Path, port: u16, context_size: u32, embeddings: bool) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string().into(),
        "--model".into(),
        model.as_os_str().to_owned(),
        "--ctx-size".into(),
        context_size.to_string().into(),
    ];
    if embeddings {
        // Serve /v1/embeddings. Pooling is taken from the GGUF metadata (our
        // pinned embedding model declares mean pooling), so it is not overridden
        // here.
        args.push("--embeddings".into());
    }
    args
}

fn spawn_with_args(binary: &Path, args: Vec<OsString>) -> Result<ServerHandle> {
    // Stdin must be a pipe (not null) for the Unix watchdog: its write end is
    // retained inside `child`, and its closure on app exit is the kill signal.
    let child = Child::spawn(
        server_command(binary, args),
        Stdio::piped(),
        Stdio::inherit(),
        Stdio::inherit(),
    )
    .context("spawning llama-server")?;
    Ok(ServerHandle { child })
}

/// Spawns `llama-server` bound to `127.0.0.1:{port}` serving `model`.
///
/// The server is intentionally bound to loopback only and never receives the
/// `--rpc` flag (its known-dangerous remote surface). Metal is selected
/// automatically on macOS, so no GPU flags are needed in phase 1.
pub fn spawn_llama_server(
    binary: &Path,
    model: &Path,
    port: u16,
    context_size: u32,
) -> Result<ServerHandle> {
    spawn_with_args(binary, server_args(model, port, context_size, false))
}

/// Spawns `llama-server` in embeddings mode (`--embeddings`), serving the
/// OpenAI-compatible `/v1/embeddings` endpoint on loopback.
pub fn spawn_llama_embedding_server(
    binary: &Path,
    model: &Path,
    port: u16,
    context_size: u32,
) -> Result<ServerHandle> {
    spawn_with_args(binary, server_args(model, port, context_size, true))
}

/// Repeatedly calls `is_ready` until it resolves `true` or `timeout` elapses,
/// awaiting `sleep()` between attempts. The first check runs immediately.
///
/// The delay is injected rather than using a timer directly so production can
/// schedule it on the GPUI background executor (the project disallows
/// `smol::Timer::after`) while tests can supply an instant sleeper.
pub async fn poll_until_ready<Check, CheckFut, Sleep, SleepFut>(
    timeout: Duration,
    mut is_ready: Check,
    mut sleep: Sleep,
) -> Result<()>
where
    Check: FnMut() -> CheckFut,
    CheckFut: std::future::Future<Output = bool>,
    Sleep: FnMut() -> SleepFut,
    SleepFut: std::future::Future<Output = ()>,
{
    let start = Instant::now();
    loop {
        if is_ready().await {
            return Ok(());
        }
        if start.elapsed() >= timeout {
            anyhow::bail!(
                "timed out after {:?} waiting for the local model server to become ready",
                timeout
            );
        }
        sleep().await;
    }
}

/// Whether `llama-server` reports itself ready. Its `/health` endpoint returns
/// 200 once the model is loaded and 503 while it is still loading.
pub async fn health_ready(http_client: &dyn http_client::HttpClient, port: u16) -> bool {
    let url = format!("http://127.0.0.1:{port}/health");
    match http_client.get(&url, Default::default(), false).await {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn embedding_args_add_only_the_embeddings_flag() {
        let model = Path::new("/tmp/m.gguf");
        let chat = server_args(model, 8080, 4096, false);
        let embed = server_args(model, 8080, 2048, true);
        assert!(!chat.iter().any(|a| a == "--embeddings"));
        assert_eq!(embed.last().unwrap(), "--embeddings");
        // Both stay loopback-only and never enable the RPC surface.
        for args in [&chat, &embed] {
            assert!(args.iter().any(|a| a == "127.0.0.1"));
            assert!(!args.iter().any(|a| a == "--rpc"));
            assert!(!args.iter().any(|a| a == "--pooling"));
        }
    }

    // The core orphan fix: if the app dies without running destructors
    // (crash, force-quit, SIGKILL), the watchdog must kill the server. Closing
    // the app-side write end of the stdin pipe is exactly what the kernel does
    // on app death, so taking it out of the handle simulates that.
    #[cfg(unix)]
    #[test]
    fn watchdog_kills_the_server_when_the_app_side_of_the_pipe_closes() {
        // A long sleep stands in for llama-server, spawned through the real
        // watchdog wrapper.
        let mut handle = spawn_with_args(Path::new("/bin/sleep"), vec!["300".into()]).unwrap();

        drop(handle.child.stdin.take());

        // The watchdog `wait`s for the server before exiting, so the wrapper
        // reaching an exit status proves the server died too.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match handle.child.try_status() {
                Ok(Some(_)) => break,
                Ok(None) => {}
                Err(error) => panic!("polling the watchdog's status failed: {error:?}"),
            }
            assert!(
                Instant::now() < deadline,
                "the watchdog should kill the server and exit once the pipe closes"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn find_free_port_returns_a_bindable_port() {
        let port = find_free_port().unwrap();
        assert_ne!(port, 0);
        // The port was released, so we can bind it again right away.
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).unwrap();
        assert_eq!(listener.local_addr().unwrap().port(), port);
    }

    #[test]
    fn poll_succeeds_when_port_opens_late() {
        futures::executor::block_on(async {
            let attempts = Cell::new(0u32);
            // Simulates a server that only starts answering on the 3rd poll.
            poll_until_ready(
                Duration::from_secs(5),
                || {
                    let count = attempts.get() + 1;
                    attempts.set(count);
                    async move { count >= 3 }
                },
                || std::future::ready(()),
            )
            .await
            .unwrap();
            assert_eq!(attempts.get(), 3);
        });
    }

    #[test]
    fn poll_times_out_when_never_ready() {
        futures::executor::block_on(async {
            let attempts = Cell::new(0u32);
            let error = poll_until_ready(
                Duration::from_millis(20),
                || {
                    attempts.set(attempts.get() + 1);
                    async { false }
                },
                || std::future::ready(()),
            )
            .await
            .unwrap_err();
            assert!(error.to_string().contains("timed out"));
            assert!(attempts.get() >= 1, "should have polled at least once");
        });
    }
}
