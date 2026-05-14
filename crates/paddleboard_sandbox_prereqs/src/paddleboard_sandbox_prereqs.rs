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
    let has_runsc = value
        .get("host")
        .and_then(|h| h.get("ociRuntimes"))
        .and_then(|r| r.as_object())
        .is_some_and(|map| map.contains_key("runsc"));

    if has_runsc {
        GvisorStatus::Available
    } else {
        GvisorStatus::NotConfigured
    }
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
}

/// Hand-curated guidance for getting the sandbox stack to a working state.
/// `os` is taken as a parameter so tests can exercise each branch deterministically.
pub fn install_instructions(status: &SandboxStatus, os: Os) -> InstallInstructions {
    if status.is_satisfied() {
        return InstallInstructions {
            title: "Sandbox prerequisites are satisfied".to_string(),
            steps: vec![InstallStep {
                description: "Podman is installed and reachable, and gVisor (runsc) is registered. PaddleBoard's sandboxed tools are ready to use.".to_string(),
                command: None,
            }],
            doc_url: None,
        };
    }

    let mut steps = Vec::new();
    let mut title = "Install sandbox prerequisites".to_string();

    match (&status.podman, os) {
        (PodmanStatus::Missing, Os::MacOs) => {
            title = "Install Podman".to_string();
            steps.push(InstallStep {
                description: "Install Podman via Homebrew.".to_string(),
                command: Some("brew install podman".to_string()),
            });
            steps.push(InstallStep {
                description: "Initialize a Podman machine. Podman on macOS runs containers inside a small Linux VM; this command creates it.".to_string(),
                command: Some("podman machine init".to_string()),
            });
            steps.push(InstallStep {
                description: "Start the Podman machine.".to_string(),
                command: Some("podman machine start".to_string()),
            });
        }
        (PodmanStatus::Missing, Os::Linux) => {
            title = "Install Podman".to_string();
            steps.push(InstallStep {
                description: "Install Podman via your distribution's package manager.".to_string(),
                command: Some(linux_podman_install_command()),
            });
        }
        (PodmanStatus::Missing, Os::Windows) => {
            title = "Install Podman".to_string();
            steps.push(InstallStep {
                description: "Install Podman Desktop from podman-desktop.io. Note: gVisor is Linux-only, so the strongest sandboxing tier isn't available on Windows even with Podman installed.".to_string(),
                command: None,
            });
        }
        (PodmanStatus::Missing, Os::Other) => {
            title = "Install Podman".to_string();
            steps.push(InstallStep {
                description: "PaddleBoard's sandbox stack hasn't been tested on this platform. See Podman's documentation at podman.io for installation guidance.".to_string(),
                command: None,
            });
        }
        (PodmanStatus::InstalledNotRunning { .. }, Os::MacOs) => {
            title = "Start the Podman machine".to_string();
            steps.push(InstallStep {
                description: "Podman is installed but its backing Linux VM (the \"machine\") isn't running. Start it:".to_string(),
                command: Some("podman machine start".to_string()),
            });
            steps.push(InstallStep {
                description: "If no machine exists yet, create one first.".to_string(),
                command: Some("podman machine init".to_string()),
            });
        }
        (PodmanStatus::InstalledNotRunning { .. }, _) => {
            title = "Start the Podman service".to_string();
            steps.push(InstallStep {
                description: "Podman is installed but not reachable. Verify the Podman service is running and your user has permission to talk to it.".to_string(),
                command: None,
            });
        }
        (PodmanStatus::Ready { .. }, _) => {}
    }

    // Only surface gVisor steps once Podman itself is ready — running them
    // earlier would just confuse a user who hasn't even installed Podman.
    if matches!(status.podman, PodmanStatus::Ready { .. })
        && matches!(status.gvisor, GvisorStatus::NotConfigured)
    {
        title = "Install gVisor and register it with Podman".to_string();
        match os {
            Os::Linux => {
                steps.push(InstallStep {
                    description: "Download and install runsc (gVisor's OCI runtime). Requires sudo.".to_string(),
                    command: Some(
                        "curl -fsSL https://storage.googleapis.com/gvisor/releases/release/latest/$(uname -m)/runsc -o runsc \\\n  && chmod +x runsc \\\n  && sudo mv runsc /usr/local/bin/runsc"
                            .to_string(),
                    ),
                });
                steps.push(InstallStep {
                    description: "Register runsc as a Podman runtime by adding the following to ~/.config/containers/containers.conf (create the file if needed):".to_string(),
                    command: Some("[engine.runtimes]\nrunsc = [\"/usr/local/bin/runsc\"]".to_string()),
                });
            }
            Os::MacOs => {
                steps.push(InstallStep {
                    description: "On macOS, Podman runs containers inside a Linux VM, so gVisor must be installed inside that VM. SSH into the Podman machine:".to_string(),
                    command: Some("podman machine ssh".to_string()),
                });
                steps.push(InstallStep {
                    description: "Inside the VM, run the same Linux install steps (download runsc + register it in containers.conf). See the linked docs.".to_string(),
                    command: None,
                });
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
