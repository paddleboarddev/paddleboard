//! Detect whether PaddleBoard's sandbox prerequisites (Podman + gVisor's `runsc`)
//! are installed and reachable, and produce human-readable install guidance when
//! they aren't. This is the data layer; UI surfaces (CLI flag, status indicator,
//! modal) live elsewhere.

use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxStatus {
    pub podman: PodmanStatus,
    pub gvisor: GvisorStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PodmanStatus {
    /// `podman` is not on `$PATH`.
    Missing,
    /// `podman --version` succeeds but `podman info` does not. On macOS this
    /// typically means the backing `podman machine` is not started.
    InstalledNotRunning { version: String },
    /// `podman info` succeeds — the daemon is reachable.
    Ready { version: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GvisorStatus {
    /// `runsc` appears in Podman's registered OCI runtimes.
    Available,
    /// Podman is reachable but `runsc` is not registered as a runtime.
    NotConfigured,
    /// gVisor cannot run on this platform (e.g. Windows).
    NotApplicable { reason: &'static str },
    /// We couldn't determine gVisor's status because Podman itself isn't ready.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Linux,
    Windows,
    Other,
}

impl Os {
    pub fn detect() -> Os {
        match std::env::consts::OS {
            "macos" => Os::MacOs,
            "linux" => Os::Linux,
            "windows" => Os::Windows,
            _ => Os::Other,
        }
    }
}

impl SandboxStatus {
    /// True when both Podman is reachable and gVisor is registered (or
    /// inherently unavailable on this platform but Podman is fine).
    pub fn is_satisfied(&self) -> bool {
        matches!(self.podman, PodmanStatus::Ready { .. })
            && matches!(
                self.gvisor,
                GvisorStatus::Available | GvisorStatus::NotApplicable { .. }
            )
    }
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// Probe the host for Podman + gVisor availability. Each probe is bounded by
/// a 2-second timeout so a stuck `podman machine` cannot stall the caller.
pub async fn check() -> SandboxStatus {
    let podman = check_podman().await;
    let gvisor = check_gvisor(&podman).await;
    SandboxStatus { podman, gvisor }
}

async fn check_podman() -> PodmanStatus {
    let version = match run_probe("podman", &["--version"]).await {
        Some(stdout) => stdout.trim().to_string(),
        None => return PodmanStatus::Missing,
    };

    // `podman info` reaches the daemon. On macOS the CLI can be installed while
    // the backing machine is stopped, which is why we treat that case separately.
    match run_probe("podman", &["info", "--format", "json"]).await {
        Some(_) => PodmanStatus::Ready { version },
        None => PodmanStatus::InstalledNotRunning { version },
    }
}

async fn check_gvisor(podman: &PodmanStatus) -> GvisorStatus {
    if Os::detect() == Os::Windows {
        return GvisorStatus::NotApplicable {
            reason: "gVisor only runs on Linux; it is not available on Windows.",
        };
    }

    let info_json = match podman {
        PodmanStatus::Ready { .. } => match run_probe("podman", &["info", "--format", "json"]).await
        {
            Some(out) => out,
            None => return GvisorStatus::Unknown,
        },
        _ => return GvisorStatus::Unknown,
    };

    // Use untyped JSON navigation: Podman's info schema has shifted across
    // versions and field names like `ociRuntimes` may not survive a future
    // bump. Looking up by string key keeps us resilient to renames of
    // siblings.
    let value: serde_json::Value = match serde_json::from_str(&info_json) {
        Ok(v) => v,
        Err(_) => return GvisorStatus::NotConfigured,
    };
    let has_runsc_in_info = value
        .get("host")
        .and_then(|h| h.get("ociRuntimes"))
        .and_then(|r| r.as_object())
        .is_some_and(|map| map.contains_key("runsc"));

    if has_runsc_in_info {
        return GvisorStatus::Available;
    }

    // PaddleBoard: on macOS, `podman info` runs on the client and doesn't
    // reflect runtimes registered inside the Podman machine VM. Fall back to
    // probing runsc directly inside the VM.
    if run_probe("podman", &["machine", "ssh", "--", "runsc", "--version"])
        .await
        .is_some_and(|out| out.contains("runsc version"))
    {
        return GvisorStatus::Available;
    }

    GvisorStatus::NotConfigured
}

async fn run_probe(cmd: &str, args: &[&str]) -> Option<String> {
    let fut = tokio::process::Command::new(cmd).args(args).output();
    let output = tokio::time::timeout(PROBE_TIMEOUT, fut).await.ok()?.ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[derive(Debug, Clone)]
pub struct InstallInstructions {
    pub title: String,
    pub steps: Vec<InstallStep>,
    pub doc_url: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct InstallStep {
    pub description: String,
    pub command: Option<String>,
    pub command_kind: CommandKind,
}

/// Distinguishes runnable shell commands from text snippets that the user
/// needs to paste into a config file. The UI uses this to decide whether to
/// expose a "Run in Terminal" affordance — pasting a TOML fragment into a
/// shell would just produce errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CommandKind {
    #[default]
    Shell,
    Snippet,
}

impl InstallStep {
    fn note(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            command: None,
            command_kind: CommandKind::Shell,
        }
    }

    fn shell(description: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            command: Some(command.into()),
            command_kind: CommandKind::Shell,
        }
    }

    fn snippet(description: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            command: Some(command.into()),
            command_kind: CommandKind::Snippet,
        }
    }
}

/// Hand-curated guidance for getting the sandbox stack to a working state.
/// `os` is taken as a parameter so tests can exercise each branch deterministically.
pub fn install_instructions(status: &SandboxStatus, os: Os) -> InstallInstructions {
    if status.is_satisfied() {
        return InstallInstructions {
            title: "Sandbox prerequisites are satisfied".to_string(),
            steps: vec![InstallStep::note(
                "Podman is installed and reachable, and gVisor (runsc) is registered. PaddleBoard's sandboxed tools are ready to use.",
            )],
            doc_url: None,
        };
    }

    let mut steps = Vec::new();
    let mut title = "Install sandbox prerequisites".to_string();

    match (&status.podman, os) {
        (PodmanStatus::Missing, Os::MacOs) => {
            title = "Install Podman on macOS".to_string();
            steps.push(InstallStep::note(
                "PaddleBoard runs sandboxed tools (like the Python REPL and the agent's shell) inside Podman containers. On macOS, Podman runs containers inside a small Linux VM, called a \"machine\".",
            ));
            steps.push(InstallStep::note(
                "If you don't have Homebrew yet, install it first from https://brew.sh — then come back to this dialog.",
            ));
            steps.push(InstallStep::shell(
                "Install the Podman CLI via Homebrew (about 30 seconds).",
                "brew install podman",
            ));
            steps.push(InstallStep::shell(
                "Create the Linux VM that Podman will use. The defaults are fine; the VM uses roughly 2 GB of disk.",
                "podman machine init",
            ));
            steps.push(InstallStep::shell(
                "Start the VM. The first boot takes 15–30 seconds.",
                "podman machine start",
            ));
            steps.push(InstallStep::note(
                "Click Refresh below to confirm PaddleBoard now sees the running Podman daemon.",
            ));
        }
        (PodmanStatus::Missing, Os::Linux) => {
            title = "Install Podman on Linux".to_string();
            steps.push(InstallStep::note(
                "PaddleBoard runs sandboxed tools (like the Python REPL and the agent's shell) inside Podman containers.",
            ));
            steps.push(InstallStep::shell(
                "Install Podman via your distribution's package manager. PaddleBoard picked the command below based on your /etc/os-release.",
                linux_podman_install_command(),
            ));
            steps.push(InstallStep::shell(
                "Confirm the daemon responds. Most distributions need no extra setup — Podman runs rootless out of the box.",
                "podman info",
            ));
            steps.push(InstallStep::note(
                "Click Refresh below to verify PaddleBoard sees Podman.",
            ));
        }
        (PodmanStatus::Missing, Os::Windows) => {
            title = "Install Podman on Windows".to_string();
            steps.push(InstallStep::note(
                "Download and install Podman Desktop from https://podman-desktop.io. After installation, start Podman Desktop at least once so it provisions its WSL backend.",
            ));
            steps.push(InstallStep::note(
                "Note: gVisor is Linux-only, so the strongest sandboxing tier is not available on Windows even with Podman installed. PaddleBoard will fall back to Podman's default runtime.",
            ));
            steps.push(InstallStep::note(
                "Click Refresh below to verify.",
            ));
        }
        (PodmanStatus::Missing, Os::Other) => {
            title = "Install Podman".to_string();
            steps.push(InstallStep::note(
                "PaddleBoard's sandbox stack hasn't been tested on this platform. See https://podman.io/docs/installation for installation guidance for your OS.",
            ));
        }
        (PodmanStatus::InstalledNotRunning { .. }, Os::MacOs) => {
            title = "Start the Podman machine".to_string();
            steps.push(InstallStep::note(
                "Podman is installed but its backing Linux VM (the \"machine\") isn't running. The CLI is reachable; the daemon inside the VM is not.",
            ));
            steps.push(InstallStep::shell(
                "Start the machine. This takes 5–15 seconds.",
                "podman machine start",
            ));
            steps.push(InstallStep::note(
                "If the command above prints \"no machines found\", you need to create one first:",
            ));
            steps.push(InstallStep::shell(
                "Create the Linux VM that Podman will use.",
                "podman machine init",
            ));
            steps.push(InstallStep::note(
                "Click Refresh below once the machine reports as running.",
            ));
        }
        (PodmanStatus::InstalledNotRunning { .. }, Os::Linux) => {
            title = "Start the Podman service".to_string();
            steps.push(InstallStep::note(
                "Podman is installed but the daemon is unreachable. On Linux this usually means the user's Podman socket isn't running.",
            ));
            steps.push(InstallStep::shell(
                "Start (and enable) the rootless Podman socket via systemd.",
                "systemctl --user enable --now podman.socket",
            ));
            steps.push(InstallStep::shell(
                "Verify the daemon is reachable.",
                "podman info",
            ));
            steps.push(InstallStep::note(
                "Click Refresh below to verify.",
            ));
        }
        (PodmanStatus::InstalledNotRunning { .. }, _) => {
            title = "Start the Podman service".to_string();
            steps.push(InstallStep::note(
                "Podman is installed but not reachable. Verify the Podman service is running and your user has permission to talk to it, then click Refresh below.",
            ));
        }
        (PodmanStatus::Ready { .. }, _) => {}
    }

    // Only surface gVisor steps once Podman itself is ready — running them
    // earlier would just confuse a user who hasn't even installed Podman.
    if matches!(status.podman, PodmanStatus::Ready { .. })
        && matches!(status.gvisor, GvisorStatus::NotConfigured)
    {
        title = "Install gVisor (runsc) and register it with Podman".to_string();
        match os {
            Os::Linux => {
                steps.push(InstallStep::note(
                    "gVisor is a user-space kernel that provides stronger isolation than Podman's default runtime. PaddleBoard prefers it when available, and falls back to the default runtime when it isn't.",
                ));
                steps.push(InstallStep::shell(
                    "Download the latest runsc binary for your CPU architecture and install it to /usr/local/bin. Requires sudo.",
                    "curl -fsSL https://storage.googleapis.com/gvisor/releases/release/latest/$(uname -m)/runsc -o runsc \\\n  && chmod +x runsc \\\n  && sudo mv runsc /usr/local/bin/runsc",
                ));
                steps.push(InstallStep::snippet(
                    "Register runsc as a Podman runtime. Add the snippet below to ~/.config/containers/containers.conf (create the file if it doesn't exist):",
                    "[engine.runtimes]\nrunsc = [\"/usr/local/bin/runsc\"]",
                ));
                steps.push(InstallStep::note(
                    "Click Refresh below to verify runsc is registered.",
                ));
            }
            Os::MacOs => {
                steps.push(InstallStep::note(
                    "On macOS, containers run inside the Podman machine VM, so gVisor must be installed inside that VM — not on macOS itself. The next steps move you into the VM.",
                ));
                steps.push(InstallStep::shell(
                    "SSH into the Podman machine. This opens an interactive shell inside the VM.",
                    "podman machine ssh",
                ));
                steps.push(InstallStep::snippet(
                    "Inside the VM, download runsc to /usr/local/bin. (The VM is Fedora-based and requires sudo. Paste this into the SSH session.)",
                    "sudo curl -fsSL https://storage.googleapis.com/gvisor/releases/release/latest/$(uname -m)/runsc -o /usr/local/bin/runsc \\\n  && sudo chmod +x /usr/local/bin/runsc",
                ));
                steps.push(InstallStep::snippet(
                    "Still inside the VM, register runsc as a Podman runtime. Append the snippet below to /etc/containers/containers.conf (use sudo):",
                    "[engine.runtimes]\nrunsc = [\"/usr/local/bin/runsc\"]",
                ));
                steps.push(InstallStep::note(
                    "Type `exit` to leave the VM, then click Refresh below to verify.",
                ));
            }
            _ => {}
        }
    }

    InstallInstructions {
        title,
        steps,
        doc_url: Some("https://gvisor.dev/docs/user_guide/install/"),
    }
}

fn linux_podman_install_command() -> String {
    let id = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|contents| {
            contents.lines().find_map(|line| {
                line.strip_prefix("ID=")
                    .map(|value| value.trim_matches('"').to_string())
            })
        });
    match id.as_deref() {
        Some("ubuntu") | Some("debian") | Some("pop") | Some("linuxmint") => {
            "sudo apt update && sudo apt install -y podman".to_string()
        }
        Some("fedora") | Some("rhel") | Some("centos") | Some("rocky") | Some("alma") => {
            "sudo dnf install -y podman".to_string()
        }
        Some("arch") | Some("manjaro") | Some("endeavouros") => {
            "sudo pacman -S --noconfirm podman".to_string()
        }
        Some("opensuse-leap") | Some("opensuse-tumbleweed") | Some("sles") => {
            "sudo zypper install -y podman".to_string()
        }
        _ => "Use your distribution's package manager to install podman.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_satisfied_requires_both_podman_and_gvisor() {
        let happy = SandboxStatus {
            podman: PodmanStatus::Ready {
                version: "podman version 5.0.0".into(),
            },
            gvisor: GvisorStatus::Available,
        };
        assert!(happy.is_satisfied());

        let no_podman = SandboxStatus {
            podman: PodmanStatus::Missing,
            gvisor: GvisorStatus::Available,
        };
        assert!(!no_podman.is_satisfied());

        let no_gvisor = SandboxStatus {
            podman: PodmanStatus::Ready {
                version: "podman version 5.0.0".into(),
            },
            gvisor: GvisorStatus::NotConfigured,
        };
        assert!(!no_gvisor.is_satisfied());

        // Windows: NotApplicable still counts as satisfied if podman is ready,
        // because the user has done everything they can on this platform.
        let windows_ready = SandboxStatus {
            podman: PodmanStatus::Ready {
                version: "podman version 5.0.0".into(),
            },
            gvisor: GvisorStatus::NotApplicable {
                reason: "gVisor only runs on Linux; it is not available on Windows.",
            },
        };
        assert!(windows_ready.is_satisfied());
    }

    #[test]
    fn missing_podman_gives_install_steps() {
        let status = SandboxStatus {
            podman: PodmanStatus::Missing,
            gvisor: GvisorStatus::Unknown,
        };
        let instructions = install_instructions(&status, Os::MacOs);
        assert!(instructions.title.contains("Podman"));
        assert!(instructions.steps.iter().any(|step| step
            .command
            .as_deref()
            .is_some_and(|cmd| cmd.contains("brew install podman"))));
    }

    #[test]
    fn satisfied_status_returns_short_confirmation() {
        let status = SandboxStatus {
            podman: PodmanStatus::Ready {
                version: "podman version 5.0.0".into(),
            },
            gvisor: GvisorStatus::Available,
        };
        let instructions = install_instructions(&status, Os::Linux);
        assert_eq!(instructions.steps.len(), 1);
        assert!(instructions.title.contains("satisfied"));
    }
}
