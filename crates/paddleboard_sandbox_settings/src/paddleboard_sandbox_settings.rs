//! Settings and policy logic for PaddleBoard's sandbox enforcement.
//!
//! When a sandboxed tool (one-shot exec, long-lived service, MCP stdio
//! transport) is about to spawn a podman container, it consults
//! [`decide_gate`] with the cached sandbox-prereqs status and the user's
//! [`SandboxSettings`]. The returned [`SandboxGateDecision`] tells the call
//! site whether to proceed sandboxed, fall back to running on the host, surface
//! a one-shot warning, or block entirely (so the agent gets a clear error and
//! the user sees the install modal).

use std::sync::atomic::{AtomicBool, Ordering};

use gpui::App;
use serde::Deserialize;
use settings::{RegisterSetting, Settings};

pub use paddleboard_sandbox_prereqs::SandboxStatus;

/// Force-link the crate so the `RegisterSetting` inventory entry for
/// [`SandboxSettings`] is reachable. The derive emits an `inventory::submit!`
/// block, but the linker can drop a crate that has no externally-visible call
/// sites — calling this function from PaddleBoard's main keeps the symbol
/// alive.
pub fn init(_cx: &mut App) {}

static WARN_ONCE_SHOWN: AtomicBool = AtomicBool::new(false);

/// Returns `true` the first time it is called in a process, and `false`
/// thereafter. Used by the `warn_once` policy to ensure callers only emit a
/// notification once per session.
pub fn claim_warn_once_slot() -> bool {
    !WARN_ONCE_SHOWN.swap(true, Ordering::SeqCst)
}


/// Policy for what to do when [`SandboxStatus::is_satisfied`] is false at the
/// point a sandboxed tool tries to spawn.
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnMissingRuntime {
    /// Refuse to launch and surface the install modal. Keeps the sandbox
    /// guarantee honest; agent receives a clear error.
    #[default]
    Block,
    /// Bypass the container and run the command directly on the host. Escape
    /// hatch for environments where the sandbox stack is genuinely unavailable
    /// (Windows, CI) or the user has accepted the risk.
    FallBackToHost,
    /// Proceed sandboxed but emit a one-shot notification with install
    /// guidance. Useful while users are still onboarding.
    WarnOnce,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, RegisterSetting)]
pub struct SandboxSettings {
    /// Policy applied when prereqs are missing at tool-launch time.
    pub on_missing_runtime: OnMissingRuntime,
    /// Whether to probe the host for sandbox prereqs at all. When `false`,
    /// `decide_gate` always returns [`SandboxGateDecision::Allow`], regardless
    /// of cached state.
    pub prereq_check_enabled: bool,
}

impl Default for SandboxSettings {
    fn default() -> Self {
        Self {
            on_missing_runtime: OnMissingRuntime::Block,
            prereq_check_enabled: true,
        }
    }
}

impl Settings for SandboxSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let Some(content) = content.paddleboard_sandbox.as_ref() else {
            return Self::default();
        };
        Self {
            on_missing_runtime: content
                .on_missing_runtime
                .as_ref()
                .map(|v| (*v).into())
                .unwrap_or_default(),
            prereq_check_enabled: content.prereq_check_enabled.unwrap_or(true),
        }
    }
}

impl From<settings::PaddleboardOnMissingRuntimeContent> for OnMissingRuntime {
    fn from(value: settings::PaddleboardOnMissingRuntimeContent) -> Self {
        match value {
            settings::PaddleboardOnMissingRuntimeContent::Block => OnMissingRuntime::Block,
            settings::PaddleboardOnMissingRuntimeContent::FallBackToHost => {
                OnMissingRuntime::FallBackToHost
            }
            settings::PaddleboardOnMissingRuntimeContent::WarnOnce => OnMissingRuntime::WarnOnce,
        }
    }
}

/// What a sandbox-aware call site should do given the current prereqs status
/// and user policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxGateDecision {
    /// Proceed sandboxed.
    Allow,
    /// Prereqs are missing and the user has opted to run on the host instead.
    /// The reason is included for logging / debug surfaces.
    FallBackToHost { reason: String },
    /// Prereqs are missing and policy is `block`. The call site should
    /// surface the install modal and return this error to its caller.
    Block { reason: String },
    /// Prereqs are missing but policy is `warn_once`. The call site should
    /// emit a notification with this message and then proceed sandboxed.
    WarnOnce { reason: String },
}

/// Decide what to do given the cached probe result and user policy. Pure so
/// the policy matrix is testable without a real podman host.
pub fn decide_gate(
    prereqs: Option<&SandboxStatus>,
    settings: &SandboxSettings,
) -> SandboxGateDecision {
    if !settings.prereq_check_enabled {
        return SandboxGateDecision::Allow;
    }

    // If we have no cached status yet (probe still in flight, or running on a
    // platform where the probe failed entirely), don't block — the underlying
    // podman invocation will surface its own error if it's truly broken. The
    // gate exists to *improve* UX, not to invent new failure modes.
    let Some(status) = prereqs else {
        return SandboxGateDecision::Allow;
    };

    if status.is_satisfied() {
        return SandboxGateDecision::Allow;
    }

    let reason = describe_missing(status);
    match settings.on_missing_runtime {
        OnMissingRuntime::Block => SandboxGateDecision::Block { reason },
        OnMissingRuntime::FallBackToHost => SandboxGateDecision::FallBackToHost { reason },
        OnMissingRuntime::WarnOnce => SandboxGateDecision::WarnOnce { reason },
    }
}

fn describe_missing(status: &SandboxStatus) -> String {
    use paddleboard_sandbox_prereqs::{GvisorStatus, PodmanStatus};
    match (&status.podman, &status.gvisor) {
        (PodmanStatus::Missing, _) => "podman is not installed on this host".to_string(),
        (PodmanStatus::InstalledNotRunning { .. }, _) => {
            "podman is installed but its daemon (machine) is not reachable".to_string()
        }
        (PodmanStatus::Ready { .. }, GvisorStatus::NotConfigured) => {
            "gVisor runtime (runsc) is not registered with podman".to_string()
        }
        (PodmanStatus::Ready { .. }, GvisorStatus::Unknown) => {
            "sandbox runtime status could not be determined".to_string()
        }
        _ => "sandbox prerequisites are not satisfied".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddleboard_sandbox_prereqs::{GvisorStatus, PodmanStatus, SandboxStatus};

    fn happy() -> SandboxStatus {
        SandboxStatus {
            podman: PodmanStatus::Ready {
                version: "podman version 5.0.0".into(),
            },
            gvisor: GvisorStatus::Available,
        }
    }

    fn missing_podman() -> SandboxStatus {
        SandboxStatus {
            podman: PodmanStatus::Missing,
            gvisor: GvisorStatus::Unknown,
        }
    }

    fn missing_gvisor() -> SandboxStatus {
        SandboxStatus {
            podman: PodmanStatus::Ready {
                version: "podman version 5.0.0".into(),
            },
            gvisor: GvisorStatus::NotConfigured,
        }
    }

    #[test]
    fn allow_when_prereqs_are_satisfied() {
        let s = SandboxSettings::default();
        assert_eq!(decide_gate(Some(&happy()), &s), SandboxGateDecision::Allow);
    }

    #[test]
    fn allow_when_status_not_yet_probed() {
        let s = SandboxSettings::default();
        assert_eq!(decide_gate(None, &s), SandboxGateDecision::Allow);
    }

    #[test]
    fn allow_when_prereq_check_disabled() {
        let s = SandboxSettings {
            prereq_check_enabled: false,
            on_missing_runtime: OnMissingRuntime::Block,
        };
        assert_eq!(
            decide_gate(Some(&missing_podman()), &s),
            SandboxGateDecision::Allow
        );
    }

    #[test]
    fn block_is_default_when_podman_missing() {
        let s = SandboxSettings::default();
        let decision = decide_gate(Some(&missing_podman()), &s);
        assert!(matches!(decision, SandboxGateDecision::Block { .. }));
    }

    #[test]
    fn block_when_gvisor_missing_with_default_policy() {
        let s = SandboxSettings::default();
        let decision = decide_gate(Some(&missing_gvisor()), &s);
        match decision {
            SandboxGateDecision::Block { reason } => {
                assert!(reason.contains("gVisor"));
            }
            other => panic!("expected Block, got {other:?}"),
        }
    }

    #[test]
    fn fall_back_to_host_policy_returns_fallback() {
        let s = SandboxSettings {
            prereq_check_enabled: true,
            on_missing_runtime: OnMissingRuntime::FallBackToHost,
        };
        let decision = decide_gate(Some(&missing_podman()), &s);
        assert!(matches!(decision, SandboxGateDecision::FallBackToHost { .. }));
    }

    #[test]
    fn warn_once_policy_returns_warning() {
        let s = SandboxSettings {
            prereq_check_enabled: true,
            on_missing_runtime: OnMissingRuntime::WarnOnce,
        };
        let decision = decide_gate(Some(&missing_gvisor()), &s);
        assert!(matches!(decision, SandboxGateDecision::WarnOnce { .. }));
    }
}
