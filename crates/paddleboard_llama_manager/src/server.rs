use anyhow::{Context as _, Result};
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
pub struct ServerHandle {
    child: Child,
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
    let mut command = std::process::Command::new(binary);
    command
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .arg("--model")
        .arg(model)
        .arg("--ctx-size")
        .arg(context_size.to_string());

    let child = Child::spawn(command, Stdio::null(), Stdio::inherit(), Stdio::inherit())
        .context("spawning llama-server")?;
    Ok(ServerHandle { child })
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
