use gpui::{App, AppContext as _, Global, SharedString, TaskExt};
use std::sync::Arc;
use util::command::new_command;

#[derive(Clone, Debug)]
pub struct ForwardedPort {
    pub label: SharedString,
    pub host_port: u16,
    pub container_id: Arc<str>,
}

impl ForwardedPort {
    pub fn url(&self) -> String {
        format!("http://localhost:{}", self.host_port)
    }
}

#[derive(Default)]
pub struct ForwardedPorts {
    ports: Vec<ForwardedPort>,
}

impl Global for ForwardedPorts {}

impl ForwardedPorts {
    pub fn try_global(cx: &App) -> Option<&ForwardedPorts> {
        cx.try_global::<ForwardedPorts>()
    }

    pub fn ports(&self) -> &[ForwardedPort] {
        &self.ports
    }

    pub fn register(cx: &mut App, port: ForwardedPort) {
        let registry = cx.default_global::<ForwardedPorts>();
        if let Some(existing) = registry
            .ports
            .iter_mut()
            .find(|p| p.host_port == port.host_port)
        {
            *existing = port;
        } else {
            registry.ports.push(port);
        }
    }

    /// Remove a port from the registry and best-effort `podman stop` its container.
    /// Errors from `podman stop` are logged but not surfaced — the registry update is what
    /// drives the UI; container teardown is best-effort.
    pub fn stop(cx: &mut App, host_port: u16) {
        let removed = {
            let registry = cx.default_global::<ForwardedPorts>();
            let idx = registry
                .ports
                .iter()
                .position(|p| p.host_port == host_port);
            idx.map(|i| registry.ports.remove(i))
        };

        let Some(port) = removed else { return };
        let container_id = port.container_id.to_string();
        cx.background_spawn(async move {
            let mut cmd = new_command("podman");
            cmd.args(["stop", &container_id]);
            cmd.output()
                .await
                .map_err(|e| anyhow::anyhow!("podman stop failed: {e}"))
        })
        .detach_and_log_err(cx);
    }
}

pub fn init(cx: &mut App) {
    cx.set_global(ForwardedPorts::default());
}
